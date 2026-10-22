//! The appearance table on synthetic packs, and its name tables against the
//! real palette.

use std::{collections::HashMap, sync::Arc};

use image::{Rgba, RgbaImage};
use limonite_packs::{
    Pack, PackStack,
    registry::{BlocksJson, Face, TerrainTextures},
};
use pumpkin_data::{Block, BlockState as JavaState, BlockStateId};

use super::*;
use crate::{
    blocks::{
        palette::{BedrockState, Palette, StateValue},
        palette_source,
    },
    test_util::{TempDir, png, tga},
};

fn state(name: &str, states: &[(&str, StateValue)]) -> BedrockState {
    let mut states: Vec<(Arc<str>, StateValue)> = states
        .iter()
        .map(|(key, value)| (Arc::from(*key), value.clone()))
        .collect();
    states.sort_by(|a, b| a.0.cmp(&b.0));
    BedrockState {
        name: Arc::from(name),
        states: states.into_boxed_slice(),
    }
}

fn text(value: &str) -> StateValue {
    StateValue::String(Arc::from(value))
}

fn solid(color: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(16, 16, Rgba(color))
}

/// A synthetic vanilla-like pack, and a palette of states that use it.
struct Fixture {
    _dir: TempDir,
    built: Built,
}

const STONE: u32 = 1;
const GRASS: u32 = 2;
const LEAVES: u32 = 3;
const GLASS: u32 = 4;
const LOG_X: u32 = 5;
const LOG_Z: u32 = 6;
const TALL_LOWER: u32 = 7;
const TALL_UPPER: u32 = 8;
const AIR: u32 = 9;
const EDUCATION: u32 = 10;
const BROKEN: u32 = 11;
const SAND: u32 = 12;

