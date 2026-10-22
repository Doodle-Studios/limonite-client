//! Where the Bedrock block palette comes from. This is the only module that
//! knows (owner, at M4's approval).
//!
//! Today it's the fork's `assets/bedrock/block_states.nbt`, compiled into the
//! binary and parsed once at startup, the way Pumpkin's codegen reads it
//! (`tools/pumpkin-codegen/src/block.rs`). Codegen copies each entry's
//! `network_id` verbatim into `STATE_ID_TO_BEDROCK`, so these are exactly the
//! ids in our chunks (m4-plan Finding 7).
//!
//! Everything else sees only [`Palette`]. So the source can change later (a
//! file read at runtime, a generated table, data from a server) without
//! touching the appearance table or the renderer.
//!
//! The file is CloudburstMC's 26.50 dump, mined from Bedrock Dedicated Server
//! (see `pumpkin/assets/bedrock/README.md`). It's names and states, not
//! assets, but it's Mojang-derived, so PLAN.md's Risks list it with the
//! datapack: fine for dev builds, to be decided before any release.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt,
    io::{Cursor, ErrorKind},
    sync::Arc,
};

use pumpkin_nbt::{
    Error as NbtError, Nbt,
    deserializer::{NbtDataSource, NbtReadHelperBedrock},
    tag::NbtTag,
};

use super::palette::{BedrockState, Palette, StateValue};

/// Every Bedrock block state as network NBT compounds, back to back.
static BLOCK_STATES: &[u8] = include_bytes!("../../../pumpkin/assets/bedrock/block_states.nbt");

/// Why the palette couldn't be read. `at` is the byte offset of the entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteError {
    /// The entry isn't valid Bedrock network NBT.
    Nbt { at: u64, error: String },
    /// The data ends inside the entry.
    Truncated { at: u64 },
    /// The entry has no `field`, or it has the wrong type.
    MissingField { at: u64, field: &'static str },
    /// A property whose tag isn't a byte, an int or a string.
    StateType { at: u64, key: String, tag: u8 },
    /// Two entries have the same network id, so it wouldn't name one state.
    DuplicateId { id: u32, first: String, second: String },
}

impl fmt::Display for PaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nbt { at, error } => write!(f, "entry at byte {at}: bad NBT: {error}"),
            Self::Truncated { at } => write!(f, "entry at byte {at}: the data ends inside it"),
            Self::MissingField { at, field } => write!(f, "entry at byte {at}: no `{field}`"),
            Self::StateType { at, key, tag } => {
                write!(f, "entry at byte {at}: property `{key}` has NBT tag type {tag}")
            }
            Self::DuplicateId { id, first, second } => {
                write!(f, "network id {id:#010x} is both `{first}` and `{second}`")
            }
        }
    }
}

/// Loads the palette.
///
/// # Errors
/// If the embedded file is damaged. It's checked in at a fork commit, so a
/// test catches that long before a user could.
pub fn load() -> Result<Palette, PaletteError> {
    parse(BLOCK_STATES)
}

