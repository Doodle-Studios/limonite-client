//! The decoder against Pumpkin's real encoder, and `World::apply`.
//!
//! The round trip goes through `CLevelChunk::encode_chunk`, the code the
//! server runs, so these tests fail as soon as the encoder changes in a way
//! the decoder doesn't follow.

use std::collections::BTreeMap;

use bytes::Bytes;
use pumpkin_core::net::bedrock::level_chunk::CLevelChunk;
use pumpkin_data::{Block, BlockState, BlockStateId, biome::Biome};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::{
    RawPacket, bedrock::client::update_block::CUpdateBlock, packet::Packet, serial::PacketWrite,
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_world::chunk::{ChunkData, palette::bedrock_water_state};

use super::{
    AIR, Applied, ApplyError, BlockTable, World,
    level_chunk::{self, DecodeError},
};

/// Java states `first..first + count`, all valid.
fn states(first: u16, count: u16) -> impl Iterator<Item = BlockStateId> {
    (first..first + count).map(|raw| BlockStateId::new(raw).expect("a valid state id"))
}

fn a_waterlogged_state() -> BlockStateId {
    Block::OAK_STAIRS
        .states
        .iter()
        .map(|state| state.id)
        .find(|&id| bedrock_water_state(id) != Block::AIR.default_state.id)
        .expect("oak stairs can be waterlogged")
}

fn chest(x: i32, y: i32, z: i32) -> NbtCompound {
    let mut chest = NbtCompound::new();
    chest.put_string("id", "Chest".to_string());
    chest.put_int("x", x);
    chest.put_int("y", y);
    chest.put_int("z", z);
    chest.put_byte("isMovable", 1);
    chest
}

/// A chunk with every kind of section the plan lists. Sections are 16 blocks
/// tall from y -64, so section `s` starts at y `-64 + 16 s`.
fn test_chunk() -> ChunkData {
    let chunk = ChunkData::empty(-3, 5);
    let set = |x: usize, y: i32, z: usize, state: BlockStateId| {
        chunk.section.set_block_absolute_y(x, y, z, state);
    };
    // Section 0 stays all air.
    // Section 1: two states, a stone checkerboard on air (width 1).
    for x in 0..16 {
        for z in 0..16 {
            for y in -48i32..-32 {
                if (x + z + y.unsigned_abs() as usize).is_multiple_of(2) {
                    set(x, y, z, Block::STONE.default_state.id);
                }
            }
        }
    }
    // Section 2: 65 states (width 8 on the Bedrock side).
    for (i, state) in states(1, 64).enumerate() {
        set(i % 16, -32 + (i / 16) as i32, 3, state);
    }
    // Section 3: 300 states. Past 256, Pumpkin switches the section to its
    // `Dense` storage, and Bedrock gets width 16.
    for (i, state) in states(100, 300).enumerate() {
        set(i % 16, -16 + (i / 256) as i32, i / 16 % 16, state);
    }
    // Section 4: seagrass, water and a waterlogged block, so the section has a
    // water layer.
    set(1, 0, 1, Block::SEAGRASS.default_state.id);
    set(2, 0, 1, Block::WATER.default_state.id);
    set(3, 5, 9, a_waterlogged_state());
    // A chest, high up in section 10.
    set(7, 100, 8, Block::CHEST.default_state.id);
    // Mixed biomes: the plains default, a desert cell and a river cell in
    // section 2, and ocean throughout section 6. Biome cells are 4×4×4.
    chunk.section.set_relative_biome(0, 8, 0, Biome::DESERT.id);
    chunk.section.set_relative_biome(3, 9, 2, Biome::RIVER.id);
    for x in 0..4 {
        for y in 24..28 {
            for z in 0..4 {
                chunk.section.set_relative_biome(x, y, z, Biome::OCEAN.id);
            }
        }
    }
    chunk
}

fn encode(chunk: &ChunkData, cache: bool, actors: &[NbtCompound]) -> Vec<u8> {
    CLevelChunk::encode_chunk(chunk, 0, cache, actors)
        .expect("Pumpkin encodes the chunk")
        .0
}

#[test]
fn decodes_what_pumpkin_encodes_block_for_block() {
    let server = test_chunk();
    let actor = chest(-41, 100, 88);
    let ours = level_chunk::decode(&encode(&server, false, std::slice::from_ref(&actor)))
        .expect("the chunk decodes");

    assert_eq!((ours.x, ours.z, ours.dimension), (-3, 5, 0));
    assert_eq!((ours.min_y(), ours.max_y()), (-64, 320));
    let widths: Vec<_> = ours.sub_chunks[..5]
        .iter()
        .map(|sub| match &sub.layers[0] {
            super::storage::Storage::Single(_) => 0,
            super::storage::Storage::Packed { bits, .. } => *bits,
        })
        .collect();
    assert_eq!(widths, [0, 1, 8, 16, 2], "the widths each section was meant to cover");
    assert_eq!(ours.sub_chunks[4].layers.len(), 2, "section 4 has a water layer");

    let mut checked = 0;
    for x in 0..16 {
        for z in 0..16 {
            for y in -64..320 {
                let state = server.section.get_block_absolute_y(x, y, z).unwrap();
                assert_eq!(
                    ours.block(x, y, z, 0),
                    BlockState::to_be_network_id(state),
                    "layer 0 at ({x}, {y}, {z})"
                );
                assert_eq!(
                    ours.block(x, y, z, 1),
                    BlockState::to_be_network_id(bedrock_water_state(state)),
                    "layer 1 at ({x}, {y}, {z})"
                );
                let biome = server.section.get_rough_biome_absolute_y(x, y, z).unwrap();
                let biome = Biome::from_id(biome).map_or(1, |b| b.be_network_id);
                assert_eq!(
                    ours.biome(x, y, z),
                    Some(u32::from(biome)),
                    "biome at ({x}, {y}, {z})"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 16 * 16 * 384);

    // NBT compounds can't be compared as bytes (STATUS.md, "NBT order").
    assert_eq!(ours.block_actors, [actor]);
}

#[test]
fn cached_chunks_are_refused() {
    let bytes = encode(&test_chunk(), true, &[]);
    assert!(matches!(
        level_chunk::decode(&bytes),
        Err(DecodeError::CacheBlobs)
    ));
}

#[test]
fn every_prefix_is_an_error() {
    let chunk = ChunkData::empty(1, -1);
    chunk
        .section
        .set_block_absolute_y(2, 70, 3, Block::STONE.default_state.id);
    let bytes = encode(&chunk, false, &[chest(18, 70, -13)]);
    assert!(level_chunk::decode(&bytes).is_ok());
    for cut in 0..bytes.len() {
        assert!(level_chunk::decode(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    // And one byte too many.
    let mut longer = bytes;
    longer.push(0);
    assert_eq!(
        level_chunk::decode(&longer).unwrap_err(),
        DecodeError::TrailingData { len: 1 }
    );
}

fn raw(id: i32, payload: Vec<u8>) -> RawPacket {
    RawPacket {
        id,
        payload: Bytes::from(payload),
    }
}

fn update_block(x: i32, y: i32, z: i32, id: u32, layer: u32) -> RawPacket {
    let mut payload = Vec::new();
    CUpdateBlock::with_layer(BlockPos(Vector3::new(x, y, z)), id, layer)
        .write(&mut payload)
        .unwrap();
    raw(CUpdateBlock::PACKET_ID, payload)
}

#[test]
fn applies_chunks_and_block_updates() {
    let mut world = World::new();
    let chunk = ChunkData::empty(-3, 5);
    let applied = world
        .apply(&raw(CLevelChunk::PACKET_ID, encode(&chunk, false, &[])))
        .unwrap();
    assert_eq!(applied, Applied::Chunk { x: -3, z: 5 });

    // Chunk (-3, 5) covers x -48..-33 and z 80..95.
    let stone = BlockState::to_be_network_id(Block::STONE.default_state.id);
    let water = BlockState::to_be_network_id(Block::WATER.default_state.id);
    assert_eq!(world.apply(&update_block(-40, 64, 90, stone, 0)).unwrap(), Applied::Block);
    assert_eq!(world.apply(&update_block(-40, 64, 90, water, 1)).unwrap(), Applied::Block);
    assert_eq!(world.block(-40, 64, 90, 0), Some(stone));
    assert_eq!(world.block(-40, 64, 90, 1), Some(water));
    assert_eq!(world.block(-41, 64, 90, 0), Some(AIR));

    // Not loaded: counted and ignored.
    assert_eq!(
        world.apply(&update_block(0, 64, 0, stone, 0)).unwrap(),
        Applied::BlockNotLoaded
    );
    assert_eq!(world.updates_not_loaded(), 1);
    assert_eq!(world.block(0, 64, 0, 0), None);

    // A layer we don't model is an error, and changes nothing.
    assert!(matches!(
        world.apply(&update_block(-40, 64, 90, AIR, 2)),
        Err(ApplyError::Layer(2))
    ));
    // Other packets are ignored.
    assert_eq!(world.apply(&raw(1, Vec::new())).unwrap(), Applied::Ignored);
}

#[test]
fn block_table_is_deterministic_and_lowest_state_wins() {
    let first = BlockTable::build();
    let second = BlockTable::build();
    assert_eq!(first.java_by_bedrock, second.java_by_bedrock);
    assert_eq!(first.collisions, second.collisions);
    assert!(first.collisions > 0, "Java-only properties collapse");

    // Independently: every Java state per Bedrock id, then the lowest.
    let mut all: BTreeMap<u32, Vec<BlockStateId>> = BTreeMap::new();
    for state in states(0, BlockStateId::COUNT) {
        all.entry(BlockState::to_be_network_id(state))
            .or_default()
            .push(state);
    }
    assert_eq!(first.java_by_bedrock.len(), all.len());
    for (bedrock, java) in &all {
        assert_eq!(first.java_state(*bedrock), java.iter().min().copied());
    }
    let shared: usize = all.values().map(|java| java.len() - 1).sum();
    assert_eq!(first.collisions, shared);

    // A known case: oak leaves have 28 Java states (distance, persistent,
    // waterlogged), but Bedrock has no distance or waterlogged property.
    let leaves: Vec<_> = Block::OAK_LEAVES.states.iter().map(|s| s.id).collect();
    let bedrock: std::collections::BTreeSet<_> = leaves
        .iter()
        .map(|&id| BlockState::to_be_network_id(id))
        .collect();
    assert!(bedrock.len() < leaves.len());
    for id in bedrock {
        let lowest = leaves
            .iter()
            .copied()
            .filter(|&java| BlockState::to_be_network_id(java) == id)
            .min();
        assert_eq!(first.java_state(id), lowest);
    }
}