fn fixture() -> Fixture {
    let dir = TempDir::new("appearance");
    dir.write(
        "blocks.json",
        br#"{
            "format_version": [1, 1, 0],
            "stone": { "textures": "stone" },
            "grass": {
                "isotropic": { "up": true, "down": true },
                "textures": { "up": "grass_top", "down": "dirt", "side": "grass_side" }
            },
            "oak_leaves": { "isotropic": { "up": true }, "textures": "oak_leaves" },
            "white_stained_glass": { "textures": "glass_white" },
            "oak_log": { "textures": { "up": "log_top", "down": "log_top", "side": "log_side" } },
            "tall_grass": { "textures": { "up": "tall_top", "down": "tall_bottom", "side": "unused" } },
            "air": {},
            "broken": { "textures": "nowhere" },
            "sand": { "isotropic": true, "textures": "sand" }
        }"#,
    );
    dir.write(
        "textures/terrain_texture.json",
        br#"{ "texture_data": {
            "stone": { "textures": "textures/blocks/stone" },
            "grass_top": { "textures": ["textures/blocks/grass_top", "textures/blocks/grass_top"] },
            "dirt": { "textures": "textures/blocks/dirt" },
            "grass_side": { "textures": [{ "path": "textures/blocks/grass_side" }, "textures/blocks/snowy"] },
            "oak_leaves": { "textures": ["textures/blocks/leaves_oak", "textures/blocks/leaves_oak_opaque"] },
            "glass_white": { "textures": "textures/blocks/glass_white" },
            "log_top": { "textures": "textures/blocks/log_top" },
            "log_side": { "textures": "textures/blocks/log_side" },
            "tall_top": { "textures": "textures/blocks/tall_top" },
            "tall_bottom": { "textures": "textures/blocks/tall_bottom" },
            "sand": { "textures": "textures/blocks/sand" }
        } }"#,
    );
    dir.write("textures/misc/missing_texture.png", &png(&solid([255, 0, 255, 255])));
    dir.write("textures/blocks/stone.png", &png(&solid([1, 1, 1, 255])));
    dir.write("textures/blocks/grass_top.png", &png(&solid([2, 2, 2, 255])));
    dir.write("textures/blocks/dirt.png", &png(&solid([3, 3, 3, 255])));
    // The grass side: alpha 0 dirt, with one masked texel, as in vanilla.
    let mut side = solid([4, 4, 4, 0]);
    side.put_pixel(5, 5, Rgba([200, 200, 200, 255]));
    dir.write("textures/blocks/grass_side.tga", &tga(&side));
    // Fancy leaves have holes; the fast-graphics copy, variant 1, doesn't.
    let mut leaves = solid([5, 5, 5, 255]);
    leaves.put_pixel(3, 3, Rgba([0, 0, 0, 0]));
    dir.write("textures/blocks/leaves_oak.tga", &tga(&leaves));
    dir.write("textures/blocks/leaves_oak_opaque.png", &png(&solid([6, 6, 6, 255])));
    dir.write("textures/blocks/glass_white.png", &png(&solid([7, 7, 7, 100])));
    dir.write("textures/blocks/log_top.png", &png(&solid([8, 8, 8, 255])));
    dir.write("textures/blocks/log_side.png", &png(&solid([9, 9, 9, 255])));
    dir.write("textures/blocks/tall_top.png", &png(&solid([10, 10, 10, 0])));
    dir.write("textures/blocks/tall_bottom.png", &png(&solid([11, 11, 11, 0])));
    dir.write("textures/blocks/sand.png", &png(&solid([12, 12, 12, 255])));

    let palette = Palette::new(HashMap::from([
        (STONE, state("stone", &[])),
        (GRASS, state("grass_block", &[])),
        (LEAVES, state("oak_leaves", &[("persistent_bit", StateValue::Bool(false))])),
        (GLASS, state("white_stained_glass", &[])),
        (LOG_X, state("oak_log", &[("pillar_axis", text("x"))])),
        (LOG_Z, state("oak_log", &[("pillar_axis", text("z"))])),
        (TALL_LOWER, state("tall_grass", &[("upper_block_bit", StateValue::Bool(false))])),
        (TALL_UPPER, state("tall_grass", &[("upper_block_bit", StateValue::Bool(true))])),
        (AIR, state("air", &[])),
        (EDUCATION, state("element_1", &[])),
        (BROKEN, state("broken", &[])),
        (SAND, state("sand", &[])),
    ]));
    let stack = PackStack::new(vec![Pack::new(dir.path().to_path_buf())]);
    let (blocks, problems) = BlocksJson::load(&stack);
    assert!(problems.is_empty(), "{problems:?}");
    let (terrain, problems) = TerrainTextures::load(&stack);
    assert!(problems.is_empty(), "{problems:?}");
    let built = build(&palette, &blocks, &terrain, &stack);
    Fixture { _dir: dir, built }
}

impl Fixture {
    fn look(&self, id: u32) -> &Appearance {
        self.built.table.get(id)
    }

    /// The colour a face's layer holds: each test texture is one colour.
    fn color(&self, id: u32, face: Face) -> [u8; 4] {
        let size = self.built.textures.size as usize;
        let layer = usize::from(self.look(id).face(face).layer);
        let at = layer * size * size * 4;
        self.built.textures.mips[0][at..at + 4].try_into().unwrap()
    }
}

#[test]
fn faces_take_their_own_key_then_side_and_variant_0() {
    let f = fixture();
    // `grass_block` is keyed `grass` in blocks.json.
    assert_eq!(f.color(GRASS, Face::Up), [2, 2, 2, 255]);
    assert_eq!(f.color(GRASS, Face::Down), [3, 3, 3, 255]);
    assert_eq!(f.color(GRASS, Face::East)[..3], [4, 4, 4]);
    // Variant 0 is the fancy leaves, not the opaque copy.
    assert_eq!(f.color(LEAVES, Face::North), [5, 5, 5, 255]);
    assert_eq!(f.color(STONE, Face::South), [1, 1, 1, 255]);
}

