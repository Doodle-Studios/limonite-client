//! Checks against a real Bedrock install, read at runtime. It writes nothing,
//! and nothing from the install is ever copied into the repo.
//!
//! `#[ignore]`d, because CI and fresh clones have no install. Run it with the
//! folder that holds `data/resource_packs`:
//!
//! ```text
//! LIMONITE_BEDROCK="C:\XboxGames\Minecraft for Windows\Content" cargo test -p limonite-packs --test real_install -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use limonite_packs::{
    PackStack, Version,
    brarchive::Archive,
    registry::{BlocksJson, TerrainTextures},
};

/// The base game version the embedded server declares (Pumpkin's
/// `CURRENT_BEDROCK_MC_VERSION`). The client passes the real constant.
const BASE_GAME_VERSION: Version = Version([1, 26, 51]);

fn resource_packs() -> PathBuf {
    let install = std::env::var_os("LIMONITE_BEDROCK")
        .expect("set LIMONITE_BEDROCK to the folder that holds data/resource_packs");
    Path::new(&install).join("data").join("resource_packs")
}

/// Every `.brarchive` file under `dir`.
fn archives_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            archives_under(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "brarchive") {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs a Bedrock install; set LIMONITE_BEDROCK"]
fn the_vanilla_stack_loads_without_problems() {
    let vanilla = PackStack::vanilla(&resource_packs(), BASE_GAME_VERSION).unwrap();
    let packs = vanilla.stack.packs();
    println!(
        "{} packs, {} ({}) to {} ({}); left out above {BASE_GAME_VERSION}: {:?}",
        packs.len(),
        packs[0].name(),
        vanilla.versions[0],
        packs[packs.len() - 1].name(),
        vanilla.versions[packs.len() - 1],
        vanilla.above_base,
    );
    assert!(vanilla.problems.is_empty(), "{:?}", vanilla.problems);
    assert_eq!(packs[0].name(), "vanilla");

    let (blocks, problems) = BlocksJson::load(&vanilla.stack);
    assert!(problems.is_empty(), "{problems:?}");
    let (terrain, problems) = TerrainTextures::load(&vanilla.stack);
    assert!(problems.is_empty(), "{problems:?}");
    println!(
        "blocks.json: {} blocks; terrain_texture.json: {} texture keys",
        blocks.blocks.len(),
        terrain.textures.len()
    );
    assert!(blocks.blocks.contains_key("grass"));
    assert!(terrain.textures.contains_key("grass_side"));
}

#[test]
#[ignore = "needs a Bedrock install; set LIMONITE_BEDROCK"]
fn every_archive_in_the_stack_opens_and_cut_short_fails() {
    let vanilla = PackStack::vanilla(&resource_packs(), BASE_GAME_VERSION).unwrap();
    let mut files = Vec::new();
    for pack in vanilla.stack.packs() {
        archives_under(&pack.dir().join("__brarchive"), &mut files);
    }
    let mut smallest: Option<(PathBuf, Vec<u8>)> = None;
    let mut entries = 0;
    for file in &files {
        let bytes = std::fs::read(file).unwrap();
        let archive = Archive::open(bytes.clone())
            .unwrap_or_else(|err| panic!("{}: {err}", file.display()));
        entries += archive.len();
        if !archive.is_empty() && smallest.as_ref().is_none_or(|(_, s)| bytes.len() < s.len()) {
            smallest = Some((file.clone(), bytes));
        }
    }
    println!("{} archives open, {entries} files with data", files.len());

    // The owner's amendment: every proper prefix of a real archive is an
    // error, never a panic. The smallest archive with data keeps it quick.
    let (file, bytes) = smallest.expect("some archive holds data");
    println!("prefix test on {} ({} bytes)", file.display(), bytes.len());
    for cut in 0..bytes.len() {
        assert!(Archive::open(bytes[..cut].to_vec()).is_err(), "a {cut}-byte prefix opened");
    }
}
