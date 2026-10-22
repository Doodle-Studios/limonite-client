//! The registry files that merge across a stack: `blocks.json` and
//! `textures/terrain_texture.json`, as typed values.
//!
//! PLAN.md's rule: registry JSON merges per key, and the higher pack wins.
//! - **`terrain_texture.json`** merges per texture key: a higher pack's
//!   entry replaces the whole entry.
//! - **`blocks.json`** merges per block name, and within a block field by
//!   field: a higher pack's `textures` replaces the lower `textures`, and the
//!   fields it doesn't mention are kept. Vanilla relies on this: 1.21.70's
//!   `blocks.json` gives the terracottas only a new `sound`, and they keep the
//!   textures 1.20.30 gave them (m4-plan Deviations).
//!
//! An entry or a file that can't be parsed is skipped and reported as a
//! [`Problem`], never a panic: the client draws what it can.

use std::{collections::HashMap, fmt};

use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::{json, stack::PackStack};

pub const BLOCKS_JSON: &str = "blocks.json";
pub const TERRAIN_TEXTURE_JSON: &str = "textures/terrain_texture.json";

/// Something a loader skipped, and where it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The pack's folder name.
    pub pack: String,
    /// The file, with an entry's key when it's about one entry.
    pub file: String,
    pub message: String,
}

impl Problem {
    pub fn new(pack: impl fmt::Display, file: impl fmt::Display, message: impl fmt::Display) -> Self {
        Self {
            pack: pack.to_string(),
            file: file.to_string(),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.pack, self.file, self.message)
    }
}

/// A block face, named as `blocks.json` names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Face {
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];

    /// The key `blocks.json` uses for this face.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    /// One of the four faces `side` covers.
    #[must_use]
    pub const fn is_side(self) -> bool {
        !matches!(self, Self::Down | Self::Up)
    }
}

/// `blocks.json`, merged over a stack: what each block name looks like.
#[derive(Clone, Debug, Default)]
pub struct BlocksJson {
    pub blocks: HashMap<String, BlockEntry>,
}

/// One block's entry. Fields the client doesn't use yet, such as
/// `ambient_occlusion_exponent`, are ignored.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct BlockEntry {
    /// The `terrain_texture.json` key for each face.
    pub textures: Option<FaceTextures>,
    /// The same for the block as an inventory item.
    pub carried_textures: Option<FaceTextures>,
    /// Faces that may be drawn turned at random, so a field of them doesn't
    /// show a repeating pattern.
    pub isotropic: Option<Isotropic>,
    pub sound: Option<String>,
}

impl BlockEntry {
    /// Lays `higher` over this entry: each field it has replaces this one's.
    fn merge(&mut self, higher: Self) {
        let Self {
            textures,
            carried_textures,
            isotropic,
            sound,
        } = higher;
        // Destructuring without `..` makes a new field a compile error here
        // until it's merged too.
        self.textures = textures.or(self.textures.take());
        self.carried_textures = carried_textures.or(self.carried_textures.take());
        self.isotropic = isotropic.or(self.isotropic.take());
        self.sound = sound.or(self.sound.take());
    }
}

/// A block's texture keys: one for all six faces, or one per face.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum FaceTextures {
    All(String),
    Faces(FaceKeys),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct FaceKeys {
    pub up: Option<String>,
    pub down: Option<String>,
    pub side: Option<String>,
    pub north: Option<String>,
    pub south: Option<String>,
    pub east: Option<String>,
    pub west: Option<String>,
}

impl FaceTextures {
    /// The texture key for `face`: the face's own key wins, then `side` for
    /// the four sides. A single key covers all six faces.
    #[must_use]
    pub fn key(&self, face: Face) -> Option<&str> {
        let keys = match self {
            Self::All(key) => return Some(key),
            Self::Faces(keys) => keys,
        };
        let own = match face {
            Face::Down => &keys.down,
            Face::Up => &keys.up,
            Face::North => &keys.north,
            Face::South => &keys.south,
            Face::West => &keys.west,
            Face::East => &keys.east,
        };
        own.as_deref()
            .or_else(|| keys.side.as_deref().filter(|_| face.is_side()))
    }
}

