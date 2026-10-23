//! `--bench <seconds>`: the frame-time baseline that M6 must beat (m4-plan
//! Decision 13).
//!
//! Once the join's meshing has settled, the camera rises [`RISE`] blocks
//! above where the host spawned, looks [`PITCH`] degrees down, and flies east
//! at creative speed for the given time. Then the table is logged and the
//! window closes. The window asks for a present mode without vsync, so frame
//! times show the work, not the display's refresh rate. Run it in a release
//! build, on a fresh copy of one template world, so that runs differ only in
//! what's being compared (STATUS.md, Dev environment).
//!
//! **Percentiles, not just the average** (worth teaching). A stutter is a
//! few slow frames among thousands of quick ones. It barely moves the
//! average, but it's what the eye notices. The p99 frame time is the one
//! that 99% of frames beat, so one frame in a hundred is slower.

use std::time::{Duration, Instant};

use tracing::info;

use crate::render::{
    camera::{Camera, FLY_SPEED, Movement},
    meshes::MeshStats,
};

/// How far above the host's spawn the camera flies, in blocks: two
/// sub-chunks, so it looks over the terrain around spawn instead of flying
/// through it. A template world fixes the spawn, so every run flies the same
/// path.
const RISE: f64 = 32.0;
/// How far the camera looks down, in degrees (Bedrock's pitch: positive is
/// down).
const PITCH: f32 = 15.0;
/// East: Bedrock's yaw −90 faces +x.
const YAW: f32 = -90.0;

/// What one frame of the flight drew.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Since the frame before.
    pub time: Duration,
    pub draws: u32,
    pub visible: u32,
    /// Meshes, faces and vertex-buffer bytes on the GPU.
    pub meshes: usize,
    pub quads: u64,
    pub gpu_bytes: u64,
    /// Chunks loaded.
    pub chunks: usize,
}

pub struct Bench {
    duration: Duration,
    radius: i32,
    state: State,
}

enum State {
    /// The join's meshing hasn't settled yet.
    Waiting,
    Flying(Flight),
    /// The table is logged.
    Done,
}

struct Flight {
    started: Instant,
    present_mode: wgpu::PresentMode,
    samples: Vec<Sample>,
    /// Chunks loaded when the flight began: the join's.
    join_chunks: usize,
    /// Meshing's totals when the flight began.
    meshed: u64,
    mesh_time: Duration,
}

impl Bench {
    pub const fn new(duration: Duration, radius: i32) -> Self {
        Self {
            duration,
            radius,
            state: State::Waiting,
        }
    }

    pub const fn waiting(&self) -> bool {
        matches!(self.state, State::Waiting)
    }

    pub const fn is_done(&self) -> bool {
        matches!(self.state, State::Done)
    }

    /// Starts the flight: the camera rises above the spawn and turns east.
    pub fn start(&mut self, camera: &mut Camera, chunks: usize, stats: &MeshStats, present_mode: wgpu::PresentMode) {
        camera.position.y += RISE;
        camera.pitch = PITCH;
        camera.yaw = YAW;
        let p = camera.position;
        info!(
            "Bench: the join's meshing has settled; flying east for {} s at {FLY_SPEED} blocks a second \
             from ({:.1}, {:.1}, {:.1}), looking {PITCH}° down",
            self.duration.as_secs(),
            p.x,
            p.y,
            p.z,
        );
        self.state = State::Flying(Flight {
            started: Instant::now(),
            present_mode,
            samples: Vec::new(),
            join_chunks: chunks,
            meshed: stats.meshed,
            mesh_time: stats.mesh_time,
        });
    }

    /// Where the camera goes this frame: east while flying, nowhere
    /// otherwise, whatever the keys and the mouse say.
    pub fn movement(&self) -> Movement {
        match self.state {
            State::Flying(_) => Movement {
                forward: 1.0,
                ..Movement::default()
            },
            State::Waiting | State::Done => Movement::default(),
        }
    }

