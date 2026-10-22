//! Limonite's layered virtual filesystem over Bedrock resource packs
//! (PLAN.md, "Resource packs").
//!
//! - A [`Pack`] is a folder whose files are loose or inside `__brarchive`
//!   archives ([`brarchive`]).
//! - A [`PackStack`] orders packs bottom first. A lookup walks it top-down
//!   and returns the first hit, while registry JSON merges per key, the
//!   higher pack winning ([`registry`]).
//! - [`PackStack::vanilla`] builds the vanilla stack from a Bedrock install.
//!
//! This crate knows pack formats only: archives, manifests, Bedrock's JSON
//! and the registry schemas. It knows nothing about network ids or the GPU;
//! the client joins the two. Vanilla files are only ever read at runtime from
//! the user's install, never copied into the repo, so the tests build their
//! own synthetic packs.

pub mod brarchive;
pub mod json;
pub mod manifest;
pub mod pack;
pub mod registry;
pub mod stack;
#[cfg(test)]
mod test_support;

pub use manifest::Version;
pub use pack::{Pack, PackError};
pub use registry::Problem;
pub use stack::{ImageFormat, PackStack, StackError, TextureFile, VanillaStack};
