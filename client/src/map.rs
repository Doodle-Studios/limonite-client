//! A top-down map of the loaded chunks, one pixel per block column, drawn
//! from the client's world state alone.
//!
//! It looks like an in-game Java map, using the Java map colours Pumpkin
//! already compiles in (`Block::map_color`): north (-Z) is up and east (+X)
//! is right. The colours are Java's, not Bedrock's, which is close enough to
//! compare coastlines, rivers, trees and biome edges with the official
//! client.

use std::{collections::BTreeSet, path::Path};

use image::{ImageFormat, Rgba, RgbaImage};
use pumpkin_data::{Block, map_color::MapColor};
use pumpkin_util::math::vector3::Vector3;

use crate::world::{AIR, World, chunk::Chunk, storage::Storage};

/// Java's map brightness steps, out of 255: lower than the column to the
/// north, level with it, higher than it.
const DARK: u16 = 180;
const LEVEL: u16 = 220;
const BRIGHT: u16 = 255;

const UNLOADED: Rgba<u8> = Rgba([0, 0, 0, 0]);
const UNKNOWN: Rgba<u8> = Rgba([255, 0, 255, 255]);
const MARKER: Rgba<u8> = Rgba([255, 0, 0, 255]);
/// Lava is Java's "fire" colour, pure red too, so the marker gets an outline.
const MARKER_OUTLINE: Rgba<u8> = Rgba([255, 255, 255, 255]);

/// What a block column looks like from above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    /// A block with a map colour at height `y`.
    Land { y: i32, color: u8 },
    /// Water at the surface, `depth` blocks deep.
    Water { y: i32, depth: u32 },
    /// The top block's id isn't in the block table.
    Unknown,
    /// Nothing with a colour, all the way down.
    Empty,
}

pub struct Map {
    pub image: RgbaImage,
    pub chunks: usize,
    /// Distinct block ids the block table doesn't know.
    pub unknown_ids: BTreeSet<u32>,
}

/// Draws every loaded chunk, with the host marked at `host` if given.
/// Returns `None` if no chunk is loaded.
#[must_use]
pub fn render(world: &World, host: Option<Vector3<f32>>) -> Option<Map> {
    let (min_x, max_x) = min_max(world.chunks().map(|chunk| chunk.x))?;
    let (min_z, max_z) = min_max(world.chunks().map(|chunk| chunk.z))?;
    let width = ((max_x - min_x + 1) * 16) as u32;
    let height = ((max_z - min_z + 1) * 16) as u32;

    // First every column's top block, then the shading, which needs the
    // column to the north.
    let mut columns = vec![None; (width * height) as usize];
    let mut unknown_ids = BTreeSet::new();
    for chunk in world.chunks() {
        let px0 = ((chunk.x - min_x) * 16) as u32;
        let py0 = ((chunk.z - min_z) * 16) as u32;
        for z in 0..16 {
            for x in 0..16 {
                let column = top_of(world, chunk, x, z, &mut unknown_ids);
                columns[((py0 + z as u32) * width + px0 + x as u32) as usize] = Some(column);
            }
        }
    }

    let mut image = RgbaImage::from_pixel(width, height, UNLOADED);
    for py in 0..height {
        for px in 0..width {
            let Some(column) = columns[(py * width + px) as usize] else {
                continue;
            };
            let north = py
                .checked_sub(1)
                .and_then(|north| columns[(north * width + px) as usize]);
            image.put_pixel(px, py, shade(column, north));
        }
    }

    if let Some(host) = host {
        let px = host.x.floor() as i64 - i64::from(min_x) * 16;
        let py = host.z.floor() as i64 - i64::from(min_z) * 16;
        mark(&mut image, px, py);
    }

    Some(Map {
        image,
        chunks: world.chunk_count(),
        unknown_ids,
    })
}

/// Renders the map and writes it to `path` as a PNG.
///
/// # Errors
/// If no chunk is loaded, or the file can't be written.
pub fn write(world: &World, host: Option<Vector3<f32>>, path: &Path) -> Result<Map, String> {
    let map = render(world, host).ok_or("no chunks are loaded")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("can't create {}: {err}", dir.display()))?;
    }
    map.image
        .save_with_format(path, ImageFormat::Png)
        .map_err(|err| format!("can't write {}: {err}", path.display()))?;
    Ok(map)
}

fn min_max(values: impl Iterator<Item = i32>) -> Option<(i32, i32)> {
    values.fold(None, |range, v| match range {
        None => Some((v, v)),
        Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
    })
}

