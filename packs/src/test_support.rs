//! Synthetic packs for the tests. No test reads a Mojang file, except the
//! real-install test, which reads at runtime and writes nothing.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use crate::brarchive::{ENTRY_LEN, MAGIC, VERSION};

/// A fresh folder under the system's temp folder, deleted when dropped.
///
/// The name holds the process id and a counter, because tests run in
/// parallel threads and test binaries can run side by side.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("limonite-packs-{}-{n}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("can create a temp folder");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `bytes` at `path` (with `/`), creating its folders.
    pub fn write(&self, path: &str, bytes: &[u8]) {
        let file = path.split('/').fold(self.0.clone(), |dir, part| dir.join(part));
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("can create folders");
        }
        std::fs::write(&file, bytes).expect("can write a test file");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A valid archive of `files`, laid out as the real ones are: the data in
/// index order, and the archive ending where the last file does. Zero-size
/// files get offset 0, as in sound archives.
pub fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    let mut data = Vec::new();
    for (name, bytes) in files {
        let mut entry = [0u8; ENTRY_LEN];
        entry[0] = name.len() as u8;
        entry[1..=name.len()].copy_from_slice(name.as_bytes());
        let offset = if bytes.is_empty() { 0 } else { data.len() as u32 };
        entry[248..252].copy_from_slice(&offset.to_le_bytes());
        entry[252..256].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry);
        data.extend_from_slice(bytes);
    }
    out.extend_from_slice(&data);
    out
}

/// A `manifest.json` with the given UUID and version.
pub fn manifest(uuid: &str, version: [u32; 3]) -> Vec<u8> {
    let [major, minor, patch] = version;
    format!(
        r#"{{"format_version":2,"header":{{"name":"test","uuid":"{uuid}",
        "version":[{major},{minor},{patch}],"min_engine_version":[1,13,0]}},"modules":[]}}"#
    )
    .into_bytes()
}
