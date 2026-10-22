//! Block textures on the CPU: decoded from the packs into one 2D texture
//! array, one layer per texture, all at one size, with mipmaps (m4-plan
//! Decision 7). Step 10 uploads it to the GPU.
//!
//! **Why an array, not an atlas.** An atlas packs every texture into one big
//! image, and each face gets UVs into it. An array keeps each texture in its
//! own layer instead:
//! - nothing bleeds between neighbouring textures, because a layer has no
//!   neighbours: filtering and mipmapping stay inside it;
//! - each texture gets its own mipmaps, all the way down to 1×1;
//! - a face stores a layer number, and its UVs always run from 0 to 1.
//!
//! The cost: every layer has the same size, so smaller textures are scaled
//! up to the largest (PLAN.md, "Textures").

use std::{collections::HashMap, time::Instant};

use image::RgbaImage;
use limonite_packs::{ImageFormat, PackStack, TextureFile};

/// Bedrock's own texture for anything that can't be resolved. It's always
/// layer 0.
pub const MISSING_TEXTURE: &str = "textures/misc/missing_texture";

/// What a texture's alpha channel means. It decides how mipmaps average the
/// colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlphaUse {
    /// How much of the texel is there: leaves, glass, plants. Transparent
    /// texels often hold junk colour, so they mustn't darken the average.
    Coverage,
    /// A mask that marks texels to tint: a grass block's sides. The texture
    /// is drawn opaque, so every texel's colour counts.
    Mask,
}

/// The array, ready to upload.
pub struct TextureArray {
    /// Every layer's width and height, in texels.
    pub size: u32,
    pub layers: u32,
    /// The mip levels, largest first. Level n holds every layer at
    /// `size >> n` texels square, one after another, as RGBA bytes.
    pub mips: Vec<Vec<u8>>,
}

impl TextureArray {
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.mips.iter().map(Vec::len).sum()
    }
}

/// Where a texture went in the array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer {
    pub index: u16,
    /// Some texel of its first frame isn't fully opaque.
    pub transparent: bool,
}

/// The array, and what happened to each texture asked for.
pub struct Loaded {
    pub array: TextureArray,
    /// For each requested texture, in order, its layer, or `None` if it
    /// couldn't be found or decoded. Those draw as layer 0, the missing
    /// texture.
    pub layers: Vec<Option<Layer>>,
    /// The paths that couldn't be loaded, and why.
    pub failed: Vec<(String, String)>,
    /// How long decoding and building took.
    pub seconds: f64,
}

/// Loads every texture in `requests` (a path without its extension, and
/// how its alpha is used) into one array, after the missing texture.
#[must_use]
pub fn load(stack: &PackStack, requests: &[(String, AlphaUse)]) -> Loaded {
    let start = Instant::now();
    let mut failed = Vec::new();
    let mut frames = Vec::new();

    // Layer 0. Should the install lack it, a checkerboard of our own stands
    // in: nothing Mojang's is ever bundled.
    let missing = match read_frame(stack, MISSING_TEXTURE) {
        Ok(frame) => frame,
        Err(reason) => {
            failed.push((MISSING_TEXTURE.to_string(), reason));
            checkerboard()
        }
    };
    frames.push((missing, AlphaUse::Coverage));

    // The same path can be asked for more than once; it gets one layer.
    let mut by_request: HashMap<(&str, AlphaUse), Option<u16>> = HashMap::new();
    let mut layer_of = Vec::with_capacity(requests.len());
    for (path, alpha) in requests {
        let index = *by_request.entry((path.as_str(), *alpha)).or_insert_with(|| {
            match read_frame(stack, path) {
                Ok(frame) => {
                    frames.push((frame, *alpha));
                    Some((frames.len() - 1) as u16)
                }
                Err(reason) => {
                    failed.push((path.clone(), reason));
                    None
                }
            }
        });
        layer_of.push(index);
    }

    let size = frames
        .iter()
        .map(|(frame, _)| frame.width().max(frame.height()))
        .max()
        .unwrap_or(1)
        .next_power_of_two();
    let mut transparent = Vec::with_capacity(frames.len());
    let mut levels: Vec<Vec<u8>> = (0..=size.ilog2()).map(|_| Vec::new()).collect();
    for (frame, alpha) in &frames {
        transparent.push(frame.pixels().any(|texel| texel[3] < 255));
        let mut level = scale_nearest(frame, size);
        let mut level_size = size;
        for mip in &mut levels {
            mip.extend_from_slice(&level);
            if level_size > 1 {
                level = downsample(&level, level_size, *alpha);
                level_size /= 2;
            }
        }
    }

    let layers = layer_of
        .into_iter()
        .map(|index| {
            index.map(|index| Layer {
                index,
                transparent: transparent[usize::from(index)],
            })
        })
        .collect();
    Loaded {
        array: TextureArray {
            size,
            layers: frames.len() as u32,
            mips: levels,
        },
        layers,
        failed,
        seconds: start.elapsed().as_secs_f64(),
    }
}

