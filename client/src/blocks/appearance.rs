//! How every Bedrock block state looks (m4-plan Decision 5): its shape, the
//! texture on each face, how each face is drawn, and its tint.
//!
//! - **Textures** come from the packs. `blocks.json` names a texture key for
//!   each face, and `terrain_texture.json` a path for each key. M4 takes
//!   variant 0 of a key, the default.
//! - **Shapes, tints and the blend list** come from the tables below, keyed
//!   by Bedrock name. The install has no geometry for vanilla blocks
//!   (Finding 8), so a client has to know the shapes itself. Any block not in
//!   a table is drawn as a cube, and the log names the ones that aren't
//!   really cubes.
//! - **Render classes** come from the textures' alpha: a face whose texture
//!   has a see-through texel is alpha-tested.
//!
//! The table is built once at startup, for every id in the palette, and the
//! mesher looks ids up in it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use limonite_packs::{
    PackStack,
    registry::{BlocksJson, Face, FaceTextures, TerrainTextures},
};
use pumpkin_data::{BlockState as JavaState, BlockStateId};

use super::palette::{BedrockState, Palette};
use crate::render::{
    textures::{self, AlphaUse, TextureArray},
    tint::Tint,
};

/// Blocks renamed since `blocks.json` was keyed, which it still knows by the
/// old name (Finding 6).
const ALIASES: [(&str, &str); 8] = [
    ("grass_block", "grass"),
    ("sea_lantern", "seaLantern"),
    ("iron_chain", "chain"),
    ("invisible_bedrock", "invisibleBedrock"),
    ("moving_block", "movingBlock"),
    ("piston_arm_collision", "pistonArmCollision"),
    ("sticky_piston_arm_collision", "stickyPistonArmCollision"),
    ("trip_wire", "tripWire"),
];

/// Never drawn.
const INVISIBLE: &[&str] = &[
    "air",
    "barrier",
    "structure_void",
    "light_block_0",
    "light_block_1",
    "light_block_2",
    "light_block_3",
    "light_block_4",
    "light_block_5",
    "light_block_6",
    "light_block_7",
    "light_block_8",
    "light_block_9",
    "light_block_10",
    "light_block_11",
    "light_block_12",
    "light_block_13",
    "light_block_14",
    "light_block_15",
];

/// Two crossed planes with one texture: the plants. Saplings and corals
/// come in families, matched by [`CROSS_SUFFIXES`].
const CROSS: &[&str] = &[
    "short_grass",
    "fern",
    "deadbush",
    "bush",
    "firefly_bush",
    "short_dry_grass",
    "tall_dry_grass",
    "dandelion",
    "poppy",
    "blue_orchid",
    "allium",
    "azure_bluet",
    "red_tulip",
    "orange_tulip",
    "white_tulip",
    "pink_tulip",
    "oxeye_daisy",
    "cornflower",
    "lily_of_the_valley",
    "wither_rose",
    "torchflower",
    "closed_eyeblossom",
    "open_eyeblossom",
    "cactus_flower",
    "wheat",
    "carrots",
    "potatoes",
    "beetroot",
    "sweet_berry_bush",
    "nether_wart",
    "torchflower_crop",
    "melon_stem",
    "pumpkin_stem",
    "brown_mushroom",
    "red_mushroom",
    "crimson_fungus",
    "warped_fungus",
    "crimson_roots",
    "warped_roots",
    "nether_sprouts",
    "weeping_vines",
    "twisting_vines",
    "cave_vines",
    "cave_vines_body_with_berries",
    "cave_vines_head_with_berries",
    "hanging_roots",
    "mangrove_propagule",
    // Sugar cane and cobweb keep their old Bedrock names.
    "reeds",
    "web",
    "kelp",
    "seagrass",
];

/// Families of crossed planes, by name suffix: every sapling, and the coral
/// plants and floor fans, dead or alive. (Wall fans hang on a side; they
/// stay stand-in cubes.)
const CROSS_SUFFIXES: &[&str] = &["_sapling", "_coral", "_coral_fan"];

/// Two-tall plants: crossed planes, with the `down` texture on the lower
/// half and `up` on the upper (`upper_block_bit`).
const TWO_TALL: &[&str] = &[
    "tall_grass",
    "large_fern",
    "sunflower",
    "lilac",
    "rose_bush",
    "peony",
    "pitcher_plant",
    "pitcher_crop",
];

