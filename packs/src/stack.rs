//! A stack of packs, bottom first: the layered filesystem of PLAN.md,
//! "Resource packs".
//!
//! A lookup walks the stack from the top and returns the first pack's copy,
//! so a higher pack overrides a lower one file by file. Registry JSON merges
//! instead; see [`crate::registry`].

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

use crate::{
    manifest::Version,
    pack::{Pack, PackError},
    registry::Problem,
};

/// The header UUID every vanilla resource pack shares: `vanilla`, then one
/// pack per game version that changed something, `vanilla_1.14` onwards.
pub const VANILLA_UUID: &str = "0575c61f-a5da-4b7f-9961-ffda2908861e";

/// A file found in the stack, and the index of the pack it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub pack: usize,
    pub bytes: Vec<u8>,
}

/// The image formats a texture path can resolve to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Tga,
}

impl ImageFormat {
    /// The extensions tried, in order, for a texture path without one.
    pub const ALL: [Self; 2] = [Self::Png, Self::Tga];

    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Tga => "tga",
        }
    }
}

/// A texture file found in the stack. TGA files have no magic number, so the
/// format they were found as is kept for the decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureFile {
    pub pack: usize,
    pub format: ImageFormat,
    pub bytes: Vec<u8>,
}

/// An ordered list of packs, bottom first.
pub struct PackStack {
    packs: Vec<Pack>,
}

impl PackStack {
    /// A stack of `packs`, bottom first.
    #[must_use]
    pub const fn new(packs: Vec<Pack>) -> Self {
        Self { packs }
    }

    /// The packs, bottom first.
    #[must_use]
    pub fn packs(&self) -> &[Pack] {
        &self.packs
    }

    /// The top pack's copy of `path`.
    ///
    /// # Errors
    /// The first read error on the way down. A damaged pack fails loudly
    /// instead of silently showing a lower pack's file.
    pub fn read(&self, path: &str) -> Result<Option<Found>, PackError> {
        for (pack, layer) in self.packs.iter().enumerate().rev() {
            if let Some(bytes) = layer.read(path)? {
                return Ok(Some(Found { pack, bytes }));
            }
        }
        Ok(None)
    }

    /// The top pack's copy of a texture. Texture paths have no extension, so
    /// each pack is tried with `.png`, then `.tga`, before the next one down.
    /// So a higher pack's TGA beats a lower pack's PNG.
    ///
    /// # Errors
    /// As [`PackStack::read`].
    pub fn read_texture(&self, path: &str) -> Result<Option<TextureFile>, PackError> {
        for (pack, layer) in self.packs.iter().enumerate().rev() {
            for format in ImageFormat::ALL {
                if let Some(bytes) = layer.read(&format!("{path}.{}", format.extension()))? {
                    return Ok(Some(TextureFile {
                        pack,
                        format,
                        bytes,
                    }));
                }
            }
        }
        Ok(None)
    }

    /// Every pack's copy of `path`, bottom first, for files that merge.
    pub fn layers<'a>(
        &'a self,
        path: &'a str,
    ) -> impl Iterator<Item = (&'a Pack, Result<Option<Vec<u8>>, PackError>)> + 'a {
        self.packs.iter().map(move |pack| (pack, pack.read(path)))
    }

    /// The vanilla stack in a Bedrock install's `resource_packs` folder.
    ///
    /// It holds every pack whose manifest has [`VANILLA_UUID`] and a version
    /// at or below `base_game_version`, the oldest at the bottom. The newer
    /// ones then override older files and add new ones, as in the game.
    /// Capping at the server's base game version keeps out packs made for a
    /// newer game than the server speaks.
    ///
    /// Folders without a manifest aren't packs and are passed over. A
    /// manifest that can't be read is a [`Problem`]: the folder might have
    /// been a vanilla pack.
    ///
    /// # Errors
    /// If the folder can't be listed, or holds no vanilla pack at or below
    /// the base game version.
    pub fn vanilla(resource_packs: &Path, base_game_version: Version) -> Result<VanillaStack, StackError> {
        let entries = std::fs::read_dir(resource_packs).map_err(|error| StackError::ReadDir {
            dir: resource_packs.to_path_buf(),
            error,
        })?;
        let mut found = Vec::new();
        let mut above_base = Vec::new();
        let mut problems = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    problems.push(Problem::new("", resource_packs.display(), error));
                    continue;
                }
            };
            // Folder packs only; zip and `.mcpack` packs come with M8.
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let pack = Pack::new(entry.path());
            match pack.manifest() {
                Ok(Some(manifest)) if manifest.header.uuid.eq_ignore_ascii_case(VANILLA_UUID) => {
                    let version = manifest.header.version;
                    if version <= base_game_version {
                        found.push((version, pack));
                    } else {
                        above_base.push((pack.name().to_string(), version));
                    }
                }
                Ok(_) => {}
                Err(error) => problems.push(Problem::new(pack.name(), "manifest.json", error)),
            }
        }
        if found.is_empty() {
            return Err(StackError::NoVanillaPacks {
                dir: resource_packs.to_path_buf(),
                base_game_version,
            });
        }
        // Ties are broken by name, so the order never depends on the order
        // the file system lists folders in.
        found.sort_by(|(a_version, a), (b_version, b)| {
            a_version.cmp(b_version).then_with(|| a.name().cmp(b.name()))
        });
        above_base.sort();
        let versions = found.iter().map(|(version, _)| *version).collect();
        Ok(VanillaStack {
            stack: Self::new(found.into_iter().map(|(_, pack)| pack).collect()),
            versions,
            above_base,
            problems,
        })
    }
}

