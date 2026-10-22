//! What Bedrock block ids mean: the palette names each one, and the
//! appearance table says how it looks.

pub mod appearance;
pub mod palette;
pub mod palette_source;

use std::time::Instant;

use tracing::{debug, info, warn};

use self::{appearance::Built, palette::Palette};
use crate::install::Vanilla;

/// Loads the Bedrock block palette and checks that it names every Bedrock id
/// Pumpkin can send, logging the counts and how long it took.
///
/// # Errors
/// If the palette can't be read.
pub fn load_palette() -> Result<Palette, String> {
    let start = Instant::now();
    let palette =
        palette_source::load().map_err(|err| format!("the Bedrock block palette is damaged: {err}"))?;
    let took = start.elapsed();
    info!(
        "Block palette: {} Bedrock states, {} block names; parsed in {:.0} ms ({} build)",
        palette.len(),
        palette.names().len(),
        took.as_secs_f64() * 1000.0,
        if cfg!(debug_assertions) { "debug" } else { "release" },
    );
    let coverage = palette.coverage();
    if coverage.missing.is_empty() {
        info!(
            "Block palette: names all {} Bedrock ids Pumpkin can send",
            coverage.pumpkin_ids
        );
    } else {
        warn!(
            "Block palette: {} of the {} Bedrock ids Pumpkin can send have no entry, and will \
             draw as missing: {:#010x?}",
            coverage.missing.len(),
            coverage.pumpkin_ids,
            coverage.missing,
        );
    }
    Ok(palette)
}

/// Works out how every block looks and loads the textures that takes,
/// logging what was found and what's missing.
#[must_use]
pub fn build_appearance(palette: &Palette, vanilla: &Vanilla) -> Built {
    let start = Instant::now();
    let built = appearance::build(palette, &vanilla.blocks, &vanilla.terrain, &vanilla.stack);
    let report = &built.report;
    info!(
        "Block looks: {} of {} block names drawn with their textures, {} invisible; {} have no \
         blocks.json entry (Education Edition and engine blocks), and Pumpkin can send {} of them",
        report.textured,
        report.names,
        report.invisible,
        report.no_entry.len(),
        report.no_entry_sent.len(),
    );
    debug!("No blocks.json entry: {:?}", report.no_entry);
    if !report.no_entry_sent.is_empty() {
        warn!(
            "Pumpkin can send these blocks, which have no blocks.json entry and draw the \
             missing texture: {:?}",
            report.no_entry_sent
        );
    }
    for (name, why) in &report.unresolved {
        info!("Missing texture: {name}, because {why}");
    }
    for (path, why) in &report.failed_textures {
        warn!("Texture {path} didn't load: {why}");
    }
    info!(
        "Stand-in cubes: {} block names aren't full cubes in Java but are drawn as one: {}",
        report.stand_ins.len(),
        report.stand_ins.iter().map(String::as_str).collect::<Vec<_>>().join(", "),
    );
    let textures = &built.textures;
    info!(
        "Texture array: {} layers of {}×{} with {} mip levels, {:.1} MB; textures loaded in \
         {:.0} ms, everything in {:.0} ms",
        textures.layers,
        textures.size,
        textures.size,
        textures.mips.len(),
        textures.bytes() as f64 / 1e6,
        built.texture_seconds * 1000.0,
        start.elapsed().as_secs_f64() * 1000.0,
    );
    built
}