/// Parses network NBT compounds, back to back, each with a `name`, a
/// `states` compound and a `network_id`. Other fields (`name_hash`,
/// `version`) are ignored.
fn parse(bytes: &[u8]) -> Result<Palette, PaletteError> {
    let mut strings = Interner::default();
    let mut states = HashMap::new();
    let mut source = EndGuard {
        cursor: Cursor::new(bytes),
        read_past_end: false,
    };
    while source.cursor.position() < bytes.len() as u64 {
        let at = source.cursor.position();
        let nbt = Nbt::read(&mut NbtReadHelperBedrock::new(&mut source))
            .map_err(|err| PaletteError::Nbt { at, error: err.to_string() })?;
        if source.read_past_end {
            return Err(PaletteError::Truncated { at });
        }

        // Taking the compound apart by value, rather than cloning the parts
        // out of it.
        let (mut name, mut network_id, mut properties) = (None, None, None);
        for (key, tag) in nbt.root_tag {
            match (&*key, tag) {
                ("name", NbtTag::String(value)) => name = Some(value),
                ("network_id", NbtTag::Int(value)) => network_id = Some(value.cast_unsigned()),
                ("states", NbtTag::Compound(value)) => properties = Some(value),
                _ => {}
            }
        }
        let missing = |field| PaletteError::MissingField { at, field };
        let name = name.ok_or_else(|| missing("name"))?;
        let network_id = network_id.ok_or_else(|| missing("network_id"))?;
        let properties = properties.ok_or_else(|| missing("states"))?;

        let mut props = Vec::new();
        for (key, tag) in properties {
            let value = match tag {
                NbtTag::Byte(value) => StateValue::Bool(value != 0),
                NbtTag::Int(value) => StateValue::Int(value),
                NbtTag::String(value) => StateValue::String(strings.get(&value)),
                other => {
                    return Err(PaletteError::StateType {
                        at,
                        key: key.into(),
                        tag: other.get_type_id(),
                    });
                }
            };
            props.push((strings.get(&key), value));
        }
        // NBT compounds keep their tags in a `HashMap`, whose order changes
        // from run to run. Sorted, a state compares and prints the same way
        // every time, and lookups can binary search.
        props.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

        let state = BedrockState {
            name: strings.get(name.strip_prefix("minecraft:").unwrap_or(&name)),
            states: props.into_boxed_slice(),
        };
        match states.entry(network_id) {
            Entry::Vacant(slot) => {
                slot.insert(state);
            }
            Entry::Occupied(slot) => {
                return Err(PaletteError::DuplicateId {
                    id: network_id,
                    first: slot.get().name.to_string(),
                    second: state.name.to_string(),
                });
            }
        }
    }
    Ok(Palette::new(states))
}

/// A byte source that remembers whether anything tried to read past its end.
///
/// `pumpkin-nbt` ends a compound quietly when the data runs out between two
/// of its tags, as if its End tag had been there. So an entry cut short at a
/// tag boundary would parse, with its last tags missing. A compound that
/// really ends never reads past the end, so this tells the two apart.
struct EndGuard<'a> {
    cursor: Cursor<&'a [u8]>,
    read_past_end: bool,
}

impl EndGuard<'_> {
    /// Passes `result` on, noting an attempt to read past the end.
    fn note<T>(&mut self, result: Result<T, NbtError>) -> Result<T, NbtError> {
        if let Err(NbtError::Incomplete(err)) = &result
            && err.kind() == ErrorKind::UnexpectedEof
        {
            self.read_past_end = true;
        }
        result
    }
}

/// Each method hands over to the cursor's own implementation, then notes
/// what happened. The trait's two other methods have default bodies that
/// call `read_u8`, so they're covered too.
impl<'a> NbtDataSource<'a> for EndGuard<'a> {
    fn read_u8(&mut self) -> Result<u8, NbtError> {
        let result = self.cursor.read_u8();
        self.note(result)
    }

    fn read_bytes(&mut self, buf: &mut [u8]) -> Result<(), NbtError> {
        let result = self.cursor.read_bytes(buf);
        self.note(result)
    }

    fn seek_relative(&mut self, offset: i64) -> Result<(), NbtError> {
        let result = self.cursor.seek_relative(offset);
        self.note(result)
    }

    fn read_string(&mut self, len: usize) -> Result<Cow<'a, str>, NbtError> {
        let result = self.cursor.read_string(len);
        self.note(result)
    }

    fn read_byte_array(&mut self, len: usize) -> Result<Cow<'a, [i8]>, NbtError> {
        let result = self.cursor.read_byte_array(len);
        self.note(result)
    }
}

/// Hands out one shared copy of each distinct string. There are 22,091
/// states but only 1,477 block names, and far fewer property names and
/// values, so most states share their strings.
#[derive(Default)]
struct Interner(HashSet<Arc<str>>);