/// Thin planes against the faces a state names: vines by
/// `vine_direction_bits`, the others by `multi_face_direction_bits`.
const VINE: &str = "vine";
const MULTI_FACE: &[&str] = &["glow_lichen", "sculk_vein"];

/// The faces `vine_direction_bits` names, bit by bit. Checked against
/// Pumpkin's Java-to-Bedrock table in the tests.
const VINE_BITS: [(u8, Face); 4] = [
    (1, Face::South),
    (2, Face::West),
    (4, Face::North),
    (8, Face::East),
];

/// The faces `multi_face_direction_bits` names, bit by bit. Also checked
/// against Pumpkin's table.
const MULTI_FACE_BITS: [(u8, Face); 6] = [
    (1, Face::Down),
    (2, Face::Up),
    (4, Face::South),
    (8, Face::West),
    (16, Face::North),
    (32, Face::East),
];

/// Thin boxes on the floor, one sixteenth tall: carpets by suffix, and lily
/// pads.
const CARPET_SUFFIX: &str = "_carpet";
const LILY_PAD: &str = "waterlily";
/// Snow layers: `height` + 1 layers, each two sixteenths.
const SNOW_LAYER: &str = "snow_layer";

/// Liquids, by name.
const LIQUIDS: [(&str, Liquid); 4] = [
    ("water", Liquid::Water),
    ("flowing_water", Liquid::Water),
    ("lava", Liquid::Lava),
    ("flowing_lava", Liquid::Lava),
];

/// Drawn with alpha blending, back to front, in the second pass. Stained
/// glass, and its panes, by suffix.
const BLEND: &[&str] = &["water", "flowing_water", "ice", "slime", "honey_block"];
const BLEND_SUFFIXES: &[&str] = &["stained_glass", "stained_glass_pane"];

/// What each tinted block is tinted by.
const TINTS: &[(&str, Tint)] = &[
    ("grass_block", Tint::Grass),
    ("short_grass", Tint::Grass),
    ("tall_grass", Tint::Grass),
    ("fern", Tint::Grass),
    ("large_fern", Tint::Grass),
    ("reeds", Tint::Grass),
    ("oak_leaves", Tint::Foliage),
    ("jungle_leaves", Tint::Foliage),
    ("acacia_leaves", Tint::Foliage),
    ("dark_oak_leaves", Tint::Foliage),
    ("mangrove_leaves", Tint::Foliage),
    ("vine", Tint::Foliage),
    ("birch_leaves", Tint::Birch),
    ("spruce_leaves", Tint::Evergreen),
    ("water", Tint::Water),
    ("flowing_water", Tint::Water),
];

/// Blocks whose sides are drawn opaque, their texture's alpha marking the
/// texels to tint, and whose bottom isn't tinted (Finding 5).
const GRASS_SIDES: &str = "grass_block";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liquid {
    Water,
    Lava,
}

/// A block's geometry. Heights are in sixteenths of a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Invisible,
    Cube,
    /// Two planes crossed diagonally: plants.
    Cross,
    /// `surface` is the top's height when the same liquid isn't above.
    Liquid { liquid: Liquid, surface: u8 },
    /// A full-width box from `bottom` to `top`: slabs, snow, carpets.
    Slab { bottom: u8, top: u8 },
    /// Thin planes just inside the faces in this set of `1 << face` bits.
    Planes { faces: u8 },
}

/// How a face's texels are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderClass {
    /// Every texel opaque.
    Opaque,
    /// Some texels see-through: alpha-tested, so they're either there or not.
    Cutout,
    /// Drawn opaque, with alpha marking the texels to tint.
    Overlay,
    /// Alpha-blended, in the second pass.
    Blend,
}

/// One face, ready for the mesher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceLook {
    /// The face's layer in the texture array; 0 is the missing texture.
    pub layer: u16,
    pub class: RenderClass,
    /// Whether the face takes its block's tint. An overlay face tints only
    /// the texels its alpha marks.
    pub tinted: bool,
    /// Quarter turns of the texture, always applied: a sideways log's bark.
    pub turns: u8,
    /// Whether the mesher may add a quarter turn picked by position.
    pub isotropic: bool,
}

