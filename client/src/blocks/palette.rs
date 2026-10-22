//! The Bedrock block palette: every block state Bedrock knows, by network id.
//!
//! A Bedrock network id is a hash of one block state: its name and its
//! properties. So, unlike M3's [`crate::world::BlockTable`], which maps a
//! Bedrock id back to the lowest of the Java states that share it, the
//! palette is a true inverse: each id names exactly one Bedrock state.
//!
//! Only [`super::palette_source`] knows where the palette comes from.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use pumpkin_data::{BlockState as JavaState, BlockStateId};

/// A block state property's value. Bedrock stores flags as bytes, numbers as
/// ints, and everything else as strings.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StateValue {
    Bool(bool),
    Int(i32),
    String(Arc<str>),
}

/// One Bedrock block state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BedrockState {
    /// The block's name without `minecraft:`, e.g. `grass_block`.
    pub name: Arc<str>,
    /// The properties, sorted by name, e.g. `pillar_axis = "y"`.
    pub states: Box<[(Arc<str>, StateValue)]>,
}

#[cfg_attr(not(test), expect(dead_code, reason = "M4 Step 6's appearance table reads them"))]
impl BedrockState {
    /// A property's value. The properties are sorted, so this is a binary
    /// search.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&StateValue> {
        self.states
            .binary_search_by(|(name, _)| (**name).cmp(key))
            .ok()
            .map(|i| &self.states[i].1)
    }

    #[must_use]
    pub fn int(&self, key: &str) -> Option<i32> {
        match self.get(key)? {
            StateValue::Int(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            StateValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn string(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            StateValue::String(value) => Some(value),
            _ => None,
        }
    }
}

/// Every Bedrock block state, by network id.
pub struct Palette {
    states: HashMap<u32, BedrockState>,
}

/// How many of the Bedrock ids Pumpkin can send the palette names.
pub struct Coverage {
    /// Distinct Bedrock ids in Pumpkin's Java-to-Bedrock table.
    pub pumpkin_ids: usize,
    /// Those without a palette entry, sorted. Should be none.
    pub missing: Vec<u32>,
}

impl Palette {
    /// Only the source builds palettes.
    pub(super) const fn new(states: HashMap<u32, BedrockState>) -> Self {
        Self { states }
    }

    /// The state with this network id, or `None` if Bedrock has no such id.
    #[cfg_attr(not(test), expect(dead_code, reason = "M4 Step 6's appearance table reads it"))]
    #[must_use]
    pub fn state(&self, id: u32) -> Option<&BedrockState> {
        self.states.get(&id)
    }

    /// Every (network id, state), in no particular order.
    #[expect(dead_code, reason = "M4 Step 6's appearance table reads it")]
    pub fn iter(&self) -> impl Iterator<Item = (u32, &BedrockState)> {
        self.states.iter().map(|(&id, state)| (id, state))
    }

    /// How many states there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// The distinct block names, sorted.
    #[must_use]
    pub fn names(&self) -> BTreeSet<&str> {
        self.states.values().map(|state| &*state.name).collect()
    }

    /// Checks the palette against every Bedrock id Pumpkin can send: those
    /// in its Java-to-Bedrock table, `STATE_ID_TO_BEDROCK`, which is what
    /// [`JavaState::to_be_network_id`] reads.
    #[must_use]
    pub fn coverage(&self) -> Coverage {
        let pumpkin_ids: HashSet<u32> = (0..BlockStateId::COUNT)
            .filter_map(BlockStateId::new)
            .map(JavaState::to_be_network_id)
            .collect();
        let mut missing: Vec<u32> = pumpkin_ids
            .iter()
            .copied()
            .filter(|id| !self.states.contains_key(id))
            .collect();
        missing.sort_unstable();
        Coverage {
            pumpkin_ids: pumpkin_ids.len(),
            missing,
        }
    }
}
