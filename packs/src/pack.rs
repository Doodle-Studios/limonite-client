//! One pack: a folder whose files are loose, or packed into `__brarchive`
//! archives, one archive per folder.
//!
//! A path inside a pack always uses `/`, as pack JSON writes it, e.g.
//! `textures/blocks/stone.png`. It resolves to the loose file first, then to
//! the archive for its folder, `__brarchive/textures/blocks.brarchive`. Files
//! in the pack's root (`manifest.json`, `blocks.json`) are always loose.

use std::{
    collections::HashMap,
    fmt, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use crate::{
    brarchive::{Archive, ArchiveError},
    json,
    manifest::Manifest,
};

/// The folder that holds a pack's archives.
const ARCHIVE_DIR: &str = "__brarchive";
const MANIFEST: &str = "manifest.json";

/// Why a file couldn't be read from a pack. A file that simply isn't there
/// isn't an error: reads return `None` for it.
#[derive(Clone, Debug)]
pub enum PackError {
    /// The path isn't relative and made of plain names: it has an empty part,
    /// `.` or `..`, a backslash or a colon. Paths come from pack files, and
    /// this keeps them inside the pack.
    BadPath(String),
    /// Reading `file` failed for a reason other than its absence.
    Io { file: PathBuf, error: Arc<io::Error> },
    /// The archive `file` is damaged.
    Archive { file: PathBuf, error: ArchiveError },
    /// `file` isn't valid JSON of the expected shape.
    Json { file: PathBuf, error: String },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPath(path) => write!(f, "`{path}` isn't a plain path inside a pack"),
            Self::Io { file, error } => write!(f, "can't read {}: {error}", file.display()),
            Self::Archive { file, error } => write!(f, "{}: {error}", file.display()),
            Self::Json { file, error } => write!(f, "{}: {error}", file.display()),
        }
    }
}

impl std::error::Error for PackError {}

/// What a folder's archive turned out to be, kept so each archive is read
/// and checked once. `Ok(None)`: the folder has no archive.
type CachedArchive = Result<Option<Arc<Archive>>, PackError>;

/// A pack in a folder.
pub struct Pack {
    dir: PathBuf,
    name: String,
    /// Archives by folder (`textures/blocks`). A `Mutex` rather than a
    /// `RefCell`, so a `Pack` can be shared between threads later.
    archives: Mutex<HashMap<String, CachedArchive>>,
}

impl Pack {
    /// A pack in `dir`. Nothing is read until a file is asked for.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        let name = dir
            .file_name()
            .map_or_else(|| dir.display().to_string(), |name| name.to_string_lossy().into_owned());
        Self {
            dir,
            name,
            archives: Mutex::new(HashMap::new()),
        }
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder's name, e.g. `vanilla_1.21.90`, for logs.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The pack's manifest, or `None` if it has none: then it isn't a pack.
    ///
    /// # Errors
    /// If the manifest can't be read or parsed.
    pub fn manifest(&self) -> Result<Option<Manifest>, PackError> {
        let Some(bytes) = self.read(MANIFEST)? else {
            return Ok(None);
        };
        json::parse(&bytes).map(Some).map_err(|err| PackError::Json {
            file: self.dir.join(MANIFEST),
            error: err.to_string(),
        })
    }

    /// The file at `path`: loose if there's a loose copy, otherwise from the
    /// archive for its folder. `None` if the pack doesn't have it.
    ///
    /// # Errors
    /// A bad path, a read error, or a damaged archive for the folder.
    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, PackError> {
        let parts = plain_parts(path)?;
        let loose = parts.iter().fold(self.dir.clone(), |dir, part| dir.join(part));
        match std::fs::read(&loose) {
            Ok(bytes) => return Ok(Some(bytes)),
            // A folder with the file's name isn't the file.
            Err(err) if err.kind() == io::ErrorKind::NotFound || loose.is_dir() => {}
            Err(err) => {
                return Err(PackError::Io {
                    file: loose,
                    error: Arc::new(err),
                });
            }
        }
        let Some((folder, file)) = path.rsplit_once('/') else {
            return Ok(None);
        };
        let archive = self.archive(folder)?;
        Ok(archive.and_then(|archive| archive.get(file).map(<[u8]>::to_vec)))
    }

    /// The archive for `folder`, opened and checked on first use.
    fn archive(&self, folder: &str) -> CachedArchive {
        // A poisoned lock only means another thread panicked mid-insert; the
        // map itself is still whole.
        let mut archives = self.archives.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(cached) = archives.get(folder) {
            return cached.clone();
        }
        let opened = self.open_archive(folder);
        archives.insert(folder.to_string(), opened.clone());
        opened
    }

    fn open_archive(&self, folder: &str) -> CachedArchive {
        // `textures/blocks` -> `<pack>/__brarchive/textures/blocks.brarchive`.
        let mut file = self.dir.join(ARCHIVE_DIR);
        for part in folder.split('/') {
            file.push(part);
        }
        file.as_mut_os_string().push(".brarchive");
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(PackError::Io {
                    file,
                    error: Arc::new(err),
                });
            }
        };
        match Archive::open(bytes) {
            Ok(archive) => Ok(Some(Arc::new(archive))),
            Err(error) => Err(PackError::Archive { file, error }),
        }
    }
}

