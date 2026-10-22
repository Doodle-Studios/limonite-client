//! The mesher on hand-made appearances: no packs and no GPU.

use bytes::Bytes;
use limonite_packs::registry::Face;
use pumpkin_core::net::bedrock::level_chunk::CLevelChunk;
use pumpkin_data::{Block as JavaBlock, BlockState};
use pumpkin_protocol::{RawPacket, packet::Packet};
use pumpkin_world::chunk::ChunkData;

use super::*;
use crate::{
    blocks::appearance::{Appearance, AppearanceTable, FaceLook, Liquid, RenderClass, Shape},
    render::tint::{Tint, Tints},
    world::{AIR, World},
};

const STONE: u32 = 1;
const GLASS: u32 = 2;
const PLANT: u32 = 3;
const WATER: u32 = 4;
const SEAGRASS: u32 = 5;
const SLAB: u32 = 6;
const SNOW: u32 = 7;
const VINE: u32 = 8;
const SAND: u32 = 9;

fn look(shape: Shape, class: RenderClass, tint: Tint, layer: u16) -> Appearance {
    let face = FaceLook {
        layer,
        class,
        tinted: tint != Tint::None,
        turns: 0,
        isotropic: false,
    };
    Appearance {
        shape,
        tint,
        faces: [face; 6],
        occludes: shape == Shape::Cube && class == RenderClass::Opaque,
    }
}

fn table() -> AppearanceTable {
    let mut sand = look(Shape::Cube, RenderClass::Opaque, Tint::None, 9);
    sand.faces[Face::Up as usize].isotropic = true;
    let vine_faces = (1 << Face::North as u8) | (1 << Face::South as u8);
    AppearanceTable::from_looks([
        (AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)),
        (STONE, look(Shape::Cube, RenderClass::Opaque, Tint::None, 1)),
        (GLASS, look(Shape::Cube, RenderClass::Cutout, Tint::None, 2)),
        (PLANT, look(Shape::Cross, RenderClass::Cutout, Tint::Grass, 3)),
        (
            WATER,
            look(
                Shape::Liquid {
                    liquid: Liquid::Water,
                    surface: 14,
                },
                RenderClass::Blend,
                Tint::Water,
                4,
            ),
        ),
        (SEAGRASS, look(Shape::Cross, RenderClass::Cutout, Tint::None, 5)),
        (SLAB, look(Shape::Slab { bottom: 0, top: 8 }, RenderClass::Opaque, Tint::None, 6)),
        (SNOW, look(Shape::Slab { bottom: 0, top: 2 }, RenderClass::Opaque, Tint::None, 7)),
        (VINE, look(Shape::Planes { faces: vine_faces }, RenderClass::Cutout, Tint::Foliage, 8)),
        (SAND, sand),
    ])
}

fn tints() -> Tints {
    Tints::uniform([10, 200, 10], [40, 100, 250, 166])
}

/// Meshes a sub-chunk at the origin holding `blocks` on layer 0 and water
/// on layer 1 at `waterlogged`.
fn mesh_of(blocks: &[(i32, i32, i32, u32)], waterlogged: &[(i32, i32, i32)]) -> Mesh {
    let table = table();
    let n = Neighbourhood::from_fn(&table, [0, 0, 0], 1, |x, y, z, layer| {
        if layer == 0 {
            blocks
                .iter()
                .find(|&&(bx, by, bz, _)| (bx, by, bz) == (x, y, z))
                .map_or(AIR, |&(.., id)| id)
        } else if waterlogged.contains(&(x, y, z)) {
            WATER
        } else {
            AIR
        }
    });
    mesh(&n, &tints())
}

/// The quads of a mesh's vertex list.
fn quads(vertices: &[Vertex]) -> Vec<&[Vertex]> {
    vertices.chunks(4).collect()
}

/// Whether some quad lies in the plane `axis` = `at` sixteenths.
fn has_quad_at(vertices: &[Vertex], axis: usize, at: u16) -> bool {
    quads(vertices)
        .iter()
        .any(|quad| quad.iter().all(|vertex| vertex.position[axis] == at))
}

