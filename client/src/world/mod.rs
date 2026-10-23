//! The client's picture of the world, built only from what the server sends.
//!
//! This is the wall between networking and rendering (PLAN.md, "Client
//! core"): packets go in through [`World::apply`], and everything else, the
//! map now and the renderer from M4, reads the world, never packets.
//!
//! Chunks are stored the way Bedrock sends them: palette-compressed
//! sub-chunks of block network ids in XZY order. LAN guests and P2P peers
//! only ever get bytes, so this one format serves every transport.

pub mod chunk;
pub mod level_chunk;
pub mod storage;

use std::collections::{HashMap, hash_map::Entry};

use pumpkin_core::net::bedrock::level_chunk::CLevelChunk;
use pumpkin_data::{Block, BlockState, BlockStateId};
use pumpkin_protocol::{
    RawPacket,
    bedrock::client::{
        network_chunk_publisher_update::CNetworkChunkPublisherUpdate, update_block::CUpdateBlock,
    },
    packet::Packet,
    serial::PacketRead,
};
use tracing::{debug, info};

use self::{chunk::Chunk, level_chunk::DecodeError};

/// Air's Bedrock network id, which a missing sub-chunk or layer reads as.
pub const AIR: u32 = BlockState::to_be_network_id(Block::AIR.default_state.id);

/// How many chunks beyond the server's published radius the world keeps.
///
/// Pumpkin's view isn't the circle it publishes. It takes 2 off each axis
/// distance before testing the circle (`Cylindrical::is_within_distance`),
/// so the view reaches r + 1 chunks along the axes and r + 2√2 ≈ r + 2.83 on
/// the diagonals (m4-plan Finding 10). And Pumpkin never resends a chunk it
/// believes we still have, so a chunk dropped while still in its view would
/// stay a hole. A margin of 3 covers the whole view; a test checks it at
/// every radius from 2 to 32.
pub const VIEW_MARGIN: i32 = 3;

/// What a packet did to the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// A chunk was decoded and stored (replacing any older copy).
    Chunk { x: i32, z: i32 },
    /// A block changed.
    Block { x: i32, y: i32, z: i32 },
    /// A block update for a chunk we don't have; counted and ignored.
    BlockNotLoaded,
    /// The server's view moved; chunks it has left were dropped.
    View { dropped: usize },
    /// The world doesn't use this packet.
    Ignored,
}

/// A change to the world, for whoever draws it ([`World::take_changes`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// A chunk arrived, new or replacing an older copy.
    Chunk { x: i32, z: i32 },
    /// The block at this position changed, on either layer.
    Block { x: i32, y: i32, z: i32 },
    /// A chunk was dropped, because the server's view left it.
    Dropped { x: i32, z: i32 },
}

/// The server's view, from a `NetworkChunkPublisherUpdate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    /// The chunk the view is centred on.
    pub center: (i32, i32),
    /// Its radius in chunks.
    pub radius: i32,
}

impl View {
    /// Whether a chunk is kept: within [`VIEW_MARGIN`] chunks past the
    /// published radius of the centre, measured in a straight line.
    #[must_use]
    pub const fn keeps(&self, x: i32, z: i32) -> bool {
        let (dx, dz) = ((x - self.center.0) as i64, (z - self.center.1) as i64);
        let keep = (self.radius + VIEW_MARGIN) as i64;
        dx * dx + dz * dz <= keep * keep
    }
}

#[derive(Debug)]
pub enum ApplyError {
    Chunk(DecodeError),
    UpdateBlock(std::io::Error),
    /// A layer other than 0 (the block) or 1 (its water).
    Layer(u32),
    PublisherUpdate(std::io::Error),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Chunk(err) => write!(f, "bad LevelChunk: {err}"),
            Self::UpdateBlock(err) => write!(f, "bad UpdateBlock: {err}"),
            Self::Layer(layer) => write!(f, "UpdateBlock for layer {layer}"),
            Self::PublisherUpdate(err) => write!(f, "bad NetworkChunkPublisherUpdate: {err}"),
        }
    }
}