/// How one block state looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub shape: Shape,
    pub tint: Tint,
    /// Indexed by `Face as usize`: down, up, north, south, west, east. Crossed
    /// planes and attached planes use the same look in every slot.
    pub faces: [FaceLook; 6],
    /// A full cube whose faces are all opaque or overlay: it hides the faces
    /// of its neighbours.
    pub occludes: bool,
}

impl Appearance {
    /// A cube in the missing texture: for ids the palette doesn't have.
    const MISSING: Self = Self {
        shape: Shape::Cube,
        tint: Tint::None,
        faces: [FaceLook {
            layer: 0,
            class: RenderClass::Opaque,
            tinted: false,
            turns: 0,
            isotropic: false,
        }; 6],
        occludes: true,
    };

    #[cfg_attr(not(test), expect(dead_code, reason = "the window's meshes use it (M4 Step 10)"))]
    #[must_use]
    pub const fn face(&self, face: Face) -> &FaceLook {
        &self.faces[face as usize]
    }

    /// The liquid it is, if it's one.
    #[cfg_attr(not(test), expect(dead_code, reason = "the window's meshes use it (M4 Step 10)"))]
    #[must_use]
    pub const fn liquid(&self) -> Option<Liquid> {
        match self.shape {
            Shape::Liquid { liquid, .. } => Some(liquid),
            _ => None,
        }
    }
}

/// Every palette id's appearance.
#[cfg_attr(not(test), expect(dead_code, reason = "the window's meshes use it (M4 Step 10)"))]
pub struct AppearanceTable {
    by_id: HashMap<u32, Appearance>,
}

impl AppearanceTable {
    /// The id's appearance. An id the palette doesn't know is a cube in the
    /// missing texture.
    #[cfg_attr(not(test), expect(dead_code, reason = "the window's meshes use it (M4 Step 10)"))]
    #[must_use]
    pub fn get(&self, id: u32) -> &Appearance {
        self.by_id.get(&id).unwrap_or(&Appearance::MISSING)
    }

    /// A table of hand-made appearances, for tests that don't need packs.
    #[cfg(test)]
    pub fn from_looks(looks: impl IntoIterator<Item = (u32, Appearance)>) -> Self {
        Self {
            by_id: looks.into_iter().collect(),
        }
    }
}

/// What building the table found, for the startup log.
#[derive(Debug, Default)]
pub struct Report {
    /// Distinct block names in the palette.
    pub names: usize,
    /// Names with a `blocks.json` entry, after aliasing.
    pub with_entry: usize,
    /// Names drawn with every texture they need.
    pub textured: usize,
    /// Names never drawn.
    pub invisible: usize,
    /// Names without a `blocks.json` entry: Education Edition and
    /// engine-internal blocks.
    pub no_entry: BTreeSet<String>,
    /// Of those, the ones Pumpkin can send. Should be none.
    pub no_entry_sent: BTreeSet<String>,
    /// Names with an entry whose textures don't all resolve, and why.
    pub unresolved: BTreeMap<String, String>,
    /// Names drawn as cubes whose Java shape isn't a full cube.
    pub stand_ins: BTreeSet<String>,
    /// Texture files that couldn't be loaded, and why.
    pub failed_textures: Vec<(String, String)>,
}

/// The table, its texture array, and the report.
pub struct Built {
    #[cfg_attr(not(test), expect(dead_code, reason = "the window's meshes use it (M4 Step 10)"))]
    pub table: AppearanceTable,
    pub textures: TextureArray,
    pub texture_seconds: f64,
    pub report: Report,
}

/// One face before textures are loaded: which requested texture, if any.
#[derive(Clone, Copy, Default)]
struct FacePlan {
    texture: Option<usize>,
    overlay: bool,
    tinted: bool,
    turns: u8,
    isotropic: bool,
}

struct Plan {
    shape: Shape,
    tint: Tint,
    blend: bool,
    faces: [FacePlan; 6],
}

