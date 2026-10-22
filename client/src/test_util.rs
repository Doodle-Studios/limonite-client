//! Helpers for tests that need packs on disk. They build synthetic packs:
//! no test reads a Mojang file (m4-plan Decision 15).

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use image::{ImageFormat, RgbaImage};

/// A fresh folder under the system's temp folder, deleted when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("limonite-client-{}-{n}-{label}", std::process::id()));
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

fn encode(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).expect("can encode a test image");
    bytes.into_inner()
}

pub fn png(image: &RgbaImage) -> Vec<u8> {
    encode(image, ImageFormat::Png)
}

pub fn tga(image: &RgbaImage) -> Vec<u8> {
    encode(image, ImageFormat::Tga)
}