pub struct World {
    chunks: HashMap<(i32, i32), Chunk>,
    blocks: BlockTable,
    /// `UpdateBlock`s that arrived for a chunk we don't have.
    updates_not_loaded: u64,
    /// What changed since [`World::take_changes`], when tracking. `None`
    /// otherwise, so a world nobody draws keeps no log.
    changes: Option<Vec<Change>>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    #[must_use]
    pub fn new() -> Self {
        let blocks = BlockTable::build();
        blocks.log();
        Self {
            chunks: HashMap::new(),
            blocks,
            updates_not_loaded: 0,
            changes: None,
        }
    }

    /// Starts recording what changes, for [`World::take_changes`].
    pub fn track_changes(&mut self) {
        self.changes.get_or_insert_with(Vec::new);
    }

    /// What changed since the last call, oldest first. Empty unless
    /// [`World::track_changes`] was called.
    pub fn take_changes(&mut self) -> Vec<Change> {
        self.changes.as_mut().map(std::mem::take).unwrap_or_default()
    }

    fn record(&mut self, change: Change) {
        if let Some(changes) = &mut self.changes {
            changes.push(change);
        }
    }

    /// Applies one packet from the server. Packets the world doesn't use are
    /// [`Applied::Ignored`].
    ///
    /// # Errors
    /// If the packet can't be decoded; the world is unchanged.
    pub fn apply(&mut self, packet: &RawPacket) -> Result<Applied, ApplyError> {
        match packet.id {
            CLevelChunk::PACKET_ID => {
                let chunk = level_chunk::decode(&packet.payload).map_err(ApplyError::Chunk)?;
                let (x, z) = (chunk.x, chunk.z);
                self.chunks.insert((x, z), chunk);
                self.record(Change::Chunk { x, z });
                Ok(Applied::Chunk { x, z })
            }
            CUpdateBlock::PACKET_ID => {
                let mut body = packet.payload.as_ref();
                let update = CUpdateBlock::read(&mut body).map_err(ApplyError::UpdateBlock)?;
                let layer = update.layer.0;
                if layer > 1 {
                    return Err(ApplyError::Layer(layer));
                }
                let pos = update.block_position.0;
                if self.set_block(pos.x, pos.y, pos.z, layer as usize, update.block_runtime_id.0) {
                    let (x, y, z) = (pos.x, pos.y, pos.z);
                    self.record(Change::Block { x, y, z });
                    Ok(Applied::Block { x, y, z })
                } else {
                    self.updates_not_loaded += 1;
                    Ok(Applied::BlockNotLoaded)
                }
            }
            CNetworkChunkPublisherUpdate::PACKET_ID => {
                let mut body = packet.payload.as_ref();
                let update =
                    CNetworkChunkPublisherUpdate::read(&mut body).map_err(ApplyError::PublisherUpdate)?;
                let pos = update.pos_for_view.0;
                // The radius is in blocks: the view distance times 16.
                let radius = update.new_radius.0.div_ceil(16);
                let view = View {
                    center: (pos.x >> 4, pos.z >> 4),
                    radius: i32::try_from(radius).unwrap_or(i32::MAX - VIEW_MARGIN),
                };
                Ok(Applied::View {
                    dropped: self.set_view(view),
                })
            }
            _ => Ok(Applied::Ignored),
        }
    }

    /// Follows the server's view: drops every chunk it no longer keeps, and
    /// returns how many. A chunk that arrives later is kept until the next
    /// update, since the server only sends chunks inside its view.
    fn set_view(&mut self, view: View) -> usize {
        let before = self.chunks.len();
        let mut dropped = Vec::new();
        self.chunks.retain(|&(x, z), _| {
            let keep = view.keeps(x, z);
            if !keep {
                dropped.push((x, z));
            }
            keep
        });
        for &(x, z) in &dropped {
            self.record(Change::Dropped { x, z });
        }
        if !dropped.is_empty() {
            debug!(
                "View at chunk ({}, {}), radius {}: dropped {} of {} chunks",
                view.center.0,
                view.center.1,
                view.radius,
                dropped.len(),
                before,
            );
        }
        dropped.len()
    }