    /// Records a drawn frame. Once the flight has lasted its time, logs the
    /// table and returns true: the window can close.
    pub fn record(&mut self, sample: Sample, stats: &MeshStats) -> bool {
        let State::Flying(flight) = &mut self.state else {
            return false;
        };
        flight.samples.push(sample);
        if flight.started.elapsed() < self.duration {
            return false;
        }
        if let Some(summary) = summarise(&flight.samples) {
            log_table(&summary, flight, stats, self.radius);
        }
        self.state = State::Done;
        true
    }
}

/// The flight's numbers. Frame times are in milliseconds.
#[derive(Debug, PartialEq)]
struct Summary {
    frames: usize,
    seconds: f64,
    average: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    draws_average: f64,
    draws_max: u32,
    visible_average: f64,
    meshes_max: usize,
    quads_max: u64,
    bytes_max: u64,
    chunks_max: usize,
    /// The last frame's sample: what was loaded at the end.
    last: Sample,
}

fn summarise(samples: &[Sample]) -> Option<Summary> {
    let last = *samples.last()?;
    let mut times: Vec<f64> = samples.iter().map(|s| s.time.as_secs_f64() * 1e3).collect();
    times.sort_unstable_by(f64::total_cmp);
    let frames = samples.len();
    let n = frames as f64;
    let total: f64 = times.iter().sum();
    let average_of = |value: fn(&Sample) -> f64| samples.iter().map(value).sum::<f64>() / n;
    Some(Summary {
        frames,
        seconds: total / 1e3,
        average: total / n,
        p50: percentile(&times, 50.0),
        p95: percentile(&times, 95.0),
        p99: percentile(&times, 99.0),
        max: times[frames - 1],
        draws_average: average_of(|s| f64::from(s.draws)),
        draws_max: samples.iter().map(|s| s.draws).max().unwrap_or(0),
        visible_average: average_of(|s| f64::from(s.visible)),
        meshes_max: samples.iter().map(|s| s.meshes).max().unwrap_or(0),
        quads_max: samples.iter().map(|s| s.quads).max().unwrap_or(0),
        bytes_max: samples.iter().map(|s| s.gpu_bytes).max().unwrap_or(0),
        chunks_max: samples.iter().map(|s| s.chunks).max().unwrap_or(0),
        last,
    })
}

