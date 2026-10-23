//! What Bedrock block ids mean: the palette names each one, and the
//! appearance table says how it looks.

pub mod appearance;
pub mod palette;
pub mod palette_source;

use std::{collections::HashMap, time::Instant};

use tracing::{debug, info, warn};

use self::{
    appearance::{Built, Report},
    palette::Palette,
};
use crate::{install::Vanilla, world::chunk::Chunk};

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

/// The Bedrock ids drawn as stand-in cubes, with their block names.
#[must_use]
pub fn stand_in_ids(palette: &Palette, report: &Report) -> HashMap<u32, String> {
    palette
        .iter()
        .filter(|(_, state)| report.stand_ins.contains(&*state.name))
        .map(|(id, state)| (id, state.name.to_string()))
        .collect()
}

/// Logs how many loaded blocks are drawn as stand-in cubes, by name, most
/// first: the to-do list for a special-shapes pass (owner, M4).
pub fn log_stand_in_census<'a>(chunks: impl Iterator<Item = &'a Chunk>, stand_ins: &HashMap<u32, String>) {
    let start = Instant::now();
    let mut loaded = 0;
    let census = stand_in_census(chunks.inspect(|_| loaded += 1), stand_ins);
    let total: u64 = census.iter().map(|(_, count)| count).sum();
    let list: Vec<String> = census.iter().map(|(name, count)| format!("{name} {count}")).collect();
    info!(
        "Stand-in cubes in the {loaded} loaded chunks: {total} blocks of {} names, most first \
         (counted in {:.0} ms): {}",
        census.len(),
        start.elapsed().as_secs_f64() * 1000.0,
        list.join(", "),
    );
}

/// Counts the blocks in `chunks` whose id is in `stand_ins`, by name: most
/// first, then by name. Names with no blocks are left out.
fn stand_in_census<'a>(chunks: impl Iterator<Item = &'a Chunk>, stand_ins: &HashMap<u32, String>) -> Vec<(String, u64)> {
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for storage in chunks.flat_map(|chunk| &chunk.sub_chunks).flat_map(|sub_chunk| &sub_chunk.layers) {
        // Which palette entries are stand-ins. Most storages have none, and
        // are skipped without reading their 4,096 positions.
        let names: Vec<Option<&str>> = storage
            .palette()
            .iter()
            .map(|id| stand_ins.get(id).map(String::as_str))
            .collect();
        if names.iter().all(Option::is_none) {
            continue;
        }
        let mut uses = vec![0u64; names.len()];
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..16 {
                    uses[storage.palette_index(x, y, z)] += 1;
                }
            }
        }
        for (name, count) in names.into_iter().zip(uses) {
            if let Some(name) = name {
                *counts.entry(name).or_default() += count;
            }
        }
    }
    let mut census: Vec<(String, u64)> = counts
        .into_iter()
        .filter(|&(_, count)| count > 0)
        .map(|(name, count)| (name.to_string(), count))
        .collect();
    census.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    census
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{chunk::SubChunk, storage::Storage};

    fn chunk(layers: Vec<Storage>) -> Chunk {
        Chunk {
            x: 0,
            z: 0,
            dimension: 0,
            min_section: -4,
            sub_chunks: vec![SubChunk { layers }],
            biomes: Vec::new(),
            block_actors: Vec::new(),
        }
    }

    #[test]
    fn the_census_counts_stand_ins_by_name_most_first() {
        // Two bamboo states, one fence; 1 is stone and 0 air, neither a stand-in.
        let stand_ins: HashMap<u32, String> = [(10, "bamboo"), (11, "bamboo"), (20, "oak_fence")]
            .into_iter()
            .map(|(id, name)| (id, name.to_string()))
            .collect();
        let all_bamboo = chunk(vec![Storage::Single(10)]);
        let mut mixed = Storage::Single(1);
        for (x, id) in [(0, 20), (1, 20), (2, 20), (3, 11), (4, 11)] {
            mixed.set(x, 0, 0, id);
        }
        // A fence placed and then broken stays in the palette, used nowhere.
        let mut stale = Storage::Single(1);
        stale.set(5, 5, 5, 20);
        stale.set(5, 5, 5, 1);
        let chunks = [all_bamboo, chunk(vec![mixed, Storage::Single(0)]), chunk(vec![stale])];

        let census = stand_in_census(chunks.iter(), &stand_ins);
        assert_eq!(
            census,
            [("bamboo".to_string(), 4096 + 2), ("oak_fence".to_string(), 3)]
        );
        assert!(stand_in_census(chunks[2..].iter(), &stand_ins).is_empty(), "unused entries count nothing");
    }
}