#[test]
fn render_classes_come_from_alpha_and_the_tables() {
    let f = fixture();
    let stone = f.look(STONE);
    assert!(stone.faces.iter().all(|face| face.class == RenderClass::Opaque));
    assert!(stone.occludes);

    // The grass block: an opaque top, overlay sides and an untinted bottom.
    let grass = f.look(GRASS);
    assert_eq!(grass.tint, Tint::Grass);
    assert_eq!(grass.face(Face::Up).class, RenderClass::Opaque);
    assert!(grass.face(Face::Up).tinted);
    for side in [Face::North, Face::South, Face::West, Face::East] {
        assert_eq!(grass.face(side).class, RenderClass::Overlay);
        assert!(grass.face(side).tinted);
    }
    assert!(!grass.face(Face::Down).tinted);
    assert!(grass.occludes, "overlay faces are drawn opaque");

    let leaves = f.look(LEAVES);
    assert_eq!(leaves.face(Face::Up).class, RenderClass::Cutout);
    assert_eq!(leaves.tint, Tint::Foliage);
    assert!(!leaves.occludes);

    let glass = f.look(GLASS);
    assert!(glass.faces.iter().all(|face| face.class == RenderClass::Blend));
    assert!(!glass.occludes);
}

#[test]
fn isotropic_faces_are_flagged() {
    let f = fixture();
    let grass = f.look(GRASS);
    assert!(grass.face(Face::Up).isotropic);
    assert!(grass.face(Face::Down).isotropic);
    assert!(!grass.face(Face::North).isotropic);
    assert!(f.look(SAND).faces.iter().all(|face| face.isotropic));
    assert!(!f.look(LEAVES).face(Face::Down).isotropic);
}

#[test]
fn sideways_logs_show_their_rings_at_the_ends() {
    let f = fixture();
    let rings = [8, 8, 8, 255];
    let bark = [9, 9, 9, 255];
    assert_eq!(f.color(LOG_X, Face::East), rings);
    assert_eq!(f.color(LOG_X, Face::West), rings);
    for face in [Face::Up, Face::Down, Face::North, Face::South] {
        assert_eq!(f.color(LOG_X, face), bark);
        assert_eq!(f.look(LOG_X).face(face).turns, 1);
    }
    assert_eq!(f.color(LOG_Z, Face::North), rings);
    assert_eq!(f.color(LOG_Z, Face::Up), bark);
    assert_eq!(f.look(LOG_Z).face(Face::Up).turns, 0);
    assert_eq!(f.look(LOG_Z).face(Face::East).turns, 1);
}

#[test]
fn two_tall_plants_use_down_then_up() {
    let f = fixture();
    assert_eq!(f.look(TALL_LOWER).shape, Shape::Cross);
    assert_eq!(f.color(TALL_LOWER, Face::North)[..3], [11, 11, 11]);
    assert_eq!(f.color(TALL_UPPER, Face::North)[..3], [10, 10, 10]);
    assert_eq!(f.look(TALL_UPPER).face(Face::North).class, RenderClass::Cutout);
}

#[test]
fn missing_and_unknown_blocks_draw_the_missing_texture() {
    let f = fixture();
    assert_eq!(f.look(AIR).shape, Shape::Invisible);
    for id in [EDUCATION, BROKEN, 999] {
        assert_eq!(f.look(id).shape, Shape::Cube);
        assert_eq!(f.color(id, Face::Up), [255, 0, 255, 255]);
    }
    let report = &f.built.report;
    assert_eq!(report.names, 10);
    assert!(report.no_entry.contains("element_1"));
    assert!(report.unresolved["broken"].contains("nowhere"));
    assert_eq!(report.invisible, 1);
    // stone, grass_block, oak_leaves, white_stained_glass, oak_log, tall_grass, sand.
    assert_eq!(report.textured, 7);
    assert!(report.failed_textures.is_empty(), "{:?}", report.failed_textures);
}

