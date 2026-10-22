//! Biome tints (m4-plan Decision 8): the colours grass, leaves and water are
//! multiplied by.
//!
//! - **Grass and foliage** come from Bedrock's colormaps in the install,
//!   256×256 images indexed by climate. A biome's colour is the texel at its
//!   temperature and downfall, as both Java and Bedrock look it up.
//! - **The climate** comes from `pumpkin-data`: a Bedrock biome id leads to a
//!   Java biome through `be_network_id`, whose weather has the base
//!   temperature and the downfall.
//! - **Water** is one colour, the install's default surface colour and
//!   transparency from `biomes_client.json`.
//!
//! One colour per biome, with no blending between neighbours. Per-biome
//! overrides (swamps, badlands, ...) and water colours are deferred: the
//! install keys them by Bedrock biome name, and nothing we can read maps our
//! numeric ids to those names (Finding 9).

use image::RgbaImage;
use limonite_packs::{PackStack, json};
use pumpkin_data::biome::Biome;
use serde_json::Value;

use super::textures::read_image;

/// What a face is tinted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tint {
    #[default]
    None,
    /// Grass blocks, short and tall grass, ferns and sugar cane.
    Grass,
    /// Oak, jungle, acacia, dark oak and mangrove leaves, and vines.
    Foliage,
    /// Birch leaves.
    Birch,
    /// Spruce leaves.
    Evergreen,
    Water,
}

/// The colormaps, in the order [`Tints`] keeps their colours.
const COLORMAPS: [(Tint, &str); 4] = [
    (Tint::Grass, "textures/colormap/grass"),
    (Tint::Foliage, "textures/colormap/foliage"),
    (Tint::Birch, "textures/colormap/birch"),
    (Tint::Evergreen, "textures/colormap/evergreen"),
];

/// A climate: a Java biome's base temperature and downfall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climate {
    pub temperature: f32,
    pub downfall: f32,
}

/// The tint colours by Bedrock biome id.
#[cfg_attr(not(test), expect(dead_code, reason = "the window's meshes use it (M4 Step 10)"))]
pub struct Tints {
    /// For each Bedrock biome id 0 to 255: the grass, foliage, birch and
    /// evergreen colours. An id with no Java biome gets plains'.
    colors: Vec<[[u8; 3]; 4]>,
    /// RGBA: the colour, and the surface transparency as alpha.
    water: [u8; 4],
}

/// What loading found, for the startup log.
pub struct TintReport {
    /// Bedrock biome ids with a climate.
    pub biomes: usize,
    /// Java biomes whose Bedrock id a lower Java biome already had.
    pub collisions: usize,
    /// Each colormap's path, and its size or why it couldn't be loaded.
    pub colormaps: Vec<(&'static str, ColormapSize)>,
    pub water: Result<[u8; 4], String>,
}

/// A colormap's width and height, or why it couldn't be loaded.
pub type ColormapSize = Result<(u32, u32), String>;

impl Tints {
    /// Loads the colormaps and the water colour from the packs, and builds
    /// the climate table.
    #[must_use]
    pub fn load(stack: &PackStack) -> (Self, TintReport) {
        let (climates, collisions) = climates();
        let loaded = COLORMAPS.map(|(_, path)| (path, read_image(stack, path)));
        let report_maps = loaded
            .iter()
            .map(|(path, map)| (*path, map.as_ref().map(RgbaImage::dimensions).map_err(Clone::clone)))
            .collect();
        let maps: [Option<&RgbaImage>; 4] = loaded.each_ref().map(|(_, map)| map.as_ref().ok());
        let plains = Climate {
            temperature: Biome::PLAINS.weather.base_temperature(),
            downfall: Biome::PLAINS.weather.downfall(),
        };
        let colors = climates
            .iter()
            .map(|climate| {
                let climate = climate.unwrap_or(plains);
                // A missing colormap leaves the texture's own grey: wrong, but
                // obviously so, and the log says why.
                maps.map(|map| map.map_or([255; 3], |map| sample(map, climate)))
            })
            .collect();
        let water = water_color(stack);
        let tints = Self {
            colors,
            water: *water.as_ref().unwrap_or(&[255, 255, 255, 166]),
        };
        let report = TintReport {
            biomes: climates.iter().flatten().count(),
            collisions,
            colormaps: report_maps,
            water,
        };
        (tints, report)
    }

