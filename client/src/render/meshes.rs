//! The GPU meshes, one per sub-chunk (m4-plan Decision 6): which sub-chunks
//! need meshing, meshing them nearest first within a time budget, and the
//! vertex buffers the terrain draws.
//!
//! **What makes a sub-chunk dirty:** a chunk arriving (all its sub-chunks,
//! and its neighbours', whose borders it changes); a block changing (its
//! sub-chunk, and the one across a border it's on). A dropped chunk's meshes
//! go. A sub-chunk whose chunk lacks a side neighbour waits: the neighbour's
//! arrival marks it dirty again.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use glam::DVec3;
use tracing::{debug, info};
use wgpu::util::DeviceExt;

use super::{
    mesher::{self, Neighbourhood},
    tint::Tints,
};
use crate::{
    blocks::appearance::AppearanceTable,
    world::{AIR, Change, World, storage::Storage},
};

/// How long meshing may take each pass of the loop.
pub const BUDGET: Duration = Duration::from_millis(4);
/// How often the queue is re-sorted while new work keeps arriving.
const RESORT: Duration = Duration::from_millis(100);
/// A join is over once no chunk has arrived for this long, the same rule as
/// the headless run's chunk wait. Early on, the queue can empty while chunks
/// are still coming: most sub-chunks then wait for a side neighbour.
const JOIN_QUIET: Duration = Duration::from_secs(2);

/// A sub-chunk: its chunk's x and z, and its y in sixteen-block sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubChunkKey {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SubChunkKey {
    /// The sub-chunk holding a world position.
    fn containing(position: DVec3) -> Self {
        let section = |c: f64| (c.floor() as i32) >> 4;
        Self {
            x: section(position.x),
            y: section(position.y),
            z: section(position.z),
        }
    }

    /// The sub-chunk's lowest corner, in blocks.
    #[must_use]
    pub const fn origin(self) -> [i32; 3] {
        [self.x * 16, self.y * 16, self.z * 16]
    }

    /// Squared distance between two sub-chunks, in sub-chunks.
    const fn distance_squared(self, other: Self) -> i64 {
        let (dx, dy, dz) = (
            (self.x - other.x) as i64,
            (self.y - other.y) as i64,
            (self.z - other.z) as i64,
        );
        dx * dx + dy * dy + dz * dz
    }
}

/// One sub-chunk's vertices on the GPU: the opaque quads, then the blended.
pub struct GpuMesh {
    pub buffer: wgpu::Buffer,
    pub opaque_quads: u32,
    pub blend_quads: u32,
    pub origin: [i32; 3],
}

/// What meshing has done, for the log and the title.
#[derive(Debug, Default)]
pub struct MeshStats {
    /// Sub-chunks meshed (gathered and meshed, empty or not).
    pub meshed: u64,
    /// The time that took, uploads not included.
    pub mesh_time: Duration,
    /// Quads in the meshes built.
    pub quads_built: u64,
    /// Bytes of vertex buffers alive now.
    pub gpu_bytes: u64,
}

impl MeshStats {
    /// The average time to mesh a sub-chunk, in microseconds.
    #[must_use]
    pub fn micros_each(&self) -> f64 {
        if self.meshed == 0 {
            0.0
        } else {
            self.mesh_time.as_secs_f64() * 1e6 / self.meshed as f64
        }
    }
}

#[derive(Default)]
pub struct Meshes {
    meshes: HashMap<SubChunkKey, GpuMesh>,
    /// Sub-chunks to mesh again.
    dirty: HashSet<SubChunkKey>,
    /// Dirty sub-chunks, furthest first, so the nearest pops off the end.
    /// It can hold keys meshed or dropped since; those are skipped.
    queue: Vec<SubChunkKey>,
    /// When the queue was last sorted, and around which sub-chunk.
    sorted: Option<(Instant, SubChunkKey)>,
    /// Whether sub-chunks became dirty since the last sort.
    fresh_work: bool,
    /// The most quads in any mesh, which the shared index buffer must cover.
    max_quads: u32,
    pub stats: MeshStats,
    /// When the first chunk arrived, and the latest.
    join_started: Option<Instant>,
    last_chunk: Option<Instant>,
    /// When the queue last ran empty, if it still is.
    emptied_at: Option<Instant>,
    /// Whether work arrived since meshing last caught up.
    busy: bool,
    caught_up_once: bool,
}