/// Which faces are `isotropic`: all or none, or per face.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Isotropic {
    All(bool),
    Faces(HashMap<String, bool>),
}

impl Isotropic {
    /// Whether `face` may be drawn turned: its own key, then `side` for the
    /// four sides.
    #[must_use]
    pub fn applies(&self, face: Face) -> bool {
        match self {
            Self::All(all) => *all,
            Self::Faces(faces) => faces
                .get(face.key())
                .or_else(|| faces.get("side").filter(|_| face.is_side()))
                .copied()
                .unwrap_or(false),
        }
    }
}

impl BlocksJson {
    /// Every pack's `blocks.json`, merged bottom up.
    #[must_use]
    pub fn load(stack: &PackStack) -> (Self, Vec<Problem>) {
        let mut merged = Self::default();
        let mut problems = Vec::new();
        for (pack, entries) in registry_layers(stack, BLOCKS_JSON, None, &mut problems) {
            for (name, value) in entries {
                // The only key that isn't a block: an array, not an entry.
                if name == "format_version" {
                    continue;
                }
                match serde_json::from_value::<BlockEntry>(value) {
                    Ok(entry) => merged.blocks.entry(name).or_default().merge(entry),
                    Err(err) => problems.push(Problem::new(&pack, format!("{BLOCKS_JSON} `{name}`"), err)),
                }
            }
        }
        (merged, problems)
    }
}

/// `textures/terrain_texture.json`, merged over a stack: where each texture
/// key's image is.
#[derive(Clone, Debug, Default)]
pub struct TerrainTextures {
    pub textures: HashMap<String, TextureEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct TextureEntry {
    pub textures: TextureVariants,
    /// Set on the flowing liquids, whose texture covers two blocks.
    #[serde(default, deserialize_with = "flag")]
    pub quad: bool,
}

/// A texture key's images. Several are variants: old data values, growth
/// stages, or a leaf block's fast-graphics copy. The first is the default.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum TextureVariants {
    Many(Vec<TextureRef>),
    One(TextureRef),
}

impl TextureVariants {
    /// The variants in order.
    #[must_use]
    pub fn as_slice(&self) -> &[TextureRef] {
        match self {
            Self::Many(many) => many,
            Self::One(one) => std::slice::from_ref(one),
        }
    }

    /// Variant 0, the default.
    #[must_use]
    pub fn first(&self) -> Option<&TextureRef> {
        self.as_slice().first()
    }
}

/// One image: a path without an extension, and the colours some entries
/// carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureRef {
    pub path: String,
    /// A baked tint for the image's overlay pixels, such as a grass side's
    /// green. `#RRGGBB`.
    pub overlay_color: Option<String>,
    /// A baked tint for the whole image. `#RRGGBB`.
    pub tint_color: Option<String>,
}

impl<'de> Deserialize<'de> for TextureRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// A path alone, or an object with the path and its colours.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Path(String),
            Object {
                path: String,
                overlay_color: Option<String>,
                tint_color: Option<String>,
            },
        }
        Ok(match Written::deserialize(deserializer)? {
            Written::Path(path) => Self {
                path,
                overlay_color: None,
                tint_color: None,
            },
            Written::Object {
                path,
                overlay_color,
                tint_color,
            } => Self {
                path,
                overlay_color,
                tint_color,
            },
        })
    }
}

/// A flag written as a number (`"quad": 1`) or a bool.
fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Written {
        Bool(bool),
        Number(f64),
    }
    Ok(match Written::deserialize(deserializer)? {
        Written::Bool(flag) => flag,
        Written::Number(n) => n != 0.0,
    })
}

impl TerrainTextures {
    /// Every pack's `terrain_texture.json`, merged bottom up per texture key.
    #[must_use]
    pub fn load(stack: &PackStack) -> (Self, Vec<Problem>) {
        let mut merged = Self::default();
        let mut problems = Vec::new();
        for (pack, entries) in registry_layers(stack, TERRAIN_TEXTURE_JSON, Some("texture_data"), &mut problems) {
            for (key, value) in entries {
                match serde_json::from_value::<TextureEntry>(value) {
                    Ok(entry) => {
                        merged.textures.insert(key, entry);
                    }
                    Err(err) => problems.push(Problem::new(&pack, format!("{TERRAIN_TEXTURE_JSON} `{key}`"), err)),
                }
            }
        }
        (merged, problems)
    }
}

