//! The renderer. So far, its CPU half: the block texture array and the biome
//! tints.

pub mod mesher;
pub mod textures;
pub mod tint;

use limonite_packs::PackStack;
use tracing::{info, warn};

use self::tint::Tints;

/// Loads the biome tints, logging the colormaps, the climate table and the
/// water colour.
pub fn load_tints(stack: &PackStack) -> Tints {
    let (tints, report) = Tints::load(stack);
    let maps: Vec<String> = report
        .colormaps
        .iter()
        .map(|(path, size)| {
            let name = path.rsplit('/').next().unwrap_or(path);
            match size {
                Ok((width, height)) => format!("{name} {width}×{height}"),
                Err(err) => {
                    warn!("Colormap {path} didn't load, so its tint is white: {err}");
                    format!("{name} missing")
                }
            }
        })
        .collect();
    let water = match &report.water {
        Ok([r, g, b, a]) => format!("#{r:02X}{g:02X}{b:02X} at {:.0}% opacity", f32::from(*a) / 2.55),
        Err(err) => {
            warn!("No water colour, so water is white: {err}");
            "missing".to_string()
        }
    };
    info!(
        "Tints: {} Bedrock biome ids with a climate ({} Java biomes share an id with a lower one); \
         colormaps {}; water {water}",
        report.biomes,
        report.collisions,
        maps.join(", "),
    );
    tints
}