#[test]
fn one_stone_has_six_faces_and_two_touching_have_ten() {
    assert!(mesh_of(&[], &[]).is_empty());
    let one = mesh_of(&[(5, 5, 5, STONE)], &[]);
    assert_eq!(one.quads(), 6);
    assert!(!one.is_empty());
    assert!(one.blend.is_empty());
    let two = mesh_of(&[(5, 5, 5, STONE), (6, 5, 5, STONE)], &[]);
    assert_eq!(two.quads(), 10);
    assert!(!has_quad_at(&two.opaque, 0, 6 * 16), "the shared face is hidden");
}

#[test]
fn quads_face_outwards() {
    let mesh = mesh_of(&[(5, 5, 5, STONE)], &[]);
    for quad in quads(&mesh.opaque) {
        let p = |i: usize| quad[i].position.map(i32::from);
        let (a, b, c) = (p(0), p(1), p(2));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let outward = match quad[0].face() {
            0 => [0, -1, 0],
            1 => [0, 1, 0],
            2 => [0, 0, -1],
            3 => [0, 0, 1],
            4 => [-1, 0, 0],
            _ => [1, 0, 0],
        };
        let dot: i32 = normal.iter().zip(outward).map(|(n, o)| n * o).sum();
        assert!(dot > 0, "face {} winds inwards: {normal:?}", quad[0].face());
    }
}

#[test]
fn stone_beside_glass_keeps_the_face_between() {
    let mesh = mesh_of(&[(5, 5, 5, STONE), (6, 5, 5, GLASS)], &[]);
    // Glass doesn't hide the stone's east face; stone hides the glass's west.
    assert_eq!(mesh.quads(), 6 + 5);
    let stone_east = quads(&mesh.opaque).into_iter().filter(|quad| {
        quad[0].face() == Face::East as u8 && quad.iter().all(|v| v.position[0] == 6 * 16)
    });
    assert_eq!(stone_east.count(), 1);
}

#[test]
fn a_cross_plant_is_four_quads_both_sides_of_two_planes() {
    let mesh = mesh_of(&[(5, 5, 5, PLANT)], &[]);
    assert_eq!(mesh.quads(), 4);
    for vertex in &mesh.opaque {
        assert_eq!(vertex.face(), DIAGONAL);
        assert_eq!(vertex.class(), 1, "cutout");
        assert_eq!(vertex.tint, [10, 200, 10, 255], "grass-tinted");
    }
}

#[test]
fn water_beside_water_or_waterlogged_seagrass_has_no_face_between() {
    // Two water blocks: 6 faces each, less the two between them.
    let mesh = mesh_of(&[(5, 5, 5, WATER), (6, 5, 5, WATER)], &[]);
    assert_eq!(mesh.blend.len() / 4, 10);
    assert!(!has_quad_at(&mesh.blend, 0, 6 * 16));
    assert!(mesh.blend.iter().all(|vertex| vertex.tint == [40, 100, 250, 166]));

    // Seagrass with water around it, on layer 1: the water faces between the
    // two water volumes go too, and the seagrass is drawn.
    let mesh = mesh_of(&[(5, 5, 5, WATER), (6, 5, 5, SEAGRASS)], &[(6, 5, 5)]);
    assert!(!has_quad_at(&mesh.blend, 0, 6 * 16));
    assert_eq!(mesh.blend.len() / 4, 10);
    assert_eq!(mesh.opaque.len() / 4, 4, "the seagrass cross");
}

#[test]
fn water_under_air_gets_a_lowered_top() {
    let alone = mesh_of(&[(5, 5, 5, WATER)], &[]);
    assert!(has_quad_at(&alone.blend, 1, 5 * 16 + 14), "the surface is 14/16 up");
    assert!(!alone.blend.iter().any(|vertex| vertex.position[1] == 6 * 16));

    // With water above, the lower block has no top, and full-height sides.
    let stacked = mesh_of(&[(5, 5, 5, WATER), (5, 6, 5, WATER)], &[]);
    let lower_sides_reach_the_top = quads(&stacked.blend)
        .into_iter()
        .filter(|quad| quad[0].face() == Face::North as u8 && quad.iter().any(|v| v.position[1] == 5 * 16))
        .any(|quad| quad.iter().any(|v| v.position[1] == 6 * 16));
    assert!(lower_sides_reach_the_top);
    assert!(!has_quad_at(&stacked.blend, 1, 5 * 16 + 14));
}