/// Builds the table for every palette id, and loads the textures it uses.
#[must_use]
pub fn build(palette: &Palette, blocks: &BlocksJson, terrain: &TerrainTextures, stack: &PackStack) -> Built {
    let mut resolver = Resolver {
        blocks,
        terrain,
        requests: Vec::new(),
        requested: HashMap::new(),
    };
    let mut report = Report::default();
    let mut plans = Vec::with_capacity(palette.len());
    let mut problems: HashMap<&str, String> = HashMap::new();
    for (id, state) in palette.iter() {
        let (plan, problem) = resolver.plan(state);
        if let Some(problem) = problem {
            problems.entry(&*state.name).or_insert(problem);
        }
        plans.push((id, &*state.name, plan));
    }

    let loaded = textures::load(stack, &resolver.requests);
    let mut failed_names: HashMap<&str, String> = HashMap::new();
    let java_full_cube = java_full_cubes();
    let mut by_id = HashMap::with_capacity(plans.len());
    for (id, name, plan) in plans {
        let mut faces = [Appearance::MISSING.faces[0]; 6];
        for (look, face) in faces.iter_mut().zip(&plan.faces) {
            let layer = face.texture.and_then(|index| {
                let layer = loaded.layers[index];
                if layer.is_none() {
                    failed_names
                        .entry(name)
                        .or_insert_with(|| format!("`{}` didn't load", resolver.requests[index].0));
                }
                layer
            });
            look.layer = layer.map_or(0, |layer| layer.index);
            look.class = if plan.blend {
                RenderClass::Blend
            } else if face.overlay {
                RenderClass::Overlay
            } else if layer.is_some_and(|layer| layer.transparent) {
                RenderClass::Cutout
            } else {
                RenderClass::Opaque
            };
            look.tinted = face.tinted;
            look.turns = face.turns;
            look.isotropic = face.isotropic;
        }
        let occludes = plan.shape == Shape::Cube
            && faces
                .iter()
                .all(|face| matches!(face.class, RenderClass::Opaque | RenderClass::Overlay));
        if plan.shape == Shape::Cube && java_full_cube.get(&id) == Some(&false) {
            report.stand_ins.insert(name.to_string());
        }
        by_id.insert(
            id,
            Appearance {
                shape: plan.shape,
                tint: plan.tint,
                faces,
                occludes,
            },
        );
    }

    // Per name: the palette counts each state, the report each block.
    let names = palette.names();
    let sendable = sendable_names(palette);
    report.names = names.len();
    report.invisible = names.iter().filter(|name| INVISIBLE.contains(name)).count();
    for name in names {
        let key = blocks_json_key(name);
        if blocks.blocks.contains_key(key) {
            report.with_entry += 1;
        } else {
            report.no_entry.insert(name.to_string());
            if sendable.contains(name) {
                report.no_entry_sent.insert(name.to_string());
            }
            continue;
        }
        match problems.get(name).or_else(|| failed_names.get(name)) {
            Some(problem) => {
                report.unresolved.insert(name.to_string(), problem.clone());
            }
            None if !INVISIBLE.contains(&name) => report.textured += 1,
            None => {}
        }
    }
    report.failed_textures = loaded.failed;
    Built {
        table: AppearanceTable { by_id },
        textures: loaded.array,
        texture_seconds: loaded.seconds,
        report,
    }
}

/// Resolves block names to texture paths, and collects the textures to load.
struct Resolver<'a> {
    blocks: &'a BlocksJson,
    terrain: &'a TerrainTextures,
    requests: Vec<(String, AlphaUse)>,
    requested: HashMap<(String, AlphaUse), usize>,
}