impl Interner {
    fn get(&mut self, text: &str) -> Arc<str> {
        // `Arc<str>` borrows as `str`, so the set can be searched with one.
        if let Some(shared) = self.0.get(text) {
            return shared.clone();
        }
        let shared: Arc<str> = Arc::from(text);
        self.0.insert(shared.clone());
        shared
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::{Block, BlockState};
    use pumpkin_nbt::compound::NbtCompound;

    use super::*;

    fn palette() -> Palette {
        load().expect("the embedded palette parses")
    }

    fn named(palette: &Palette, java: pumpkin_data::BlockStateId) -> &BedrockState {
        let id = BlockState::to_be_network_id(java);
        palette.state(id).unwrap_or_else(|| panic!("no palette entry for {id:#010x}"))
    }

    #[test]
    fn names_known_blocks() {
        let palette = palette();
        assert_eq!(&*named(&palette, Block::GRASS_BLOCK.default_state.id).name, "grass_block");
        assert_eq!(&*named(&palette, Block::AIR.default_state.id).name, "air");

        let log = named(&palette, Block::OAK_LOG.default_state.id);
        assert_eq!(&*log.name, "oak_log");
        assert_eq!(log.string("pillar_axis"), Some("y"));

        let water = named(&palette, Block::WATER.default_state.id);
        assert_eq!(&*water.name, "water");
        assert_eq!(water.int("liquid_depth"), Some(0));
    }

    #[test]
    fn every_id_pumpkin_can_send_is_named() {
        let coverage = palette().coverage();
        assert!(coverage.pumpkin_ids > 15_000, "{} ids", coverage.pumpkin_ids);
        assert!(coverage.missing.is_empty(), "missing: {:x?}", coverage.missing);
    }

    fn entry(name: &str, network_id: Option<i32>, states: NbtCompound) -> Vec<u8> {
        let mut compound = NbtCompound::new();
        compound.put_string("name", format!("minecraft:{name}"));
        compound.put_compound("states", states);
        if let Some(id) = network_id {
            compound.put_int("network_id", id);
        }
        compound.put_int("version", 18_168_865);
        Nbt::new(String::new(), compound).write_bedrock().to_vec()
    }

    #[test]
    fn parses_names_and_typed_properties() {
        let mut states = NbtCompound::new();
        states.put_string("wood_type", "birch".to_string());
        states.put_byte("upper_block_bit", 1);
        states.put_int("growth", 7);
        let mut bytes = entry("test_block", Some(-5), states);
        bytes.extend(entry("other", Some(9), NbtCompound::new()));

        let palette = parse(&bytes).unwrap();
        assert_eq!(palette.len(), 2);
        let state = palette.state((-5i32).cast_unsigned()).unwrap();
        assert_eq!(&*state.name, "test_block");
        assert_eq!(state.string("wood_type"), Some("birch"));
        assert_eq!(state.flag("upper_block_bit"), Some(true));
        assert_eq!(state.int("growth"), Some(7));
        assert_eq!(state.int("wood_type"), None, "the wrong type is no value");
        let keys: Vec<&str> = state.states.iter().map(|(key, _)| &**key).collect();
        assert_eq!(keys, ["growth", "upper_block_bit", "wood_type"], "sorted");
        assert_eq!(palette.names().into_iter().collect::<Vec<_>>(), ["other", "test_block"]);
    }

    #[test]
    fn bad_entries_are_typed_errors() {
        let first = entry("a", Some(1), NbtCompound::new());
        let second_at = first.len() as u64;

        let mut bytes = first.clone();
        bytes.extend(entry("b", None, NbtCompound::new()));
        assert_eq!(
            parse(&bytes).err(),
            Some(PaletteError::MissingField {
                at: second_at,
                field: "network_id"
            })
        );

        let mut bytes = first.clone();
        bytes.extend(entry("b", Some(1), NbtCompound::new()));
        assert_eq!(
            parse(&bytes).err(),
            Some(PaletteError::DuplicateId {
                id: 1,
                first: "a".to_string(),
                second: "b".to_string()
            })
        );

        let mut states = NbtCompound::new();
        states.put_float("odd", 0.5);
        assert!(matches!(
            parse(&entry("c", Some(2), states)),
            Err(PaletteError::StateType { .. })
        ));

        // Every cut through an entry is an error, never a panic. Cuts between
        // two tags are the ones `EndGuard` catches: without it they parse.
        let mut at_tag_boundaries = 0;
        for cut in 1..first.len() {
            match parse(&first[..cut]) {
                Ok(_) => panic!("cut at {cut} parsed"),
                Err(PaletteError::Truncated { at: 0 }) => at_tag_boundaries += 1,
                Err(_) => {}
            }
        }
        assert!(at_tag_boundaries > 0);
    }
}