#[test]
fn liquid_surfaces_follow_java_heights() {
    let heights: Vec<u8> = (0..=8).map(liquid_surface).collect();
    assert_eq!(heights, [14, 12, 11, 9, 7, 5, 4, 2, 14]);
}

/// The real palette, through Pumpkin's Java-to-Bedrock table.
fn bedrock(palette: &Palette, java: BlockStateId) -> &BedrockState {
    palette.state(JavaState::to_be_network_id(java)).expect("a palette entry")
}

/// A Java state's properties, as strings.
fn java_props(block: &Block, java: BlockStateId) -> HashMap<&'static str, &'static str> {
    block
        .properties(java)
        .map(|props| props.to_props().into_iter().map(|p| (p.key().to_str(), p.value_str())).collect())
        .unwrap_or_default()
}

#[test]
fn every_name_in_the_tables_is_in_the_palette() {
    let palette = palette_source::load().unwrap();
    let names = palette.names();
    let tables = [INVISIBLE, CROSS, TWO_TALL, MULTI_FACE, BLEND]
        .into_iter()
        .flatten()
        .copied()
        .chain([VINE, LILY_PAD, SNOW_LAYER, GRASS_SIDES])
        .chain(LIQUIDS.iter().map(|(name, _)| *name))
        .chain(TINTS.iter().map(|(name, _)| *name))
        .chain(ALIASES.iter().map(|(name, _)| *name));
    for name in tables {
        assert!(names.contains(name), "`{name}` isn't a Bedrock block name");
    }
    for suffix in CROSS_SUFFIXES.iter().chain(BLEND_SUFFIXES).chain([&CARPET_SUFFIX]) {
        assert!(names.iter().any(|name| name.ends_with(suffix)), "nothing ends with `{suffix}`");
    }
}

#[test]
fn direction_bits_match_pumpkins_table() {
    let palette = palette_source::load().unwrap();
    let checks = [
        (&Block::VINE, "vine_direction_bits", &VINE_BITS[..]),
        (&Block::GLOW_LICHEN, "multi_face_direction_bits", &MULTI_FACE_BITS[..]),
        (&Block::SCULK_VEIN, "multi_face_direction_bits", &MULTI_FACE_BITS[..]),
    ];
    for (block, key, table) in checks {
        let mut checked = 0;
        for java in block.states.iter().map(|state| state.id) {
            let props = java_props(block, java);
            let java_faces = Face::ALL
                .into_iter()
                .filter(|face| props.get(face.key()) == Some(&"true"))
                // A vine's `up` is the block above; Bedrock's bits don't say.
                .filter(|face| table.iter().any(|(_, f)| f == face))
                .fold(0u8, |faces, face| faces | 1 << face as u8);
            let bits = bedrock(&palette, java).int(key).expect("the bits");
            assert_eq!(faces_of(bits, table), java_faces, "{} {props:?}: bits {bits}", block.name);
            checked += 1;
        }
        assert!(checked > 0);
    }
}

#[test]
fn shapes_follow_states_in_the_real_palette() {
    let palette = palette_source::load().unwrap();
    let shape = |block: &Block, wanted: &[(&str, &str)]| {
        let java = block
            .states
            .iter()
            .map(|state| state.id)
            .find(|&java| {
                let props = java_props(block, java);
                wanted.iter().all(|(k, v)| props.get(k) == Some(v))
            })
            .expect("a matching Java state");
        shape_of(bedrock(&palette, java))
    };
    assert_eq!(shape(&Block::OAK_SLAB, &[("type", "bottom")]), Shape::Slab { bottom: 0, top: 8 });
    assert_eq!(shape(&Block::OAK_SLAB, &[("type", "top")]), Shape::Slab { bottom: 8, top: 16 });
    assert_eq!(shape(&Block::OAK_SLAB, &[("type", "double")]), Shape::Cube);
    assert_eq!(shape(&Block::SNOW, &[("layers", "3")]), Shape::Slab { bottom: 0, top: 6 });
    assert_eq!(shape(&Block::WHITE_CARPET, &[]), Shape::Slab { bottom: 0, top: 1 });
    assert_eq!(
        shape(&Block::WATER, &[("level", "0")]),
        Shape::Liquid { liquid: Liquid::Water, surface: 14 }
    );
    assert_eq!(shape(&Block::POPPY, &[]), Shape::Cross);
    assert_eq!(shape(&Block::OAK_SAPLING, &[]), Shape::Cross);
    assert_eq!(shape(&Block::STONE, &[]), Shape::Cube);
    assert_eq!(shape(&Block::BARRIER, &[]), Shape::Invisible);
}