impl Resolver<'_> {
    /// One state's plan, and the first problem resolving its textures.
    fn plan(&mut self, state: &BedrockState) -> (Plan, Option<String>) {
        let name = &*state.name;
        let shape = shape_of(state);
        let tint = TINTS
            .iter()
            .find(|(tinted, _)| *tinted == name)
            .map_or(Tint::None, |&(_, tint)| tint);
        let blend = BLEND.contains(&name) || BLEND_SUFFIXES.iter().any(|suffix| name.ends_with(suffix));
        let mut plan = Plan {
            shape,
            tint,
            blend,
            faces: [FacePlan::default(); 6],
        };
        if shape == Shape::Invisible {
            return (plan, None);
        }

        let entry = self.blocks.blocks.get(blocks_json_key(name));
        let Some(textures) = entry.and_then(|entry| entry.textures.as_ref()) else {
            let problem = if entry.is_some() {
                "its blocks.json entry has no textures"
            } else {
                "no blocks.json entry"
            };
            return (plan, Some(problem.to_string()));
        };
        let isotropic = entry.and_then(|entry| entry.isotropic.as_ref());

        let mut problem = None;
        // Which key each face shows, and how far it's turned.
        let keys: [(Option<&str>, u8); 6] = match shape {
            Shape::Cross | Shape::Planes { .. } => {
                let key = cross_key(state, textures);
                [(key, 0); 6]
            }
            Shape::Cube => pillar_keys(state, textures),
            _ => Face::ALL.map(|face| (textures.key(face), 0)),
        };
        for (face, (key, turns)) in Face::ALL.into_iter().zip(keys) {
            let overlay = name == GRASS_SIDES && face.is_side();
            let tinted = tint != Tint::None && !(name == GRASS_SIDES && face == Face::Down);
            let alpha = if overlay { AlphaUse::Mask } else { AlphaUse::Coverage };
            let texture = match key.map(|key| (key, self.path(key))) {
                Some((_, Ok(path))) => Some(self.request(path, alpha)),
                Some((key, Err(err))) => {
                    problem.get_or_insert(format!("texture key `{key}`: {err}"));
                    None
                }
                None => {
                    problem.get_or_insert(format!("no texture for its {} face", face.key()));
                    None
                }
            };
            plan.faces[face as usize] = FacePlan {
                texture,
                overlay,
                tinted,
                turns,
                isotropic: isotropic.is_some_and(|iso| iso.applies(face)),
            };
        }
        (plan, problem)
    }

    /// A texture key's path: variant 0 of its `terrain_texture.json` entry.
    fn path(&self, key: &str) -> Result<String, &'static str> {
        let entry = self.terrain.textures.get(key).ok_or("not in terrain_texture.json")?;
        entry
            .textures
            .first()
            .map(|texture| texture.path.clone())
            .ok_or("has no variants")
    }

    /// The index of a texture to load, asking for each one once.
    fn request(&mut self, path: String, alpha: AlphaUse) -> usize {
        let next = self.requests.len();
        *self.requested.entry((path.clone(), alpha)).or_insert_with(|| {
            self.requests.push((path, alpha));
            next
        })
    }
}

/// The key a block has in `blocks.json`.
fn blocks_json_key(name: &str) -> &str {
    ALIASES
        .iter()
        .find(|(renamed, _)| *renamed == name)
        .map_or(name, |&(_, key)| key)
}

/// A state's shape, from the tables and its properties.
fn shape_of(state: &BedrockState) -> Shape {
    let name = &*state.name;
    if INVISIBLE.contains(&name) {
        return Shape::Invisible;
    }
    if let Some(&(_, liquid)) = LIQUIDS.iter().find(|(liquid, _)| *liquid == name) {
        return Shape::Liquid {
            liquid,
            surface: liquid_surface(state.int("liquid_depth").unwrap_or(0)),
        };
    }
    // Every block with this property is a slab. The double slabs are full
    // cubes, and their names all say "double".
    if let Some(half) = state.string("minecraft:vertical_half")
        && !name.contains("double")
    {
        return if half == "top" {
            Shape::Slab { bottom: 8, top: 16 }
        } else {
            Shape::Slab { bottom: 0, top: 8 }
        };
    }
    if name == SNOW_LAYER {
        let layers = state.int("height").unwrap_or(0).clamp(0, 7) as u8 + 1;
        return Shape::Slab { bottom: 0, top: layers * 2 };
    }
    if name.ends_with(CARPET_SUFFIX) || name == LILY_PAD {
        return Shape::Slab { bottom: 0, top: 1 };
    }
    if name == VINE {
        let bits = state.int("vine_direction_bits").unwrap_or(0);
        // With no side, a vine hangs from the block above.
        let faces = faces_of(bits, &VINE_BITS);
        return Shape::Planes {
            faces: if faces == 0 { 1 << Face::Up as u8 } else { faces },
        };
    }
    if MULTI_FACE.contains(&name) {
        let bits = state.int("multi_face_direction_bits").unwrap_or(0);
        return Shape::Planes {
            faces: faces_of(bits, &MULTI_FACE_BITS),
        };
    }
    if CROSS.contains(&name)
        || TWO_TALL.contains(&name)
        || CROSS_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
    {
        return Shape::Cross;
    }
    Shape::Cube
}