/// The nearest-rank percentile of `sorted`: the smallest value that `p`
/// percent of the values are no greater than.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn log_table(summary: &Summary, flight: &Flight, stats: &MeshStats, radius: i32) {
    let s = summary;
    let build = if cfg!(debug_assertions) {
        "a debug build, so these numbers mean little"
    } else {
        "a release build"
    };
    info!(
        "Bench table, radius {radius}, {:?}, {build}: {} frames in {:.1} s, {:.0} fps",
        flight.present_mode,
        s.frames,
        s.seconds,
        s.frames as f64 / s.seconds,
    );
    info!(
        "Bench frame times: average {:.2} ms, p50 {:.2}, p95 {:.2}, p99 {:.2}, max {:.2}",
        s.average, s.p50, s.p95, s.p99, s.max
    );
    info!(
        "Bench draws: {:.0} draw calls a frame on average (at most {}), {:.0} meshes in view",
        s.draws_average, s.draws_max, s.visible_average
    );
    info!(
        "Bench GPU: {} meshes, {} faces, {:.1} MB at the end; at most {} meshes, {} faces, {:.1} MB",
        s.last.meshes,
        s.last.quads,
        s.last.gpu_bytes as f64 / 1e6,
        s.meshes_max,
        s.quads_max,
        s.bytes_max as f64 / 1e6,
    );
    let meshed = stats.meshed - flight.meshed;
    let micros = if meshed == 0 {
        0.0
    } else {
        (stats.mesh_time - flight.mesh_time).as_secs_f64() * 1e6 / meshed as f64
    };
    info!("Bench meshing: {meshed} sub-chunks during the flight, {micros:.0} µs each");
    info!(
        "Bench chunks: {} after the join, at most {} during the flight, {} at the end",
        flight.join_chunks, s.chunks_max, s.last.chunks
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        let values: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&values, 50.0), 50.0);
        assert_eq!(percentile(&values, 95.0), 95.0);
        assert_eq!(percentile(&values, 99.0), 99.0);
        assert_eq!(percentile(&values, 100.0), 100.0);
        // One value is every percentile; a rank never falls off either end.
        assert_eq!(percentile(&[7.0], 50.0), 7.0);
        assert_eq!(percentile(&[7.0], 0.0), 7.0);
        // Ten values: p95 rounds its rank, 9.5, up to the 10th.
        let ten: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile(&ten, 95.0), 10.0);
    }

    #[test]
    fn the_summary_averages_and_ranks_the_frames() {
        let frame = |ms: u64, draws: u32, chunks: usize| Sample {
            time: Duration::from_millis(ms),
            draws,
            visible: draws,
            meshes: chunks * 10,
            quads: chunks as u64 * 100,
            gpu_bytes: chunks as u64 * 6400,
            chunks,
        };
        // Out of order on purpose: the percentiles sort, the averages don't care.
        let samples = [frame(4, 10, 5), frame(1, 30, 7), frame(3, 20, 6), frame(2, 40, 4)];
        let summary = summarise(&samples).unwrap();
        assert_eq!(summary.frames, 4);
        assert!((summary.seconds - 0.010).abs() < 1e-9);
        assert!((summary.average - 2.5).abs() < 1e-9);
        assert_eq!((summary.p50, summary.p95, summary.p99, summary.max), (2.0, 4.0, 4.0, 4.0));
        assert!((summary.draws_average - 25.0).abs() < 1e-9);
        assert_eq!(summary.draws_max, 40);
        assert_eq!((summary.chunks_max, summary.meshes_max), (7, 70));
        assert_eq!((summary.quads_max, summary.bytes_max), (700, 44800));
        // The end is the last frame recorded, not the largest.
        assert_eq!(summary.last.chunks, 4);
        assert_eq!(summarise(&[]), None);
    }

    #[test]
    fn only_a_flight_moves_the_camera() {
        let mut bench = Bench::new(Duration::from_secs(30), 8);
        assert_eq!(bench.movement(), Movement::default());
        assert!(!bench.record(Sample::default(), &MeshStats::default()), "nothing recorded while waiting");

        let mut camera = Camera::new(glam::DVec3::new(0.5, 70.0, 0.5), 0.0, 0.0);
        bench.start(&mut camera, 289, &MeshStats::default(), wgpu::PresentMode::Immediate);
        assert_eq!(camera.position.y, 70.0 + RISE);
        assert_eq!((camera.pitch, camera.yaw), (PITCH, YAW));
        assert_eq!(bench.movement().forward, 1.0);

        // Flying east: Bedrock yaw −90 is +x.
        camera.fly(bench.movement(), 1.0);
        assert!((camera.position.x - (0.5 + FLY_SPEED)).abs() < 1e-6);
        assert!((camera.position.y - (70.0 + RISE)).abs() < 1e-9, "the height stays fixed");
        assert!((camera.position.z - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_flight_ends_after_its_time() {
        let mut bench = Bench::new(Duration::ZERO, 8);
        let mut camera = Camera::new(glam::DVec3::ZERO, 0.0, 0.0);
        bench.start(&mut camera, 289, &MeshStats::default(), wgpu::PresentMode::Fifo);
        assert!(!bench.is_done());
        assert!(bench.record(Sample::default(), &MeshStats::default()));
        assert!(bench.is_done());
        assert_eq!(bench.movement(), Movement::default(), "the camera stops");
    }
}