    /// One colour for every tint and biome, and a water colour: for tests
    /// that don't need packs.
    #[cfg(test)]
    #[must_use]
    pub fn uniform(color: [u8; 3], water: [u8; 4]) -> Self {
        Self {
            colors: vec![[color; 4]; 256],
            water,
        }
    }

    /// The RGBA colour a face with `tint` gets in biome `biome`. Untinted
    /// faces get white, which leaves the texture as it is.
    #[cfg_attr(not(test), expect(dead_code, reason = "the window's meshes use it (M4 Step 10)"))]
    #[must_use]
    pub fn color(&self, biome: u32, tint: Tint) -> [u8; 4] {
        let colors = usize::try_from(biome)
            .ok()
            .and_then(|biome| self.colors.get(biome))
            // Plains is Bedrock id 1.
            .unwrap_or(&self.colors[1]);
        let rgb = |[r, g, b]: [u8; 3]| [r, g, b, 255];
        match tint {
            Tint::None => [255; 4],
            Tint::Grass => rgb(colors[0]),
            Tint::Foliage => rgb(colors[1]),
            Tint::Birch => rgb(colors[2]),
            Tint::Evergreen => rgb(colors[3]),
            Tint::Water => self.water,
        }
    }
}

/// The climate of each Bedrock biome id, 0 to 255, from the Java biome with
/// that `be_network_id`. Several Java biomes can share one; the lowest Java
/// id wins, so the table is the same on every run. Returns the table and
/// how many Java biomes lost that way.
fn climates() -> (Vec<Option<Climate>>, usize) {
    let mut biomes: Vec<&Biome> = Biome::ALL.to_vec();
    biomes.sort_by_key(|biome| biome.id);
    let mut table = vec![None; 256];
    let mut collisions = 0;
    for biome in biomes {
        let slot = &mut table[usize::from(biome.be_network_id)];
        if slot.is_some() {
            collisions += 1;
        } else {
            *slot = Some(Climate {
                temperature: biome.weather.base_temperature(),
                downfall: biome.weather.downfall(),
            });
        }
    }
    (table, collisions)
}

/// The colormap texel for a climate, as Java's `GrassColors` and
/// `FoliageColors` find it. Both values are clamped to 0..1, and downfall is
/// scaled by temperature, so only the lower-left triangle is ever used. Hot
/// and wet is the top-left corner; cold is the bottom-right.
///
/// The arithmetic is Java's too: the `f32` climate widened to `f64`, and a
/// truncating cast. That matters at texel edges: 0.8 as an `f32` is
/// 0.800000011920929, so plains lands on column 50, not 51.
fn sample(map: &RgbaImage, climate: Climate) -> [u8; 3] {
    let temperature = f64::from(climate.temperature.clamp(0.0, 1.0));
    let downfall = f64::from(climate.downfall.clamp(0.0, 1.0)) * temperature;
    let (width, height) = map.dimensions();
    let x = ((1.0 - temperature) * f64::from(width - 1)) as u32;
    let y = ((1.0 - downfall) * f64::from(height - 1)) as u32;
    let texel = map.get_pixel(x.min(width - 1), y.min(height - 1));
    [texel[0], texel[1], texel[2]]
}

/// The default water surface colour and transparency, from
/// `biomes_client.json` merged over the stack: a higher pack's field wins.
fn water_color(stack: &PackStack) -> Result<[u8; 4], String> {
    let mut color = None;
    let mut transparency = None;
    for (pack, read) in stack.layers("biomes_client.json") {
        let bytes = match read {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(err) => return Err(format!("{}: {err}", pack.name())),
        };
        let file: Value = json::parse(&bytes).map_err(|err| format!("{}: {err}", pack.name()))?;
        let default = &file["biomes"]["default"];
        if let Some(hex) = default["water_surface_color"].as_str() {
            color = Some(parse_hex(hex).ok_or_else(|| format!("{}: bad colour `{hex}`", pack.name()))?);
        }
        if let Some(value) = default["water_surface_transparency"].as_f64() {
            transparency = Some(value);
        }
    }
    let [r, g, b] = color.ok_or("no default water_surface_color in any pack")?;
    let alpha = transparency.ok_or("no default water_surface_transparency in any pack")?;
    Ok([r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8])
}

/// `#RRGGBB` to bytes.
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[cfg(test)]
mod tests {
    use image::Rgba;
    use limonite_packs::Pack;