/// A liquid's surface, in sixteenths, when the same liquid isn't above:
/// Java's heights. A source (depth 0) is 8/9 of a block, and each step of
/// flowing water is a ninth lower. Depth 8 and up is falling: full height
/// less the same ninth.
fn liquid_surface(depth: i32) -> u8 {
    let level = if depth >= 8 { 0 } else { depth.clamp(0, 7) };
    // (8 - level) / 9 of 16 sixteenths, rounded: floor(x + 1/2) with x =
    // 16 (8 - level) / 9, all in integers. At most 14, so it fits a u8.
    (((8 - level) * 32 + 9) / 18) as u8
}

/// The faces a direction bit field names, as `1 << face` bits.
fn faces_of(bits: i32, table: &[(u8, Face)]) -> u8 {
    table
        .iter()
        .filter(|(bit, _)| bits & i32::from(*bit) != 0)
        .fold(0, |faces, &(_, face)| faces | 1 << face as u8)
}

/// The one texture key of crossed or attached planes.
///
/// Bedrock's own renderers for these blocks use the face slots as a bag of
/// per-state textures. Two-tall plants show `down` on the lower half and `up`
/// on the upper; seagrass picks by `sea_grass_type`. Everything else shows
/// its north face's key: its only key, or `side`.
fn cross_key<'a>(state: &BedrockState, textures: &'a FaceTextures) -> Option<&'a str> {
    let name = &*state.name;
    if TWO_TALL.contains(&name) {
        let upper = state.flag("upper_block_bit").unwrap_or(false);
        return textures.key(if upper { Face::Up } else { Face::Down });
    }
    if name == "seagrass" {
        return textures.key(match state.string("sea_grass_type") {
            Some("double_bot") => Face::Down,
            Some("double_top") => Face::North,
            _ => Face::Up,
        });
    }
    textures.key(Face::North)
}

/// A cube's face keys and turns, following `pillar_axis` for logs, pillars
/// and the like.
///
/// Standing up (axis y, or no axis), every face shows its own key. Lying
/// along x or z, the two faces at the ends show the `up` and `down` keys,
/// and the four around the length show the side, turned a quarter where
/// that runs the grain along the length. Which faces need the turn follows
/// from the mesher's texture orientation (`render::mesher`).
fn pillar_keys<'a>(state: &BedrockState, textures: &'a FaceTextures) -> [(Option<&'a str>, u8); 6] {
    let side = textures.key(Face::North);
    let own = |face| (textures.key(face), 0);
    match state.string("pillar_axis") {
        Some("x") => [
            (side, 1),                     // down
            (side, 1),                     // up
            (side, 1),                     // north
            (side, 1),                     // south
            (textures.key(Face::Down), 0), // west
            (textures.key(Face::Up), 0),   // east
        ],
        Some("z") => [
            (side, 0),                     // down
            (side, 0),                     // up
            (textures.key(Face::Down), 0), // north
            (textures.key(Face::Up), 0),   // south
            (side, 1),                     // west
            (side, 1),                     // east
        ],
        _ => Face::ALL.map(own),
    }
}

/// For each Bedrock id Pumpkin can send, whether its Java block is drawn as
/// a full cube: an outline that is exactly one whole block. True if any of
/// the Java states sharing the id is.
fn java_full_cubes() -> HashMap<u32, bool> {
    let mut full = HashMap::new();
    for state in (0..BlockStateId::COUNT).filter_map(BlockStateId::new) {
        let is_full = state.to_state().get_block_outline_shapes().any(|shape| {
            shape.min.x == 0.0
                && shape.min.y == 0.0
                && shape.min.z == 0.0
                && shape.max.x == 1.0
                && shape.max.y == 1.0
                && shape.max.z == 1.0
        });
        *full.entry(JavaState::to_be_network_id(state)).or_insert(false) |= is_full;
    }
    full
}

/// The names of the blocks Pumpkin can send.
fn sendable_names(palette: &Palette) -> HashSet<&str> {
    (0..BlockStateId::COUNT)
        .filter_map(BlockStateId::new)
        .filter_map(|state| palette.state(JavaState::to_be_network_id(state)))
        .map(|state| &*state.name)
        .collect()
}

#[cfg(test)]
mod tests;