/// The real vanilla packs, read at runtime; nothing is written. Run it with
/// `LIMONITE_BEDROCK` set to the folder that holds `data/resource_packs`.
#[test]
#[ignore = "needs a Bedrock install; set LIMONITE_BEDROCK"]
fn the_real_install_looks_right() {
    let dir = std::env::var_os("LIMONITE_BEDROCK").expect("LIMONITE_BEDROCK is set");
    let install = crate::install::find(Some(std::path::Path::new(&dir))).unwrap();
    let vanilla = crate::install::load_vanilla(&install).unwrap();
    let palette = palette_source::load().unwrap();
    let built = build(&palette, &vanilla.blocks, &vanilla.terrain, &vanilla.stack);
    let look = |block: &Block| *built.table.get(JavaState::to_be_network_id(block.default_state.id));

    let grass = look(&Block::GRASS_BLOCK);
    assert_eq!((grass.shape, grass.tint), (Shape::Cube, Tint::Grass));
    assert_eq!(grass.face(Face::Up).class, RenderClass::Opaque);
    assert_eq!(grass.face(Face::North).class, RenderClass::Overlay);
    assert!(!grass.face(Face::Down).tinted);
    assert!(grass.face(Face::Up).isotropic);
    assert!(grass.occludes);

    for (block, tint) in [
        (&Block::OAK_LEAVES, Tint::Foliage),
        (&Block::BIRCH_LEAVES, Tint::Birch),
        (&Block::SPRUCE_LEAVES, Tint::Evergreen),
    ] {
        let leaves = look(block);
        assert_eq!((leaves.tint, leaves.face(Face::Up).class), (tint, RenderClass::Cutout), "{}", block.name);
        assert!(!leaves.occludes);
    }

    let water = look(&Block::WATER);
    assert_eq!(water.shape, Shape::Liquid { liquid: Liquid::Water, surface: 14 });
    assert_eq!((water.tint, water.face(Face::Up).class), (Tint::Water, RenderClass::Blend));
    let lava = look(&Block::LAVA);
    assert_eq!(lava.liquid(), Some(Liquid::Lava));
    assert_eq!(lava.face(Face::Up).class, RenderClass::Opaque);

    let stone = look(&Block::STONE);
    assert!(stone.occludes && stone.face(Face::Up).layer != 0);
    let short_grass = look(&Block::SHORT_GRASS);
    assert_eq!((short_grass.shape, short_grass.tint), (Shape::Cross, Tint::Grass));
    assert_eq!(short_grass.face(Face::North).class, RenderClass::Cutout);
    assert_eq!(look(&Block::GLASS).face(Face::Up).class, RenderClass::Cutout);
    assert_eq!(look(&Block::WHITE_STAINED_GLASS).face(Face::Up).class, RenderClass::Blend);
    assert!(look(&Block::SAND).face(Face::Up).isotropic);

    let report = &built.report;
    assert!(report.failed_textures.is_empty(), "{:?}", report.failed_textures);
    // Java's game-test blocks are the only ones sent as Bedrock's `unknown`.
    assert_eq!(report.no_entry_sent.iter().collect::<Vec<_>>(), ["unknown"]);
    println!(
        "{} of {} names textured; {} layers of {}",
        report.textured, report.names, built.textures.layers, built.textures.size
    );
}
