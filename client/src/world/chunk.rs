//! A decoded chunk column: its sub-chunks, biomes and block actors, kept in
//! Bedrock's own format.

use pumpkin_nbt::compound::NbtCompound;

use super::{AIR, storage::Storage};

/// A 16×16×16 cube of blocks with one or two layers.
///
/// Layer 0 holds the block itself. Layer 1, when present, holds the water a
/// block sits in: Java keeps "waterlogged" inside the block state, Bedrock in
/// a second layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubChunk {
    pub layers: Vec<Storage>,
}

/// One chunk column, as the server sent it.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    #[cfg_attr(not(test), expect(dead_code, reason = "kept for M4, which renders other dimensions"))]
    pub dimension: i32,
    /// The section index (block y / 16) of `sub_chunks[0]`: -4 in the overworld.
    pub min_section: i32,
    /// Consecutive sub-chunks from the bottom up.
    pub sub_chunks: Vec<SubChunk>,
    /// Biome ids, one storage per sub-chunk and aligned with `sub_chunks`.
    pub biomes: Vec<Storage>,
    /// Block entities (chests, signs, ...) as Bedrock NBT.
    pub block_actors: Vec<NbtCompound>,
}

impl Chunk {
    /// The lowest block y the chunk holds.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.min_section * 16
    }

    /// One above the highest block y the chunk holds.
    #[must_use]
    pub const fn max_y(&self) -> i32 {
        (self.min_section + self.sub_chunks.len() as i32) * 16
    }

    /// Which sub-chunk holds block y, if any.
    fn section(&self, y: i32) -> Option<usize> {
        usize::try_from((y >> 4) - self.min_section)
            .ok()
            .filter(|&i| i < self.sub_chunks.len())
    }

    /// The block network id at `(x, y, z)` on `layer`: x and z within the
    /// chunk (0 to 15), y absolute. Anything the chunk doesn't hold, a missing
    /// sub-chunk or a missing layer, reads as air.
    #[must_use]
    pub fn block(&self, x: usize, y: i32, z: usize, layer: usize) -> u32 {
        self.section(y)
            .and_then(|i| self.sub_chunks[i].layers.get(layer))
            .map_or(AIR, |storage| storage.get(x, (y & 15) as usize, z))
    }

    /// The biome id at `(x, y, z)`, or `None` outside the chunk's height.
    #[must_use]
    pub fn biome(&self, x: usize, y: i32, z: usize) -> Option<u32> {
        let i = self.section(y)?;
        self.biomes.get(i).map(|storage| storage.get(x, (y & 15) as usize, z))
    }

    /// Sets the block at `(x, y, z)` on `layer` (0 or 1). Returns false if the
    /// chunk has no sub-chunk there. A missing layer 1 is added, filled with
    /// air.
    pub fn set_block(&mut self, x: usize, y: i32, z: usize, layer: usize, id: u32) -> bool {
        let Some(i) = self.section(y) else {
            return false;
        };
        let layers = &mut self.sub_chunks[i].layers;
        if layer >= 2 {
            return false;
        }
        while layers.len() <= layer {
            layers.push(Storage::Single(AIR));
        }
        layers[layer].set(x, (y & 15) as usize, z, id);
        true
    }
}