impl Meshes {
    /// Marks what the world's changes make dirty, and drops the meshes of
    /// chunks it dropped.
    pub fn apply(&mut self, changes: Vec<Change>, world: &World) {
        for change in changes {
            match change {
                Change::Chunk { x, z } => {
                    let now = Instant::now();
                    self.join_started.get_or_insert(now);
                    self.last_chunk = Some(now);
                    for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                        self.mark_column(world, x + dx, z + dz);
                    }
                }
                Change::Block { x, y, z } => {
                    let key = SubChunkKey {
                        x: x >> 4,
                        y: y >> 4,
                        z: z >> 4,
                    };
                    self.mark(key);
                    // On a border, the face beside it is in the neighbour's mesh.
                    let edge = |c: i32| match c & 15 {
                        0 => -1,
                        15 => 1,
                        _ => 0,
                    };
                    for (dx, dy, dz) in [(edge(x), 0, 0), (0, edge(y), 0), (0, 0, edge(z))] {
                        if (dx, dy, dz) != (0, 0, 0) {
                            self.mark(SubChunkKey {
                                x: key.x + dx,
                                y: key.y + dy,
                                z: key.z + dz,
                            });
                        }
                    }
                }
                Change::Dropped { x, z } => {
                    let freed: u64 = self
                        .meshes
                        .iter()
                        .filter(|(key, _)| key.x == x && key.z == z)
                        .map(|(_, mesh)| mesh.buffer.size())
                        .sum();
                    self.meshes.retain(|key, _| !(key.x == x && key.z == z));
                    self.dirty.retain(|key| !(key.x == x && key.z == z));
                    self.stats.gpu_bytes -= freed;
                }
            }
        }
    }

    fn mark(&mut self, key: SubChunkKey) {
        if self.dirty.insert(key) {
            self.fresh_work = true;
            self.busy = true;
            self.emptied_at = None;
        }
    }

    fn mark_column(&mut self, world: &World, x: i32, z: i32) {
        if let Some(chunk) = world.chunk(x, z) {
            for i in 0..chunk.sub_chunks.len() {
                self.mark(SubChunkKey {
                    x,
                    y: chunk.min_section + i as i32,
                    z,
                });
            }
        }
    }

    /// Meshes dirty sub-chunks, nearest to `camera` first, until `budget`
    /// is spent, and uploads the results.
    pub fn build(
        &mut self,
        device: &wgpu::Device,
        world: &World,
        table: &AppearanceTable,
        tints: &Tints,
        camera: DVec3,
        budget: Duration,
    ) {
        if self.dirty.is_empty() {
            self.check_caught_up();
            return;
        }
        let start = Instant::now();
        let center = SubChunkKey::containing(camera);
        // Sorting thousands of keys every frame would eat the budget during
        // a join, while chunks arrive every frame. So re-sort when the queue
        // runs out, when the camera enters another sub-chunk, or every
        // `RESORT` while new work arrives; until then new work waits.
        let resort = match self.sorted {
            None => true,
            Some((at, around)) => {
                self.queue.is_empty() || around != center || (self.fresh_work && at.elapsed() >= RESORT)
            }
        };
        if resort {
            self.queue = self.dirty.iter().copied().collect();
            self.queue
                .sort_unstable_by_key(|key| std::cmp::Reverse(key.distance_squared(center)));
            self.sorted = Some((Instant::now(), center));
            self.fresh_work = false;
        }
        while let Some(key) = self.queue.pop() {
            if self.dirty.remove(&key) {
                self.mesh_one(device, world, table, tints, key);
            }
            if start.elapsed() >= budget {
                break;
            }
        }
        if self.dirty.is_empty() {
            self.emptied_at.get_or_insert_with(Instant::now);
            self.check_caught_up();
        }
    }

    fn mesh_one(
        &mut self,
        device: &wgpu::Device,
        world: &World,
        table: &AppearanceTable,
        tints: &Tints,
        key: SubChunkKey,
    ) {
        let Some(chunk) = world.chunk(key.x, key.z) else {
            self.remove(key);
            return;
        };
        let Some((index, sub_chunk)) = usize::try_from(key.y - chunk.min_section)
            .ok()
            .and_then(|i| Some((i, chunk.sub_chunks.get(i)?)))
        else {
            self.remove(key);
            return;
        };
        if sub_chunk.layers.iter().all(|layer| *layer == Storage::Single(AIR)) {
            self.remove(key);
            return;
        }
        let started = Instant::now();
        // A missing side neighbour: its arrival will mark this dirty again.
        let Some(neighbourhood) = Neighbourhood::gather(world, table, key.x, index, key.z) else {
            return;
        };
        let mesh = mesher::mesh(&neighbourhood, tints);
        self.stats.meshed += 1;
        self.stats.mesh_time += started.elapsed();
        self.stats.quads_built += mesh.quads() as u64;
        if mesh.is_empty() {
            self.remove(key);
            return;
        }

        let opaque_quads = (mesh.opaque.len() / 4) as u32;
        let blend_quads = (mesh.blend.len() / 4) as u32;
        let mut vertices = mesh.opaque;
        vertices.extend_from_slice(&mesh.blend);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sub-chunk mesh"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.max_quads = self.max_quads.max(opaque_quads).max(blend_quads);
        self.stats.gpu_bytes += buffer.size();
        let mesh = GpuMesh {
            buffer,
            opaque_quads,
            blend_quads,
            origin: key.origin(),
        };
        if let Some(old) = self.meshes.insert(key, mesh) {
            self.stats.gpu_bytes -= old.buffer.size();
        }
    }

    fn remove(&mut self, key: SubChunkKey) {
        if let Some(old) = self.meshes.remove(&key) {
            self.stats.gpu_bytes -= old.buffer.size();
        }
    }

    /// Once the queue is empty and chunks have stopped arriving, logs that
    /// meshing caught up: the first time, at info level, with how long the
    /// join's meshing took; later, at debug level.
    fn check_caught_up(&mut self) {
        let (Some(started), Some(last_chunk), Some(emptied)) = (self.join_started, self.last_chunk, self.emptied_at)
        else {
            return;
        };
        if !self.busy || last_chunk.elapsed() < JOIN_QUIET {
            return;
        }
        self.busy = false;
        let stats = &self.stats;
        let line = format!(
            "{} sub-chunks meshed, {:.0} µs each, {} quads; {} meshes on the GPU, {:.1} MB",
            stats.meshed,
            stats.micros_each(),
            stats.quads_built,
            self.meshes.len(),
            stats.gpu_bytes as f64 / 1e6,
        );
        if self.caught_up_once {
            debug!("Meshing caught up: {line}");
        } else {
            info!(
                "Meshing caught up {:.1} s after the first chunk (the last came at {:.1} s): {line}",
                (emptied - started).as_secs_f64(),
                (last_chunk - started).as_secs_f64(),
            );
            self.caught_up_once = true;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &GpuMesh> {
        self.meshes.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    /// Sub-chunks waiting to be meshed.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.dirty.len()
    }

    /// Whether meshing has caught up with the join at least once.
    #[expect(dead_code, reason = "the benchmark waits for it (M4 Step 11)")]
    #[must_use]
    pub const fn settled(&self) -> bool {
        self.caught_up_once
    }

    #[must_use]
    pub const fn max_quads(&self) -> u32 {
        self.max_quads
    }
}