/// Splits a pack path at `/`, checking every part is a plain name.
fn plain_parts(path: &str) -> Result<Vec<&str>, PackError> {
    let parts: Vec<&str> = path.split('/').collect();
    let plain = |part: &&str| {
        !part.is_empty() && *part != "." && *part != ".." && !part.contains(['\\', ':'])
    };
    if parts.iter().all(plain) {
        Ok(parts)
    } else {
        Err(PackError::BadPath(path.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, archive};

    #[test]
    fn loose_files_beat_archived_ones() {
        let dir = TempDir::new("loose-first");
        dir.write("textures/blocks/stone.png", b"loose stone");
        dir.write(
            "__brarchive/textures/blocks.brarchive",
            &archive(&[("stone.png", b"archived stone"), ("dirt.png", b"archived dirt")]),
        );
        let pack = Pack::new(dir.path().to_path_buf());
        assert_eq!(pack.read("textures/blocks/stone.png").unwrap(), Some(b"loose stone".to_vec()));
        assert_eq!(pack.read("textures/blocks/dirt.png").unwrap(), Some(b"archived dirt".to_vec()));
        assert_eq!(pack.read("textures/blocks/sand.png").unwrap(), None);
    }

    #[test]
    fn subfolders_have_their_own_archives() {
        let dir = TempDir::new("subfolders");
        dir.write("__brarchive/textures.brarchive", &archive(&[("list.json", b"[]")]));
        dir.write(
            "__brarchive/textures/blocks/deepslate.brarchive",
            &archive(&[("deepslate_top.png", b"top")]),
        );
        let pack = Pack::new(dir.path().to_path_buf());
        assert_eq!(pack.read("textures/list.json").unwrap(), Some(b"[]".to_vec()));
        assert_eq!(
            pack.read("textures/blocks/deepslate/deepslate_top.png").unwrap(),
            Some(b"top".to_vec())
        );
        // Not in the parent folder's archive, and the folder itself isn't a file.
        assert_eq!(pack.read("textures/deepslate_top.png").unwrap(), None);
        assert_eq!(pack.read("textures/blocks").unwrap(), None);
    }

    #[test]
    fn root_files_are_only_loose() {
        let dir = TempDir::new("root");
        dir.write("blocks.json", b"{}");
        let pack = Pack::new(dir.path().to_path_buf());
        assert_eq!(pack.read("blocks.json").unwrap(), Some(b"{}".to_vec()));
        assert_eq!(pack.read("sounds.json").unwrap(), None);
        assert_eq!(pack.manifest().unwrap().map(|m| m.header.name), None);
    }

    #[test]
    fn zero_size_entries_are_absent() {
        let dir = TempDir::new("zero-size");
        dir.write("__brarchive/sounds.brarchive", &archive(&[("music.fsb", b"")]));
        let pack = Pack::new(dir.path().to_path_buf());
        assert_eq!(pack.read("sounds/music.fsb").unwrap(), None);
    }

    #[test]
    fn a_damaged_archive_is_an_error_every_time() {
        let dir = TempDir::new("damaged");
        let mut bytes = archive(&[("stone.png", b"stone")]);
        bytes.pop();
        dir.write("__brarchive/textures/blocks.brarchive", &bytes);
        let pack = Pack::new(dir.path().to_path_buf());
        for _ in 0..2 {
            let err = pack.read("textures/blocks/stone.png").unwrap_err();
            assert!(
                matches!(&err, PackError::Archive { file, .. } if file.ends_with("blocks.brarchive")),
                "{err}"
            );
        }
        // Other folders are unaffected.
        assert_eq!(pack.read("textures/items/apple.png").unwrap(), None);
    }

    #[test]
    fn paths_must_stay_inside_the_pack() {
        let dir = TempDir::new("paths");
        let pack = Pack::new(dir.path().to_path_buf());
        for path in ["", "/blocks.json", "../secret", "textures/../../x", "a//b", "a\\b", "C:/x", "a/./b", "a/"] {
            assert!(matches!(pack.read(path), Err(PackError::BadPath(_))), "{path:?} was accepted");
        }
    }

    #[test]
    fn reads_the_manifest() {
        let dir = TempDir::new("manifest");
        dir.write(
            "manifest.json",
            br#"{"header":{"name":"Test","uuid":"abc","version":[1,2,3]}}"#,
        );
        let pack = Pack::new(dir.path().to_path_buf());
        let manifest = pack.manifest().unwrap().unwrap();
        assert_eq!(manifest.header.name, "Test");
        assert_eq!(pack.name(), dir.path().file_name().unwrap().to_str().unwrap());

        dir.write("manifest.json", b"{ not json");
        assert!(matches!(pack.manifest(), Err(PackError::Json { .. })));
    }
}
