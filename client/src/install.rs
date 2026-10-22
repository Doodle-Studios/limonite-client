//! Finding the user's Bedrock install, and loading its vanilla packs.
//!
//! Vanilla assets only ever come from the user's own install, read at
//! runtime (CLAUDE.md; m4-plan Decision 15). Nothing is bundled to fall back
//! on, so a missing install is a clear error, raised before the server
//! starts.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Instant,
};

use limonite_packs::{
    PackStack, Problem, Version,
    registry::{BlocksJson, TerrainTextures},
};
use pumpkin_world::CURRENT_BEDROCK_MC_VERSION;
use tracing::{info, warn};

/// Where a GDK (Microsoft Store, 2025 and later) install keeps the game's
/// files, under a drive's root.
const GDK_DEFAULT: [&str; 3] = ["XboxGames", "Minecraft for Windows", "Content"];

/// A Bedrock install: the folder that holds `data/resource_packs`.
pub struct Install {
    pub dir: PathBuf,
    pub found_by: FoundBy,
}

#[derive(Clone, Copy, Debug)]
pub enum FoundBy {
    /// `--bedrock`.
    Flag,
    /// The GDK default, on this drive.
    GdkDefault(char),
}

impl fmt::Display for FoundBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flag => write!(f, "from --bedrock"),
            Self::GdkDefault(drive) => write!(f, "the GDK default on {drive}:"),
        }
    }
}

impl Install {
    #[must_use]
    pub fn resource_packs(&self) -> PathBuf {
        resource_packs(&self.dir)
    }
}

fn resource_packs(dir: &Path) -> PathBuf {
    dir.join("data").join("resource_packs")
}

/// Whether `dir` is an install: it has the bottom vanilla pack's manifest.
fn is_install(dir: &Path) -> bool {
    resource_packs(dir).join("vanilla").join("manifest.json").is_file()
}

/// Finds the install: the `--bedrock` folder if given, otherwise the GDK
/// default on each drive, A to Z.
///
/// # Errors
/// If `--bedrock` isn't an install, or no install is found.
pub fn find(flag: Option<&Path>) -> Result<Install, String> {
    if let Some(dir) = flag {
        return if is_install(dir) {
            Ok(Install {
                dir: dir.to_path_buf(),
                found_by: FoundBy::Flag,
            })
        } else {
            Err(format!(
                "--bedrock {}: not a Bedrock install, because it has no \
                 data\\resource_packs\\vanilla\\manifest.json",
                dir.display()
            ))
        };
    }
    // Drive letters only exist on Windows. Elsewhere the install has to be
    // named.
    if cfg!(windows) {
        for drive in 'A'..='Z' {
            let dir = GDK_DEFAULT
                .iter()
                .fold(PathBuf::from(format!("{drive}:\\")), |dir, part| dir.join(part));
            if is_install(&dir) {
                return Ok(Install {
                    dir,
                    found_by: FoundBy::GdkDefault(drive),
                });
            }
        }
    }
    Err(format!(
        "No Bedrock install found. Limonite reads the vanilla textures from your own \
         Minecraft Bedrock install and bundles none. It looked for {} on every drive; \
         pass --bedrock <folder> with the folder that holds data\\resource_packs",
        GDK_DEFAULT.join("\\")
    ))
}

/// The vanilla packs and their merged registries.
pub struct Vanilla {
    pub stack: PackStack,
    pub blocks: BlocksJson,
    pub terrain: TerrainTextures,
}

/// Builds the vanilla stack, capped at the server's base game version, and
/// merges its registries, logging what it found.
///
/// # Errors
/// If no vanilla pack can be found. A file that can't be read or parsed is
/// logged and skipped instead.
pub fn load_vanilla(install: &Install) -> Result<Vanilla, String> {
    let start = Instant::now();
    let base: Version = CURRENT_BEDROCK_MC_VERSION
        .parse()
        .map_err(|err| format!("Pumpkin's base game version: {err}"))?;
    let vanilla = PackStack::vanilla(&install.resource_packs(), base).map_err(|err| err.to_string())?;
    let (blocks, block_problems) = BlocksJson::load(&vanilla.stack);
    let (terrain, texture_problems) = TerrainTextures::load(&vanilla.stack);

    info!("Bedrock install: {} ({})", install.dir.display(), install.found_by);
    let packs = vanilla.stack.packs();
    let (first, last) = (&packs[0], &packs[packs.len() - 1]);
    let left_out: Vec<_> = vanilla.above_base.iter().map(|(name, _)| name.as_str()).collect();
    info!(
        "Vanilla packs: {}, {} ({}) to {} ({}); left out above the base game version {base}: {}",
        packs.len(),
        first.name(),
        vanilla.versions[0],
        last.name(),
        vanilla.versions[packs.len() - 1],
        if left_out.is_empty() { "none".to_string() } else { left_out.join(", ") },
    );
    let problems: Vec<&Problem> = vanilla
        .problems
        .iter()
        .chain(&block_problems)
        .chain(&texture_problems)
        .collect();
    for problem in &problems {
        warn!("Skipped in the vanilla packs: {problem}");
    }
    info!(
        "Vanilla registries: blocks.json {} blocks, terrain_texture.json {} texture keys; \
         {} problems; {:.0} ms",
        blocks.blocks.len(),
        terrain.textures.len(),
        problems.len(),
        start.elapsed().as_secs_f64() * 1000.0,
    );
    Ok(Vanilla {
        stack: vanilla.stack,
        blocks,
        terrain,
    })
}