/// Scans one column from the top of the highest sub-chunk down.
fn top_of(world: &World, chunk: &Chunk, x: usize, z: usize, unknown: &mut BTreeSet<u32>) -> Column {
    let mut y = chunk.max_y() - 1;
    while y >= chunk.min_y() {
        // Skip a sub-chunk that is air on every layer in one step.
        let section = &chunk.sub_chunks[((y >> 4) - chunk.min_section) as usize];
        if section
            .layers
            .iter()
            .all(|layer| *layer == Storage::Single(AIR))
        {
            y = (y & !15) - 1;
            continue;
        }
        // Water in layer 1 is water around the block, and it's what you see.
        if is_water(world, chunk.block(x, y, z, 1)) {
            return Column::Water {
                y,
                depth: water_depth(world, chunk, x, y, z),
            };
        }
        let id = chunk.block(x, y, z, 0);
        if id != AIR {
            let Some(state) = world.blocks().java_state(id) else {
                unknown.insert(id);
                return Column::Unknown;
            };
            let color = Block::from_state_id(state).map_color;
            if color == MapColor::WATER.id {
                return Column::Water {
                    y,
                    depth: water_depth(world, chunk, x, y, z),
                };
            }
            if color != MapColor::NONE_0.id {
                return Column::Land { y, color };
            }
        }
        y -= 1;
    }
    Column::Empty
}

fn is_water(world: &World, id: u32) -> bool {
    id != AIR
        && world
            .blocks()
            .java_state(id)
            .is_some_and(|state| Block::from_state_id(state).map_color == MapColor::WATER.id)
}

/// How many blocks of water, on either layer, from `y` down.
fn water_depth(world: &World, chunk: &Chunk, x: usize, y: i32, z: usize) -> u32 {
    let mut depth = 0;
    let mut y = y;
    while y >= chunk.min_y()
        && (is_water(world, chunk.block(x, y, z, 0)) || is_water(world, chunk.block(x, y, z, 1)))
    {
        depth += 1;
        y -= 1;
    }
    depth
}

/// The pixel for `column`, given the column to its north.
///
/// Land is brighter where it rises from north to south and darker where it
/// falls, as on Java maps, so relief and coastlines show. Water is darker
/// the deeper it is (Java's thresholds, without its checkerboard dither).
fn shade(column: Column, north: Option<Column>) -> Rgba<u8> {
    let (color, brightness) = match column {
        Column::Land { y, color } => {
            let north_y = match north {
                Some(Column::Land { y, .. } | Column::Water { y, .. }) => y,
                _ => y,
            };
            let brightness = match y.cmp(&north_y) {
                std::cmp::Ordering::Greater => BRIGHT,
                std::cmp::Ordering::Equal => LEVEL,
                std::cmp::Ordering::Less => DARK,
            };
            (color, brightness)
        }
        Column::Water { depth, .. } => {
            let brightness = match depth {
                0..5 => BRIGHT,
                5..10 => LEVEL,
                _ => DARK,
            };
            (MapColor::WATER.id, brightness)
        }
        Column::Unknown => return UNKNOWN,
        Column::Empty => return UNLOADED,
    };
    let (r, g, b) = MapColor::from_id(color).map_or((0, 0, 0), |color| color.rgb);
    let scale = |c: u8| (u16::from(c) * brightness / 255) as u8;
    Rgba([scale(r), scale(g), scale(b), 255])
}