/// Finds a texture in the stack, decodes it, and takes its first frame.
fn read_frame(stack: &PackStack, path: &str) -> Result<RgbaImage, String> {
    read_image(stack, path).map(first_frame)
}

/// Finds an image in the stack (a path without its extension) and decodes
/// it whole.
///
/// # Errors
/// If no pack has it, or it can't be decoded.
pub fn read_image(stack: &PackStack, path: &str) -> Result<RgbaImage, String> {
    let file = stack
        .read_texture(path)
        .map_err(|err| err.to_string())?
        .ok_or("in no pack")?;
    decode(&file)
}

/// Decodes a PNG or TGA. TGA has no magic number, so the extension it was
/// found under says which. `image` flips TGAs stored bottom-up.
fn decode(file: &TextureFile) -> Result<RgbaImage, String> {
    let format = match file.format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Tga => image::ImageFormat::Tga,
    };
    image::load_from_memory_with_format(&file.bytes, format)
        .map(|image| image.to_rgba8())
        .map_err(|err| format!("can't decode it as {}: {err}", file.format.extension()))
}

/// A strip taller than it is wide is a flipbook: its frames are squares
/// stacked top to bottom. Animation comes with M8, so M4 shows frame 0.
fn first_frame(image: RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    if height > width {
        image::imageops::crop_imm(&image, 0, 0, width, width).to_image()
    } else {
        image
    }
}

/// Scales `frame` to `size` × `size` by nearest neighbour, returning RGBA
/// bytes. A 16×16 texture in a 32 array becomes each texel doubled, so pixel
/// art stays sharp instead of blurring.
fn scale_nearest(frame: &RgbaImage, size: u32) -> Vec<u8> {
    let (width, height) = frame.dimensions();
    if (width, height) == (size, size) {
        return frame.as_raw().clone();
    }
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        let source_y = y * height / size;
        for x in 0..size {
            let source_x = x * width / size;
            out.extend_from_slice(&frame.get_pixel(source_x, source_y).0);
        }
    }
    out
}

/// The next mip level: each 2×2 block of texels averaged into one (a box
/// filter).
///
/// Alpha is always the plain average. For [`AlphaUse::Coverage`], colour is
/// weighted by alpha, so the junk colour in see-through texels can't bleed
/// a dark fringe into a leaf's edge.
fn downsample(level: &[u8], size: u32, alpha_use: AlphaUse) -> Vec<u8> {
    let half = size / 2;
    let size = size as usize;
    let texel = |x: usize, y: usize| &level[(y * size + x) * 4..(y * size + x) * 4 + 4];
    let mut out = Vec::with_capacity(half as usize * half as usize * 4);
    for y in 0..half as usize {
        for x in 0..half as usize {
            let block = [
                texel(2 * x, 2 * y),
                texel(2 * x + 1, 2 * y),
                texel(2 * x, 2 * y + 1),
                texel(2 * x + 1, 2 * y + 1),
            ];
            let alpha_sum: u32 = block.iter().map(|t| u32::from(t[3])).sum();
            for channel in 0..3 {
                let value = match alpha_use {
                    AlphaUse::Coverage if alpha_sum > 0 => {
                        let weighted: u32 = block.iter().map(|t| u32::from(t[channel]) * u32::from(t[3])).sum();
                        (weighted + alpha_sum / 2) / alpha_sum
                    }
                    _ => (block.iter().map(|t| u32::from(t[channel])).sum::<u32>() + 2) / 4,
                };
                out.push(value as u8);
            }
            out.push(((alpha_sum + 2) / 4) as u8);
        }
    }
    out
}

/// A 16×16 magenta and black checkerboard, drawn here.
fn checkerboard() -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            image::Rgba([255, 0, 255, 255])
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    })
}

#[cfg(test)]
mod tests {
    use image::Rgba;
    use limonite_packs::Pack;