#[test]
fn slabs_snow_and_vines() {
    // A bottom slab under stone: its top is half way up, and isn't hidden.
    let slab = mesh_of(&[(5, 5, 5, SLAB), (5, 6, 5, STONE)], &[]);
    assert!(has_quad_at(&slab.opaque, 1, 5 * 16 + 8));
    // The slab's sides show the bottom half of the texture.
    let side = quads(&slab.opaque)
        .into_iter()
        .find(|quad| quad[0].face() == Face::South as u8 && quad[0].layer == 6)
        .unwrap();
    assert!(side.iter().all(|vertex| vertex.uv[1] >= 8));

    let snow = mesh_of(&[(5, 5, 5, SNOW)], &[]);
    assert_eq!(snow.quads(), 6);
    assert!(has_quad_at(&snow.opaque, 1, 5 * 16 + 2));

    // A vine on the north and south: two planes, each from both sides, a
    // sixteenth in from the faces.
    let vine = mesh_of(&[(5, 5, 5, VINE)], &[]);
    assert_eq!(vine.quads(), 4);
    assert!(has_quad_at(&vine.opaque, 2, 5 * 16 + 1));
    assert!(has_quad_at(&vine.opaque, 2, 5 * 16 + 15));
}

#[test]
fn the_isotropic_rotation_is_the_same_every_run_and_varies_by_place() {
    let field: Vec<_> = (0..16).flat_map(|x| (0..16).map(move |z| (x, 0, z, SAND))).collect();
    let first = mesh_of(&field, &[]);
    let second = mesh_of(&field, &[]);
    assert_eq!(first.opaque, second.opaque);
    // The tops: one quad per block, and more than one orientation among them.
    let orientations: std::collections::HashSet<[u8; 2]> = quads(&first.opaque)
        .into_iter()
        .filter(|quad| quad[0].face() == Face::Up as u8)
        .map(|quad| quad[0].uv)
        .collect();
    assert!(orientations.len() > 1, "{orientations:?}");
    assert_eq!(isotropic_turns(10, 64, -3, 1), isotropic_turns(10, 64, -3, 1));
}

/// Stone positions in a chunk: x and z within it, and y.
type Stones<'a> = &'a [(usize, i32, usize)];

/// A world with the given chunks, each with stone at the listed positions.
fn world_with(chunks: &[((i32, i32), Stones<'_>)]) -> World {
    let mut world = World::new();
    for &((cx, cz), stones) in chunks {
        let chunk = ChunkData::empty(cx, cz);
        for &(x, y, z) in stones {
            chunk.section.set_block_absolute_y(x, y, z, JavaBlock::STONE.default_state.id);
        }
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

#[test]
fn borders_read_the_neighbour_and_a_missing_one_stops_meshing() {
    let stone = BlockState::to_be_network_id(JavaBlock::STONE.default_state.id);
    let table = AppearanceTable::from_looks([
        (AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)),
        (stone, look(Shape::Cube, RenderClass::Opaque, Tint::None, 1)),
    ]);
    // Stone at the east edge of chunk (0, 0), and beside it in chunk (1, 0).
    let world = world_with(&[
        ((0, 0), &[(15, 70, 5)]),
        ((1, 0), &[(0, 70, 5)]),
        ((0, -1), &[]),
        ((0, 1), &[]),
        ((-1, 0), &[]),
    ]);
    // y 70 is in section 70 / 16 = 4, the 9th from the bottom (-4).
    let n = Neighbourhood::gather(&world, &table, 0, 8, 0).unwrap();
    assert_eq!(n.origin, [0, 64, 0]);
    let mesh = mesh(&n, &tints());
    assert_eq!(mesh.quads(), 5, "the east face is hidden by the neighbour's stone");

    let incomplete = world_with(&[((0, 0), &[]), ((1, 0), &[]), ((0, -1), &[]), ((-1, 0), &[])]);
    assert!(Neighbourhood::gather(&incomplete, &table, 0, 8, 0).is_none());
}