    use super::*;
    use crate::test_util::{TempDir, png};

    /// A colormap whose texel at (x, y) is (x, y, 0), so a sample tells
    /// where it landed.
    fn coordinates() -> RgbaImage {
        RgbaImage::from_fn(256, 256, |x, y| Rgba([x as u8, y as u8, 0, 255]))
    }

    fn at(temperature: f32, downfall: f32) -> [u8; 3] {
        sample(&coordinates(), Climate { temperature, downfall })
    }

    #[test]
    fn samples_a_colormaps_corners() {
        // Hot and wet: top left.
        assert_eq!(at(1.0, 1.0), [0, 0, 0]);
        // Hot and dry: bottom left.
        assert_eq!(at(1.0, 0.0), [0, 255, 0]);
        // Cold: bottom right, whatever the downfall.
        assert_eq!(at(0.0, 1.0), [255, 255, 0]);
        // Out of range clamps.
        assert_eq!(at(2.0, 5.0), [0, 0, 0]);
        assert_eq!(at(-1.0, -1.0), [255, 255, 0]);
        // Plains, 0.8 and 0.4: x = 0.19999999 × 255 = 50.99, y = (1 - 0.32) × 255 = 173.4.
        assert_eq!(at(0.8, 0.4), [50, 173, 0]);
    }

    #[test]
    fn every_pumpkin_biome_has_a_climate_and_the_lowest_java_id_wins() {
        let (table, collisions) = climates();
        for biome in Biome::ALL {
            assert!(table[usize::from(biome.be_network_id)].is_some(), "{}", biome.registry_id);
        }
        let plains = table[usize::from(Biome::PLAINS.be_network_id)].unwrap();
        assert_eq!(plains, Climate { temperature: 0.8, downfall: 0.4 });
        let distinct: std::collections::HashSet<u8> = Biome::ALL.iter().map(|b| b.be_network_id).collect();
        assert_eq!(collisions, Biome::ALL.len() - distinct.len());
    }

    #[test]
    fn loads_colormaps_and_the_water_colour_from_packs() {
        let dir = TempDir::new("tints");
        dir.write("low/textures/colormap/grass.png", &png(&coordinates()));
        dir.write(
            "low/biomes_client.json",
            br##"/* a comment */ { "biomes": { "default": { "water_surface_color": "#000000", "water_surface_transparency": 0.5 } } }"##,
        );
        // A higher pack changes the colour only.
        dir.write("high/biomes_client.json", br##"{ "biomes": { "default": { "water_surface_color": "#44AFF5" } } }"##);
        let stack = PackStack::new(vec![Pack::new(dir.path().join("low")), Pack::new(dir.path().join("high"))]);
        let (tints, report) = Tints::load(&stack);

        assert_eq!(report.water, Ok([0x44, 0xAF, 0xF5, 128]));
        assert_eq!(tints.color(1, Tint::Water), [0x44, 0xAF, 0xF5, 128]);
        assert_eq!(report.colormaps[0].1, Ok((256, 256)));
        assert!(report.colormaps[1].1.is_err(), "no foliage map in this pack");
        // Plains grass from the coordinate map; foliage falls back to white.
        assert_eq!(tints.color(1, Tint::Grass), [50, 173, 0, 255]);
        assert_eq!(tints.color(1, Tint::Foliage), [255; 4]);
        assert_eq!(tints.color(1, Tint::None), [255; 4]);
        // An id beyond the table gets plains.
        assert_eq!(tints.color(9999, Tint::Grass), [50, 173, 0, 255]);
    }

    #[test]
    fn parses_hex_colours() {
        assert_eq!(parse_hex("#44AFF5"), Some([0x44, 0xAF, 0xF5]));
        assert_eq!(parse_hex("44AFF5"), None);
        assert_eq!(parse_hex("#44AFF"), None);
        assert_eq!(parse_hex("#GGAFF5"), None);
    }
}