    /// Sets a block by world position. Returns false if its chunk (or its
    /// sub-chunk) isn't loaded.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, layer: usize, id: u32) -> bool {
        self.chunks
            .get_mut(&(x >> 4, z >> 4))
            .is_some_and(|chunk| chunk.set_block((x & 15) as usize, y, (z & 15) as usize, layer, id))
    }

    /// The block network id at a world position, or `None` if its chunk isn't
    /// loaded.
    #[must_use]
    pub fn block(&self, x: i32, y: i32, z: i32, layer: usize) -> Option<u32> {
        self.chunk(x >> 4, z >> 4)
            .map(|chunk| chunk.block((x & 15) as usize, y, (z & 15) as usize, layer))
    }

    #[must_use]
    pub fn chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
        self.chunks.get(&(x, z))
    }

    pub fn chunks(&self) -> impl Iterator<Item = &Chunk> {
        self.chunks.values()
    }

    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    #[must_use]
    pub const fn blocks(&self) -> &BlockTable {
        &self.blocks
    }

    #[must_use]
    pub const fn updates_not_loaded(&self) -> u64 {
        self.updates_not_loaded
    }
}

/// Maps Bedrock block network ids back to Java block states, for map colours
/// and for naming blocks in logs.
///
/// It isn't a true inverse. Several Java states can share one Bedrock id:
/// Java-only properties such as leaves' `distance` or `waterlogged` collapse.
/// The table is built by walking the Java state ids in ascending order, and
/// the lowest Java state wins, so it's the same on every run.
pub struct BlockTable {
    java_by_bedrock: HashMap<u32, BlockStateId>,
    /// Java states whose Bedrock id an earlier (lower) state already had.
    collisions: usize,
    /// A few of them, as (Bedrock id, kept, dropped), for the debug log.
    examples: Vec<(u32, BlockStateId, BlockStateId)>,
}

impl BlockTable {
    #[must_use]
    pub fn build() -> Self {
        let mut java_by_bedrock = HashMap::new();
        let mut collisions = 0;
        let mut examples = Vec::new();
        for raw in 0..BlockStateId::COUNT {
            let Some(state) = BlockStateId::new(raw) else {
                continue;
            };
            let bedrock = BlockState::to_be_network_id(state);
            match java_by_bedrock.entry(bedrock) {
                Entry::Vacant(entry) => {
                    entry.insert(state);
                }
                Entry::Occupied(entry) => {
                    collisions += 1;
                    if examples.len() < 5 {
                        examples.push((bedrock, *entry.get(), state));
                    }
                }
            }
        }
        Self {
            java_by_bedrock,
            collisions,
            examples,
        }
    }

    fn log(&self) {
        info!(
            "Block table: {} Bedrock ids for {} Java states; {} Java states share a Bedrock id \
             with a lower one, which wins",
            self.java_by_bedrock.len(),
            BlockStateId::COUNT,
            self.collisions,
        );
        for &(bedrock, kept, dropped) in &self.examples {
            debug!(
                "Bedrock id {bedrock:#010x}: kept Java state {} ({}), dropped {} ({})",
                kept.as_u16(),
                block_name(kept),
                dropped.as_u16(),
                block_name(dropped),
            );
        }
    }

    /// The lowest Java state with this Bedrock id.
    #[must_use]
    pub fn java_state(&self, bedrock: u32) -> Option<BlockStateId> {
        self.java_by_bedrock.get(&bedrock).copied()
    }

    /// The block's name, or the id in hex if it isn't known.
    #[must_use]
    pub fn name(&self, bedrock: u32) -> String {
        self.java_state(bedrock)
            .map_or_else(|| format!("unknown {bedrock:#010x}"), |state| block_name(state).to_string())
    }
}

fn block_name(state: BlockStateId) -> &'static str {
    Block::from_state_id(state).name
}

#[cfg(test)]
mod tests;