/// Each pack's copy of a registry file, bottom first, as the JSON object
/// that holds its entries: the whole file, or the object under `section`.
/// Packs without the file are left out; files that can't be read or parsed
/// become problems.
fn registry_layers(
    stack: &PackStack,
    file: &'static str,
    section: Option<&'static str>,
    problems: &mut Vec<Problem>,
) -> Vec<(String, Map<String, Value>)> {
    let mut layers = Vec::new();
    for (pack, read) in stack.layers(file) {
        let bytes = match read {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(err) => {
                problems.push(Problem::new(pack.name(), file, err));
                continue;
            }
        };
        let mut object = match json::parse::<Map<String, Value>>(&bytes) {
            Ok(object) => object,
            Err(err) => {
                problems.push(Problem::new(pack.name(), file, err));
                continue;
            }
        };
        let entries = match section {
            None => object,
            Some(section) => match object.remove(section) {
                Some(Value::Object(entries)) => entries,
                Some(_) => {
                    problems.push(Problem::new(pack.name(), file, format!("`{section}` isn't an object")));
                    continue;
                }
                None => {
                    problems.push(Problem::new(pack.name(), file, format!("no `{section}`")));
                    continue;
                }
            },
        };
        layers.push((pack.name().to_string(), entries));
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pack::Pack,
        test_support::{TempDir, archive},
    };

    fn stack_of(dir: &TempDir, names: &[&str]) -> PackStack {
        PackStack::new(names.iter().map(|name| Pack::new(dir.path().join(name))).collect())
    }

    #[test]
    fn blocks_merge_per_name_then_per_field() {
        let dir = TempDir::new("blocks-merge");
        dir.write(
            "low/blocks.json",
            br#"{
                "format_version": [1, 1, 0],
                "stone": { "textures": "stone", "sound": "stone" },
                "grass": {
                    "isotropic": { "up": true, "down": true },
                    "textures": { "up": "grass_top", "down": "grass_bottom", "side": "grass_side" }
                },
                "terracotta": { "textures": "terracotta", "sound": "stone" }
            }"#,
        );
        dir.write(
            "high/blocks.json",
            br#"{
                // A comment, as Bedrock allows.
                "stone": { "textures": "smooth_stone" },
                "terracotta": { "sound": "terracotta" },
                "new_block": { "textures": "new" }
            }"#,
        );
        let (blocks, problems) = BlocksJson::load(&stack_of(&dir, &["low", "high"]));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(blocks.blocks.len(), 4, "format_version isn't a block");

        // The higher pack's textures win; the lower pack's sound stays.
        let stone = &blocks.blocks["stone"];
        assert_eq!(stone.textures, Some(FaceTextures::All("smooth_stone".into())));
        assert_eq!(stone.sound.as_deref(), Some("stone"));
        // A higher entry with only a sound keeps the lower textures.
        let terracotta = &blocks.blocks["terracotta"];
        assert_eq!(terracotta.textures, Some(FaceTextures::All("terracotta".into())));
        assert_eq!(terracotta.sound.as_deref(), Some("terracotta"));
        // Untouched entries pass through.
        let grass = &blocks.blocks["grass"];
        assert!(grass.isotropic.as_ref().is_some_and(|iso| iso.applies(Face::Up)));
        assert!(blocks.blocks.contains_key("new_block"));
    }

    #[test]
    fn face_keys_resolve_own_key_then_side() {
        let all = FaceTextures::All("planks".into());
        assert!(Face::ALL.iter().all(|&face| all.key(face) == Some("planks")));

        let faces: FaceTextures = serde_json::from_str(
            r#"{ "up": "log_top", "down": "log_bottom", "side": "log_side", "north": "log_front" }"#,
        )
        .unwrap();
        assert_eq!(faces.key(Face::Up), Some("log_top"));
        assert_eq!(faces.key(Face::Down), Some("log_bottom"));
        assert_eq!(faces.key(Face::North), Some("log_front"));
        assert_eq!(faces.key(Face::South), Some("log_side"));
        assert_eq!(faces.key(Face::East), Some("log_side"));

        // `side` never covers the top or bottom.
        let sides_only: FaceTextures = serde_json::from_str(r#"{ "side": "s" }"#).unwrap();
        assert_eq!(sides_only.key(Face::Up), None);
        assert_eq!(sides_only.key(Face::West), Some("s"));
    }

    #[test]
    fn isotropic_is_all_or_per_face() {
        assert!(Isotropic::All(true).applies(Face::North));
        assert!(!Isotropic::All(false).applies(Face::Up));
        let per_face: Isotropic = serde_json::from_str(r#"{ "up": true, "side": true }"#).unwrap();
        assert!(per_face.applies(Face::Up));
        assert!(!per_face.applies(Face::Down));
        assert!(per_face.applies(Face::East));
    }

    #[test]
    fn terrain_textures_merge_per_key_and_read_every_shape() {
        let dir = TempDir::new("terrain");
        dir.write(
            "low/textures/terrain_texture.json",
            br#"{
                "resource_pack_name": "vanilla",
                "texture_data": {
                    "stone": { "textures": "textures/blocks/stone" },
                    "dirt": { "textures": "textures/blocks/dirt" }
                }
            }"#,
        );
        // The higher pack's copy is archived, as most vanilla files are. Two
        // `#`s, because the colours hold `"#`, which would end `r#"`.
        let high = br##"{
            "texture_data": {
                "stone": { "textures": "textures/blocks/stone_new" },
                "grass_side": { "textures": [
                    { "path": "textures/blocks/grass_side", "overlay_color": "#79c05a" },
                    "textures/blocks/grass_side_snowed"
                ] },
                "grass_carried": { "textures": { "path": "textures/blocks/grass_side", "tint_color": "#79c05a" } },
                "flowing_water": { "quad": 1, "textures": "textures/blocks/water_flow" }
            }
        }"##;
        dir.write(
            "high/__brarchive/textures.brarchive",
            &archive(&[("terrain_texture.json", high)]),
        );
        let (terrain, problems) = TerrainTextures::load(&stack_of(&dir, &["low", "high"]));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(terrain.textures.len(), 5);

        let path = |key: &str| terrain.textures[key].textures.first().map(|t| t.path.as_str());
        assert_eq!(path("stone"), Some("textures/blocks/stone_new"));
        assert_eq!(path("dirt"), Some("textures/blocks/dirt"));

        let grass = terrain.textures["grass_side"].textures.as_slice();
        assert_eq!(grass.len(), 2);
        assert_eq!(grass[0].overlay_color.as_deref(), Some("#79c05a"));
        assert_eq!(grass[1].path, "textures/blocks/grass_side_snowed");
        assert_eq!(
            terrain.textures["grass_carried"].textures.first().and_then(|t| t.tint_color.as_deref()),
            Some("#79c05a")
        );
        assert!(terrain.textures["flowing_water"].quad);
        assert!(!terrain.textures["dirt"].quad);
    }

    #[test]
    fn bad_files_and_entries_are_problems_not_failures() {
        let dir = TempDir::new("problems");
        dir.write("a/blocks.json", br#"{ "stone": { "textures": "stone" } }"#);
        // Not JSON at all: the whole file is skipped.
        dir.write("b/blocks.json", b"{ \"dirt\": ");
        // One bad entry: only it is skipped.
        dir.write(
            "c/blocks.json",
            br#"{ "sand": { "textures": 5 }, "gravel": { "textures": "gravel" } }"#,
        );
        dir.write("c/textures/terrain_texture.json", br#"{ "texture_data": [] }"#);
        let stack = stack_of(&dir, &["a", "b", "c"]);

        let (blocks, problems) = BlocksJson::load(&stack);
        let mut names: Vec<_> = blocks.blocks.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["gravel", "stone"]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(problems[0].pack, "b");
        assert_eq!(problems[1].pack, "c");
        assert!(problems[1].file.contains("sand"));

        let (terrain, problems) = TerrainTextures::load(&stack);
        assert!(terrain.textures.is_empty());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("isn't an object"));
    }
}