    use super::*;
    use crate::test_util::{TempDir, png, tga};

    fn stack(dir: &TempDir) -> PackStack {
        PackStack::new(vec![Pack::new(dir.path().to_path_buf())])
    }

    #[test]
    fn a_flipbook_shows_its_first_frame_scaled_up() {
        let dir = TempDir::new("flipbook");
        // A 2-frame strip of 16×16: red on top, blue below.
        let strip = RgbaImage::from_fn(16, 32, |_, y| {
            if y < 16 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) }
        });
        dir.write("textures/blocks/water_still.png", &png(&strip));
        // A 32×32 texture sets the layer size.
        let big = RgbaImage::from_fn(32, 32, |x, _| Rgba([x as u8, 0, 0, 255]));
        dir.write("textures/blocks/shelf.png", &png(&big));
        dir.write("textures/misc/missing_texture.png", &png(&checkerboard()));

        let loaded = load(
            &stack(&dir),
            &[
                ("textures/blocks/water_still".to_string(), AlphaUse::Coverage),
                ("textures/blocks/shelf".to_string(), AlphaUse::Coverage),
                ("textures/blocks/nothing".to_string(), AlphaUse::Coverage),
            ],
        );
        assert_eq!(loaded.array.size, 32);
        assert_eq!(loaded.array.layers, 3, "missing, water, shelf");
        assert_eq!(loaded.layers[0].map(|l| l.index), Some(1));
        assert_eq!(loaded.layers[2], None);
        assert_eq!(loaded.failed.len(), 1);

        // Layer 1, level 0: the red first frame, every texel doubled.
        let layer = &loaded.array.mips[0][32 * 32 * 4..2 * 32 * 32 * 4];
        assert!(layer.as_chunks::<4>().0.iter().all(|t| *t == [255, 0, 0, 255]));
        // The shelf's columns 2x and 2x+1 come from source columns 2x, 2x+1:
        // a 32-wide texture isn't scaled.
        let shelf = &loaded.array.mips[0][2 * 32 * 32 * 4..];
        assert_eq!(shelf[5 * 4], 5);
    }

    #[test]
    fn the_mip_chain_reaches_one_texel() {
        let dir = TempDir::new("mips");
        let grey = RgbaImage::from_pixel(16, 16, Rgba([100, 150, 200, 255]));
        dir.write("textures/blocks/a.tga", &tga(&grey));
        let loaded = load(&stack(&dir), &[("textures/blocks/a".to_string(), AlphaUse::Coverage)]);
        // The missing texture isn't in this pack, so the checkerboard stands in.
        assert_eq!(loaded.failed[0].0, MISSING_TEXTURE);
        assert_eq!(loaded.array.size, 16);
        let sizes: Vec<usize> = loaded.array.mips.iter().map(Vec::len).collect();
        assert_eq!(sizes, [16 * 16 * 4 * 2, 8 * 8 * 4 * 2, 4 * 4 * 4 * 2, 2 * 2 * 4 * 2, 4 * 2]);
        // A flat colour stays that colour all the way down.
        let last = &loaded.array.mips[4];
        assert_eq!(&last[4..8], [100, 150, 200, 255]);
        assert!(!loaded.layers[0].unwrap().transparent);
    }

    #[test]
    fn mipmaps_weigh_colour_by_coverage_but_not_masks() {
        // One opaque white texel, three see-through black ones.
        let level = [
            255, 255, 255, 255, 0, 0, 0, 0, //
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(downsample(&level, 2, AlphaUse::Coverage), [255, 255, 255, 64]);
        assert_eq!(downsample(&level, 2, AlphaUse::Mask), [64, 64, 64, 64]);
        // All see-through: no weight, so a plain average.
        assert_eq!(downsample(&[8, 8, 8, 0].repeat(4), 2, AlphaUse::Coverage), [8, 8, 8, 0]);
    }

    #[test]
    fn transparency_is_reported_per_texture() {
        let dir = TempDir::new("alpha");
        let leaves = RgbaImage::from_fn(16, 16, |x, _| Rgba([0, 200, 0, if x % 2 == 0 { 255 } else { 0 }]));
        dir.write("textures/blocks/leaves.tga", &tga(&leaves));
        let loaded = load(&stack(&dir), &[("textures/blocks/leaves".to_string(), AlphaUse::Coverage)]);
        assert!(loaded.layers[0].unwrap().transparent);
    }
}