/// A red cross with a white outline, centred on `(px, py)`.
fn mark(image: &mut RgbaImage, px: i64, py: i64) {
    let mut put = |dx: i64, dy: i64, color| {
        let (x, y) = (px + dx, py + dy);
        if (0..i64::from(image.width())).contains(&x) && (0..i64::from(image.height())).contains(&y) {
            image.put_pixel(x as u32, y as u32, color);
        }
    };
    for d in -4i64..=4 {
        for side in [-1, 1] {
            put(d, side, MARKER_OUTLINE);
            put(side, d, MARKER_OUTLINE);
        }
    }
    for (dx, dy) in [(-4i64, 0i64), (4, 0), (0, -4), (0, 4)] {
        put(dx + dx.signum(), dy + dy.signum(), MARKER_OUTLINE);
    }
    for d in -4i64..=4 {
        put(d, 0, MARKER);
        put(0, d, MARKER);
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use pumpkin_core::net::bedrock::level_chunk::CLevelChunk;
    use pumpkin_data::BlockState;
    use pumpkin_protocol::{RawPacket, packet::Packet};
    use pumpkin_world::chunk::ChunkData;

    use super::*;

    /// A world holding the given chunks, each built by `fill` and delivered
    /// through the real encoder and `World::apply`.
    fn world_with(chunks: &[(i32, i32)], fill: impl Fn(&ChunkData)) -> World {
        let mut world = World::new();
        for &(x, z) in chunks {
            let chunk = ChunkData::empty(x, z);
            fill(&chunk);
            let (payload, _) = CLevelChunk::encode_chunk(&chunk, 0, false, &[]).unwrap();
            world
                .apply(&RawPacket {
                    id: CLevelChunk::PACKET_ID,
                    payload: Bytes::from(payload),
                })
                .unwrap();
        }
        world
    }

    fn rgb(color: MapColor, brightness: u16) -> Rgba<u8> {
        let (r, g, b) = color.rgb;
        let scale = |c: u8| (u16::from(c) * brightness / 255) as u8;
        Rgba([scale(r), scale(g), scale(b), 255])
    }

    #[test]
    fn north_is_up_and_east_is_right() {
        // A stone floor at y 0 with a single gold block at the chunk's east
        // edge (x 15) and one at its north edge (z 0).
        let world = world_with(&[(0, 0)], |chunk| {
            for x in 0..16 {
                for z in 0..16 {
                    chunk.section.set_block_absolute_y(x, 0, z, Block::STONE.default_state.id);
                }
            }
            chunk.section.set_block_absolute_y(15, 0, 8, Block::GOLD_BLOCK.default_state.id);
            chunk.section.set_block_absolute_y(8, 0, 0, Block::GOLD_BLOCK.default_state.id);
        });
        let map = render(&world, None).unwrap();
        assert_eq!(map.image.dimensions(), (16, 16));
        let gold = MapColor::from_id(Block::GOLD_BLOCK.map_color).unwrap();
        // East (+X) is the right edge; north (-Z) is the top row.
        assert_eq!(*map.image.get_pixel(15, 8), rgb(*gold, LEVEL));
        assert_eq!(*map.image.get_pixel(8, 0), rgb(*gold, LEVEL));
        assert_ne!(*map.image.get_pixel(0, 8), rgb(*gold, LEVEL));
        assert!(map.unknown_ids.is_empty());
    }

    #[test]
    fn water_in_layer_1_wins_over_the_block() {
        // Waterlogged stairs have the colour of wood, but the water around
        // them is what's seen from above.
        let stairs = Block::OAK_STAIRS
            .states
            .iter()
            .map(|state| state.id)
            .find(|&id| BlockState::from_id(id).is_waterlogged())
            .unwrap();
        let world = world_with(&[(0, 0)], |chunk| {
            chunk.section.set_block_absolute_y(0, 10, 0, stairs);
            chunk.section.set_block_absolute_y(1, 10, 0, Block::OAK_PLANKS.default_state.id);
        });
        let map = render(&world, None).unwrap();
        // One block of water: shallow, so bright.
        assert_eq!(*map.image.get_pixel(0, 0), rgb(MapColor::WATER, BRIGHT));
        let planks = MapColor::from_id(Block::OAK_PLANKS.map_color).unwrap();
        assert_eq!(*map.image.get_pixel(1, 0), rgb(*planks, LEVEL));
    }

    #[test]
    fn unloaded_chunks_are_transparent() {
        // Chunks (0, 0) and (1, 1): (1, 0) and (0, 1) are missing.
        let world = world_with(&[(0, 0), (1, 1)], |chunk| {
            chunk.section.set_block_absolute_y(0, 0, 0, Block::STONE.default_state.id);
        });
        let map = render(&world, None).unwrap();
        assert_eq!(map.image.dimensions(), (32, 32));
        assert_eq!(*map.image.get_pixel(20, 5), UNLOADED);
        assert_eq!(*map.image.get_pixel(5, 20), UNLOADED);
        assert_eq!(map.image.get_pixel(0, 0)[3], 255);
        assert_eq!(map.image.get_pixel(16, 16)[3], 255);
    }

    #[test]
    fn slopes_shade_in_three_levels() {
        // Along x = 0, from north to south: y 5, 6 (rises), 6 (level), 4 (falls).
        let heights = [5, 6, 6, 4];
        let world = world_with(&[(0, 0)], |chunk| {
            for (z, &y) in heights.iter().enumerate() {
                chunk.section.set_block_absolute_y(0, y, z, Block::STONE.default_state.id);
            }
        });
        let map = render(&world, None).unwrap();
        let stone = MapColor::from_id(Block::STONE.map_color).unwrap();
        assert_eq!(*map.image.get_pixel(0, 1), rgb(*stone, BRIGHT));
        assert_eq!(*map.image.get_pixel(0, 2), rgb(*stone, LEVEL));
        assert_eq!(*map.image.get_pixel(0, 3), rgb(*stone, DARK));
        // The top row has no northern neighbour, so it's level.
        assert_eq!(*map.image.get_pixel(0, 0), rgb(*stone, LEVEL));
    }

    #[test]
    fn the_host_is_marked() {
        let world = world_with(&[(-1, -1)], |_| {});
        let map = render(&world, Some(Vector3::new(-8.5, 70.0, -7.2))).unwrap();
        // Chunk (-1, -1) starts at x -16, z -16: block (-9, -8) is pixel (7, 8).
        assert_eq!(*map.image.get_pixel(7, 8), MARKER);
        assert_eq!(*map.image.get_pixel(7, 9), MARKER);
        assert_eq!(*map.image.get_pixel(8, 9), MARKER_OUTLINE);
    }
}
