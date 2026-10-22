//! What Bedrock block ids mean: the palette names each one, and (from M4's
//! Step 6) the appearance table says how it looks.

pub mod palette;
pub mod palette_source;

use std::time::Instant;

use tracing::{info, warn};

use self::palette::Palette;

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