/// The vanilla stack, and what building it passed over.
pub struct VanillaStack {
    pub stack: PackStack,
    /// Each pack's version, bottom first.
    pub versions: Vec<Version>,
    /// Vanilla packs left out for being newer than the base game version.
    pub above_base: Vec<(String, Version)>,
    /// Folders whose manifest couldn't be read.
    pub problems: Vec<Problem>,
}

#[derive(Debug)]
pub enum StackError {
    ReadDir { dir: PathBuf, error: io::Error },
    NoVanillaPacks { dir: PathBuf, base_game_version: Version },
}

impl fmt::Display for StackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadDir { dir, error } => write!(f, "can't list {}: {error}", dir.display()),
            Self::NoVanillaPacks {
                dir,
                base_game_version,
            } => write!(
                f,
                "{} holds no vanilla resource pack at or below version {base_game_version}",
                dir.display()
            ),
        }
    }
}

impl std::error::Error for StackError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, archive, manifest};

    fn stack_of(dir: &TempDir, names: &[&str]) -> PackStack {
        PackStack::new(names.iter().map(|name| Pack::new(dir.path().join(name))).collect())
    }

    #[test]
    fn the_higher_pack_wins() {
        let dir = TempDir::new("higher-wins");
        dir.write("low/textures/blocks/stone.png", b"low stone");
        dir.write("low/textures/blocks/dirt.png", b"low dirt");
        dir.write(
            "high/__brarchive/textures/blocks.brarchive",
            &archive(&[("stone.png", b"high stone")]),
        );
        let stack = stack_of(&dir, &["low", "high"]);
        let found = stack.read("textures/blocks/stone.png").unwrap().unwrap();
        assert_eq!((found.pack, found.bytes.as_slice()), (1, &b"high stone"[..]));
        let found = stack.read("textures/blocks/dirt.png").unwrap().unwrap();
        assert_eq!((found.pack, found.bytes.as_slice()), (0, &b"low dirt"[..]));
        assert_eq!(stack.read("textures/blocks/sand.png").unwrap(), None);
        let layers: Vec<_> = stack
            .layers("textures/blocks/stone.png")
            .map(|(pack, read)| (pack.name().to_string(), read.unwrap()))
            .collect();
        assert_eq!(
            layers,
            [
                ("low".to_string(), Some(b"low stone".to_vec())),
                ("high".to_string(), Some(b"high stone".to_vec())),
            ]
        );
    }

    #[test]
    fn textures_try_png_then_tga_in_each_pack() {
        let dir = TempDir::new("extensions");
        // Within one pack, PNG first.
        dir.write("low/textures/blocks/grass.png", b"low png");
        dir.write("low/textures/blocks/grass.tga", b"low tga");
        // A higher pack's TGA beats a lower pack's PNG.
        dir.write("low/textures/blocks/leaves.png", b"low png");
        dir.write("high/textures/blocks/leaves.tga", b"high tga");
        let stack = stack_of(&dir, &["low", "high"]);

        let grass = stack.read_texture("textures/blocks/grass").unwrap().unwrap();
        assert_eq!((grass.pack, grass.format), (0, ImageFormat::Png));
        let leaves = stack.read_texture("textures/blocks/leaves").unwrap().unwrap();
        assert_eq!((leaves.pack, leaves.format), (1, ImageFormat::Tga));
        assert_eq!(leaves.bytes, b"high tga");
        assert_eq!(stack.read_texture("textures/blocks/none").unwrap(), None);
    }

    #[test]
    fn builds_the_vanilla_stack_oldest_first_up_to_the_base_version() {
        let dir = TempDir::new("vanilla");
        // Written out of order on purpose.
        dir.write("vanilla_1.21.90/manifest.json", &manifest(VANILLA_UUID, [1, 21, 90]));
        dir.write("vanilla/manifest.json", &manifest(VANILLA_UUID, [0, 0, 1]));
        dir.write("vanilla_1.26.52/manifest.json", &manifest(VANILLA_UUID, [1, 26, 52]));
        dir.write("vanilla_1.14/manifest.json", &manifest(VANILLA_UUID, [1, 14, 0]));
        // Not vanilla: another UUID, and a folder with no manifest.
        dir.write("chemistry/manifest.json", &manifest("abc", [1, 0, 0]));
        dir.write("not_a_pack/readme.txt", b"hi");
        // A loose file beside the packs isn't a pack either.
        dir.write("contents.json", b"{}");

        let vanilla = PackStack::vanilla(dir.path(), Version([1, 26, 51])).unwrap();
        let names: Vec<_> = vanilla.stack.packs().iter().map(Pack::name).collect();
        assert_eq!(names, ["vanilla", "vanilla_1.14", "vanilla_1.21.90"]);
        assert_eq!(
            vanilla.versions,
            [Version([0, 0, 1]), Version([1, 14, 0]), Version([1, 21, 90])]
        );
        assert_eq!(
            vanilla.above_base,
            [("vanilla_1.26.52".to_string(), Version([1, 26, 52]))]
        );
        assert!(vanilla.problems.is_empty());

        // A broken manifest is a problem, not a failure.
        dir.write("broken/manifest.json", b"{ nope");
        let vanilla = PackStack::vanilla(dir.path(), Version([1, 26, 51])).unwrap();
        assert_eq!(vanilla.problems.len(), 1);
        assert_eq!(vanilla.problems[0].pack, "broken");

        assert!(matches!(
            PackStack::vanilla(dir.path(), Version([0, 0, 0])),
            Err(StackError::NoVanillaPacks { .. })
        ));
        assert!(matches!(
            PackStack::vanilla(&dir.path().join("missing"), Version([1, 26, 51])),
            Err(StackError::ReadDir { .. })
        ));
    }
}
