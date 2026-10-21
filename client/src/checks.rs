//! The acceptance checks, each reported as one `PASS` or `FAIL` line.
//!
//! M2's "no socket is opened" can't be made from inside the process; it's
//! done from outside while the session lingers.

use pumpkin_core::server::Server;
use pumpkin_data::{Block, BlockState, BlockStateId, biome::Biome};
use pumpkin_world::chunk::{ChunkData, palette::bedrock_water_state};
use tracing::{error, info};

use crate::{
    session::Spawn,
    world::{BlockTable, chunk::Chunk},
};

/// How far east the host must have moved, as the server sees it.
pub const MIN_MOVE: f64 = 4.0;

pub struct Check {
    name: &'static str,
    passed: bool,
    detail: String,
}

impl Check {
    pub const fn new(name: &'static str, passed: bool, detail: String) -> Self {
        Self {
            name,
            passed,
            detail,
        }
    }

    pub const fn passed(&self) -> bool {
        self.passed
    }
}

/// Whether the server's copy of the host has moved at least [`MIN_MOVE`]
/// blocks east of where `StartGame` put it.
///
/// This reads the server's own state, not an echo: the position went from
/// our `PlayerAuthInput` through the `LocalChannel`, the server's read loop,
/// its packet queue and its tick thread, which applied it.
pub fn movement(server: &Server, spawn: &Spawn) -> Check {
    let players = server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new("movement", false, "server has no player".to_string());
    };
    let entity = &player.living_entity.entity;
    // `StartGame` gives the eye position; the server stores the feet.
    let eye_height = f64::from(entity.entity_type.eye_height);
    let start_x = f64::from(spawn.eye_position.x);
    let start_feet_y = f64::from(spawn.eye_position.y) - eye_height;
    let now = player.position();
    let moved = now.x - start_x;
    Check::new(
        "movement",
        moved >= MIN_MOVE,
        format!(
            "the server's player moved {moved:.2} blocks east (needs {MIN_MOVE}); \
             feet y {start_feet_y:.2} -> {:.2}",
            now.y
        ),
    )
}

/// Whether every `LevelChunk` decoded, and `StartGame` said block ids are
/// hashed, which the world's id table assumes.
pub fn chunks_decoded(received: u32, decoded: u32, errors: u32, hashed: Option<bool>) -> Check {
    let hashed_detail = match hashed {
        Some(true) => "block ids are hashed",
        Some(false) => "block ids are NOT hashed",
        None => "no StartGame",
    };
    Check::new(
        "chunks decoded",
        received > 0 && decoded == received && errors == 0 && hashed == Some(true),
        format!("{decoded} of {received} LevelChunk packets decoded, {errors} errors; {hashed_detail}"),
    )
}

/// Compares our copy of a chunk with the server's, at every position: the
/// block on layer 0, its water on layer 1, and the biome.
///
/// Each side reads through its own code. Ours goes through the decoder's
/// palette index maths (XZY words); the server's through Pumpkin's
/// `get_block_absolute_y` on its own sections (YZX). A mistake shared by
/// both sides could hide; independent ones can't.
///
/// Returns how many values differ, and describes up to `limit` of them in
/// `out`.
pub fn compare_chunk(
    server: &ChunkData,
    ours: &Chunk,
    names: &BlockTable,
    out: &mut Vec<String>,
    limit: usize,
) -> usize {
    let sections = &server.section;
    let server_max_y = sections.min_y + 16 * sections.count as i32;
    if (ours.min_y(), ours.max_y()) != (sections.min_y, server_max_y) {
        out.push(format!(
            "chunk ({}, {}): our height {}..{}, the server's {}..{server_max_y}",
            ours.x,
            ours.z,
            ours.min_y(),
            ours.max_y(),
            sections.min_y,
        ));
        return 1;
    }

    let mut differ = 0;
    let mut note = |out: &mut Vec<String>, what: std::fmt::Arguments<'_>| {
        differ += 1;
        if out.len() < limit {
            out.push(what.to_string());
        }
    };
    let (cx, cz) = (ours.x * 16, ours.z * 16);
    for x in 0..16 {
        for z in 0..16 {
            for y in sections.min_y..server_max_y {
                let (wx, wz) = (cx + x as i32, cz + z as i32);
                let Some(state) = sections.get_block_absolute_y(x, y, z) else {
                    continue;
                };
                let ours_block = ours.block(x, y, z, 0);
                // Ids as well as names: several states share a name
                // (lit and unlit redstone ore, say).
                let server_block = BlockState::to_be_network_id(state);
                if ours_block != server_block {
                    note(
                        out,
                        format_args!(
                            "({wx}, {y}, {wz}) layer 0: ours {} {ours_block:#010x}, \
                             server {} {server_block:#010x} (Java state {})",
                            names.name(ours_block),
                            block_name(state),
                            state.as_u16(),
                        ),
                    );
                }
                let water = bedrock_water_state(state);
                let server_water = BlockState::to_be_network_id(water);
                let ours_water = ours.block(x, y, z, 1);
                if ours_water != server_water {
                    note(
                        out,
                        format_args!(
                            "({wx}, {y}, {wz}) layer 1: ours {} {ours_water:#010x}, \
                             server {} {server_water:#010x} (from {})",
                            names.name(ours_water),
                            block_name(water),
                            block_name(state),
                        ),
                    );
                }
                let server_biome = sections
                    .get_rough_biome_absolute_y(x, y, z)
                    .map(|id| Biome::from_id(id).map_or(1, |biome| biome.be_network_id));
                let ours_biome = ours.biome(x, y, z);
                if ours_biome != server_biome.map(u32::from) {
                    note(
                        out,
                        format_args!(
                            "({wx}, {y}, {wz}) biome: ours {ours_biome:?}, server {server_biome:?}"
                        ),
                    );
                }
            }
        }
    }
    differ
}

fn block_name(state: BlockStateId) -> &'static str {
    Block::from_state_id(state).name
}

/// Logs one line per check and returns whether they all passed.
pub fn report(checks: &[Check]) -> bool {
    for check in checks {
        if check.passed {
            info!("PASS {}: {}", check.name, check.detail);
        } else {
            error!("FAIL {}: {}", check.name, check.detail);
        }
    }
    checks.iter().all(|check| check.passed)
}
