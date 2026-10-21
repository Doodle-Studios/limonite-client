//! What the `LocalChannel` costs: per-packet byte counts, the client's
//! framing and decode time, the server's chunk encode time split into its
//! phases, and how chunks stream while the host moves.
//!
//! Only meaningful in release builds; debug timings say nothing.

use std::{
    collections::BTreeMap,
    hint::black_box,
    thread,
    time::{Duration, Instant},
};

use pumpkin_core::{net::bedrock::level_chunk::CLevelChunk, server::Server};
use pumpkin_data::{Block, BlockState};
use pumpkin_protocol::{
    bedrock::{SubClient, client::CStartGame, packet_encoder::BedrockBatchEncoder},
    packet::Packet,
    serial::PacketWrite,
};
use pumpkin_util::math::{position::BlockPos, vector2::Vector2, vector3::Vector3};
use pumpkin_world::{chunk::palette::NetworkPalette, world::BlockFlags};
use tracing::{info, warn};

use crate::{
    packets::PacketName,
    session::{FLY_STEP, Session, TICK},
    world::{AIR, level_chunk},
};

/// Received packets of one id.
#[derive(Default)]
struct Received {
    count: u64,
    /// Packet bodies, without the length and header.
    body_bytes: u64,
    /// How many were decoded, and how long that took.
    decoded: u64,
    decode_time: Duration,
}

/// Everything the session received, and what handling it cost.
#[derive(Default)]
pub struct Stats {
    by_id: BTreeMap<i32, Received>,
    batches: u64,
    batch_bytes: u64,
    /// Time spent splitting batches into packets.
    framing: Duration,
}

impl Stats {
    pub fn record_batch(&mut self, bytes: usize, framing: Duration) {
        self.batches += 1;
        self.batch_bytes += bytes as u64;
        self.framing += framing;
    }

    pub fn record_packet(&mut self, id: i32, body_bytes: usize) {
        let received = self.by_id.entry(id).or_default();
        received.count += 1;
        received.body_bytes += body_bytes as u64;
    }

    pub fn record_decode(&mut self, id: i32, time: Duration) {
        let received = self.by_id.entry(id).or_default();
        received.decoded += 1;
        received.decode_time += time;
    }

    /// The share of received body bytes that were `LevelChunk` packets.
    pub fn chunk_byte_share(&self) -> f64 {
        let total: u64 = self.by_id.values().map(|r| r.body_bytes).sum();
        let chunks = self
            .by_id
            .get(&CLevelChunk::PACKET_ID)
            .map_or(0, |r| r.body_bytes);
        if total == 0 {
            0.0
        } else {
            chunks as f64 / total as f64
        }
    }

    /// The average decode time of packets with this id, and how many were
    /// decoded.
    fn decode_average(&self, id: i32) -> Option<(Duration, u64)> {
        let received = self.by_id.get(&id)?;
        (received.decoded > 0).then(|| {
            (
                received.decode_time / u32::try_from(received.decoded).unwrap_or(u32::MAX),
                received.decoded,
            )
        })
    }

    /// Logs one line per packet name, largest byte total first, then totals.
    pub fn log_table(&self) {
        let mut rows: Vec<_> = self.by_id.iter().collect();
        rows.sort_by_key(|(_, received)| std::cmp::Reverse(received.body_bytes));
        info!(
            "{:<30} {:>7} {:>12} {:>14}",
            "packet", "count", "body bytes", "decode µs/pkt"
        );
        for (id, received) in rows {
            let decode = if received.decoded == 0 {
                String::new()
            } else {
                format!(
                    "{:.1}",
                    micros(received.decode_time) / received.decoded as f64
                )
            };
            info!(
                "{:<30} {:>7} {:>12} {:>14}",
                PacketName(*id).to_string(),
                received.count,
                received.body_bytes,
                decode
            );
        }
        let packets: u64 = self.by_id.values().map(|r| r.count).sum();
        info!(
            "{packets} packets in {} batches, {} batch bytes; framing {:.0} µs total, {:.2} µs per batch",
            self.batches,
            self.batch_bytes,
            micros(self.framing),
            micros(self.framing) / self.batches.max(1) as f64,
        );
    }
}

/// What one chunk costs, per phase, measured on the server's loaded chunks.
/// Every duration is an average per chunk.
pub struct ChunkCosts {
    pub chunks: usize,
    pub passes: u32,
    /// E: `CLevelChunk::encode_chunk` plus the batch wrapper (M2's number).
    pub encode: Duration,
    /// Gathering the block actors (`World::bedrock_chunk_block_actors`),
    /// which `send_chunks` does just before encoding.
    pub block_actors: Duration,
    pub bytes: f64,
    /// C, Pumpkin's palette conversions to Bedrock's format, one by one:
    /// blocks, the water layer, and biomes.
    pub convert_blocks: Duration,
    pub convert_water: Duration,
    pub convert_biomes: Duration,
    /// D: our decode of the encoded chunk.
    pub decode: Duration,
    /// S: a snapshot clone of the block and biome sections, as an `Arc` path
    /// would need (Finding 5), including dropping it.
    pub snapshot: Duration,
    /// Heterogeneous block sections by Bedrock bits per entry, over all
    /// loaded chunks.
    pub bits: BTreeMap<u8, u64>,
    pub homogeneous_sections: u64,
    pub water_layers: u64,
}

impl ChunkCosts {
    /// C: all three conversions.
    pub fn conversions(&self) -> Duration {
        self.convert_blocks + self.convert_water + self.convert_biomes
    }
}

/// Measures every loaded overworld chunk `passes` times per phase, exactly as
/// the server would process it, and averages per chunk.
///
/// Each phase runs in passes of its own, so each one meets the same cache
/// state: the chunks together are far bigger than the CPU caches.
/// `keep_alive` runs after every pass, so the session can tick: the server
/// drops a client it hasn't heard from in 10 s.
///
/// Runs on the calling thread. The server doesn't need to help: the loaded
/// chunks are shared, and the work is plain synchronous code.
pub fn chunk_costs(server: &Server, passes: u32, mut keep_alive: impl FnMut()) -> Option<ChunkCosts> {
    let world = server.worlds.load().first()?.clone();
    let chunks: Vec<_> = world
        .level
        .loaded_chunks
        .iter()
        .map(|entry| entry.value().clone())
        .collect();
    if chunks.is_empty() || passes == 0 {
        return None;
    }
    let runs = chunks.len() as u32 * passes;

    // E, and the payloads D decodes.
    let encoder = BedrockBatchEncoder::new();
    let mut payloads = Vec::with_capacity(chunks.len());
    let (mut bytes, mut encode, mut gather) = (0usize, Duration::ZERO, Duration::ZERO);
    for pass in 0..passes {
        for chunk in &chunks {
            let start = Instant::now();
            let block_actors = world.bedrock_chunk_block_actors(chunk);
            gather += start.elapsed();

            let start = Instant::now();
            let Ok((payload, _blobs)) = CLevelChunk::encode_chunk(chunk, 0, false, &block_actors)
            else {
                continue;
            };
            let mut batch = Vec::new();
            if encoder
                .write_game_packet(
                    CLevelChunk::PACKET_ID as u16,
                    SubClient::Main,
                    SubClient::Main,
                    &payload,
                    &mut batch,
                )
                .is_ok()
            {
                bytes += batch.len();
            }
            encode += start.elapsed();
            if pass == 0 {
                payloads.push(payload);
            }
        }
        keep_alive();
    }

    // C, each conversion timed over a chunk's sections, under the read locks
    // `encode_chunk` takes.
    let (mut blocks, mut water, mut biomes) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    for _ in 0..passes {
        for chunk in &chunks {
            let sections = chunk
                .section
                .block_sections
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let start = Instant::now();
            for section in sections.iter() {
                black_box(section.convert_be_network());
            }
            blocks += start.elapsed();
            let start = Instant::now();
            for section in sections.iter() {
                black_box(section.convert_be_water_network());
            }
            water += start.elapsed();
            drop(sections);

            let sections = chunk
                .section
                .biome_sections
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let start = Instant::now();
            for section in sections.iter() {
                black_box(section.convert_be_network());
            }
            biomes += start.elapsed();
        }
        keep_alive();
    }

    // D.
    let mut decode = Duration::ZERO;
    for _ in 0..passes {
        for payload in &payloads {
            let start = Instant::now();
            let _ = black_box(level_chunk::decode(black_box(payload)));
            decode += start.elapsed();
        }
        keep_alive();
    }

    // S.
    let mut snapshot = Duration::ZERO;
    for _ in 0..passes {
        for chunk in &chunks {
            let start = Instant::now();
            let blocks = chunk
                .section
                .block_sections
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let biomes = chunk
                .section
                .biome_sections
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            drop(black_box((blocks, biomes)));
            snapshot += start.elapsed();
        }
        keep_alive();
    }

    // The widths, untimed.
    let mut bits = BTreeMap::new();
    let (mut homogeneous_sections, mut water_layers) = (0, 0);
    for chunk in &chunks {
        let sections = chunk
            .section
            .block_sections
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for section in sections.iter() {
            let network = section.convert_be_network();
            if matches!(network.palette, NetworkPalette::Single(_)) {
                homogeneous_sections += 1;
            } else {
                *bits.entry(network.bits_per_entry).or_insert(0) += 1;
            }
            if section.convert_be_water_network().is_some() {
                water_layers += 1;
            }
        }
    }

    Some(ChunkCosts {
        chunks: chunks.len(),
        passes,
        encode: encode / runs,
        block_actors: gather / runs,
        bytes: bytes as f64 / f64::from(runs),
        convert_blocks: blocks / runs,
        convert_water: water / runs,
        convert_biomes: biomes / runs,
        decode: decode / (payloads.len() as u32 * passes).max(1),
        snapshot: snapshot / runs,
        bits,
        homogeneous_sections,
        water_layers,
    })
}

/// Pumpkin's writer throughput on `StartGame`, a large, field-by-field packet,
/// in bytes per microsecond: a yardstick for encoding the non-chunk packets.
pub fn start_game_throughput(start_game: &CStartGame, passes: u32) -> f64 {
    let mut sink = Vec::new();
    let start = Instant::now();
    for _ in 0..passes {
        sink.clear();
        let _ = start_game.write(&mut sink);
    }
    let per_pass = micros(start.elapsed()) / f64::from(passes.max(1));
    sink.len() as f64 / per_pass
}

/// Logs the chunk cost table and the numbers the `Arc` decision needs, for a
/// join of `join_chunks` chunks.
pub fn log_costs(
    stats: &Stats,
    costs: &ChunkCosts,
    join_chunks: u32,
    start_game_bytes_per_us: Option<f64>,
) {
    let tick = Duration::from_millis(50);
    let e = micros(costs.encode);
    let c = micros(costs.conversions());
    let d = micros(costs.decode);
    let s = micros(costs.snapshot);
    info!(
        "Chunk costs per chunk ({} loaded chunks x {} passes per phase):",
        costs.chunks, costs.passes
    );
    info!(
        "  E  encode_chunk + batch wrapper: {e:.1} µs, {:.0} bytes; gathering block actors \
         first: {:.1} µs",
        costs.bytes,
        micros(costs.block_actors),
    );
    info!(
        "  C  palette conversions: blocks {:.1} + water layer {:.1} + biomes {:.1} = {c:.1} µs \
         ({:.0}% of E)",
        micros(costs.convert_blocks),
        micros(costs.convert_water),
        micros(costs.convert_biomes),
        100.0 * c / e,
    );
    info!(
        "  E-C the rest (writing words and palettes, block actor NBT): {:.1} µs",
        e - c
    );
    match stats.decode_average(CLevelChunk::PACKET_ID) {
        Some((average, count)) => info!(
            "  D  our decode: {d:.1} µs (during the session: {:.1} µs over {count} chunks)",
            micros(average)
        ),
        None => info!("  D  our decode: {d:.1} µs"),
    }
    info!("  S  snapshot of the block and biome sections: {s:.1} µs");
    let widths: Vec<String> = costs
        .bits
        .iter()
        .map(|(bits, count)| format!("{bits} bits: {count}"))
        .collect();
    info!(
        "  Block sections: {} homogeneous; heterogeneous by width: {}; {} with a water layer",
        costs.homogeneous_sections,
        widths.join(", "),
        costs.water_layers,
    );

    // What an `Arc` path would save: it skips E and D, but still needs C to
    // reach the client's Bedrock-native format and S to be correct.
    let saving = (e + d) - (s + c);
    info!("  Arc saving per chunk = (E + D) - (S + C) = {saving:.1} µs");
    let join = f64::from(join_chunks);
    let ms = |per_chunk_us: f64| per_chunk_us * join / 1000.0;
    info!(
        "This join: {join_chunks} chunks; E {:.1} ms, D {:.1} ms, S {:.1} ms, C {:.1} ms; \
         Arc saving {:.1} ms ({:.0}% of one 50 ms tick)",
        ms(e),
        ms(d),
        ms(s),
        ms(c),
        ms(saving),
        100.0 * ms(saving) / (tick.as_secs_f64() * 1000.0),
    );
    info!(
        "Chunks are {:.1}% of the body bytes received",
        100.0 * stats.chunk_byte_share()
    );
    if let Some(bytes_per_us) = start_game_bytes_per_us {
        let other_bytes: u64 = stats
            .by_id
            .iter()
            .filter(|(id, _)| **id != CLevelChunk::PACKET_ID)
            .map(|(_, r)| r.body_bytes)
            .sum();
        let other_us = other_bytes as f64 / bytes_per_us;
        let chunk_us = e * join;
        info!(
            "Non-chunk encode, estimated at StartGame's {bytes_per_us:.0} bytes/µs: {other_us:.0} µs \
             for {other_bytes} bytes; chunks are ~{:.1}% of encode time",
            100.0 * chunk_us / (chunk_us + other_us),
        );
    }
}

/// How long the probe waits for each `UpdateBlock`.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// The probe's block: near the top of the world, where nothing else happens.
const PROBE_Y: i32 = 318;

/// Check 2: how soon a block change shows in the server's shared `ChunkData`
/// (what a live `Arc` would see), against when our world gets its
/// `UpdateBlock`.
///
/// Sets stone above the host through the server's own `set_block_state`,
/// reads it straight back from the loaded chunk, then waits for our world.
/// Restores air the same way afterwards, even on a timeout.
pub fn block_change_probe(session: &mut Session, server: &Server) {
    let Some(world) = server.worlds.load().first().cloned() else {
        warn!("Probe: the server has no world");
        return;
    };
    let host = session.position();
    let (x, z) = (host.x.floor() as i32, host.z.floor() as i32);
    let pos = BlockPos(Vector3::new(x, PROBE_Y, z));
    let stone = Block::STONE.default_state.id;

    let start = Instant::now();
    world.set_block_state(&pos, stone, BlockFlags::NOTIFY_ALL);
    let returned = start.elapsed();
    let shared = world
        .level
        .loaded_chunks
        .get(&Vector2::new(x >> 4, z >> 4))
        .map(|entry| entry.value().clone())
        .and_then(|chunk| {
            chunk
                .section
                .get_block_absolute_y((x & 15) as usize, PROBE_Y, (z & 15) as usize)
        });
    let read_back = start.elapsed();
    let ours = wait_for_block(session, (x, PROBE_Y, z), BlockState::to_be_network_id(stone), start);

    if shared == Some(stone) {
        info!(
            "Probe: set_block_state returned after {:.1} µs; the shared ChunkData already held \
             stone when read {:.1} µs after the call",
            micros(returned),
            micros(read_back),
        );
    } else {
        warn!("Probe: the shared ChunkData didn't show the stone straight away ({shared:?})");
    }
    match ours {
        Some(after) => info!(
            "Probe: our world applied the UpdateBlock {:.1} ms after the call ({:.1} ticks)",
            micros(after) / 1000.0,
            after.as_secs_f64() / TICK.as_secs_f64(),
        ),
        None => warn!(
            "Probe: no UpdateBlock for the stone within {} s",
            PROBE_TIMEOUT.as_secs()
        ),
    }

    let start = Instant::now();
    world.set_block_state(&pos, Block::AIR.default_state.id, BlockFlags::NOTIFY_ALL);
    match wait_for_block(session, (x, PROBE_Y, z), AIR, start) {
        Some(after) => info!(
            "Probe: restored air; our world saw it {:.1} ms later",
            micros(after) / 1000.0
        ),
        None => warn!("Probe: restored air, but our world didn't see it"),
    }
}

/// Polls the session every half millisecond, ticking it on schedule, until
/// our world holds `id` at `pos` on layer 0. Returns the time since `start`.
fn wait_for_block(
    session: &mut Session,
    (x, y, z): (i32, i32, i32),
    id: u32,
    start: Instant,
) -> Option<Duration> {
    let mut next_tick = Instant::now() + TICK;
    loop {
        session.drain();
        if session.world().block(x, y, z, 0) == Some(id) {
            return Some(start.elapsed());
        }
        if start.elapsed() > PROBE_TIMEOUT {
            return None;
        }
        if Instant::now() >= next_tick {
            session.tick();
            next_tick += TICK;
        }
        thread::sleep(Duration::from_micros(500));
    }
}

/// The eye height measured runs stay at and `--stream` flies at: above any
/// overworld terrain.
pub const SAFE_EYE_Y: f32 = 330.0;
/// Bedrock clients get at most this many chunks per server tick
/// (`INITIAL_CHUNKS_PER_TICK`, which only Java acknowledgements change).
const CHUNKS_PER_TICK_CAP: f64 = 9.0;

/// Check 3, streaming: flies `blocks` east at creative flying speed and
/// logs how many `LevelChunk`s arrive per second, against the estimate
/// d·v/16 from a join of `join_chunks` chunks and the server's cap.
pub fn stream(session: &mut Session, server: &Server, blocks: u32, join_chunks: u32) {
    let ticks = (blocks as f32 / FLY_STEP).ceil() as u32;
    let before = session.level_chunks();
    let server_x = || server.get_all_players().first().map(|player| player.position().x);
    let server_start = server_x();
    session.fly_east(ticks, SAFE_EYE_Y);
    info!("Streaming: flying {blocks} blocks east at {FLY_STEP} blocks a tick ({ticks} ticks)");

    let start = Instant::now();
    let mut per_second = Vec::new();
    let (mut second_base, mut next_second) = (before, start + Duration::from_secs(1));
    let _ = session.run_until(start + TICK * (ticks + 20), |s| {
        if Instant::now() >= next_second {
            per_second.push(s.level_chunks() - second_base);
            second_base = s.level_chunks();
            next_second += Duration::from_secs(1);
        }
        !s.is_walking()
    });
    let flight = start.elapsed().as_secs_f64();
    let during = session.level_chunks() - before;

    // The chunks still on their way when the flight ends.
    let stopped = session.level_chunks();
    let mut last_new = Instant::now();
    let mut seen = stopped;
    let _ = session.run_until(Instant::now() + Duration::from_secs(60), |s| {
        if s.level_chunks() != seen {
            seen = s.level_chunks();
            last_new = Instant::now();
        }
        last_new.elapsed() >= Duration::from_secs(2)
    });
    let tail = session.level_chunks() - stopped;
    // The server's own view: a dead or stuck player's input is ignored, and
    // then nothing streams.
    match (server_start, server_x()) {
        (Some(start), Some(end)) => info!(
            "Streaming: the server's player moved {:.1} blocks east",
            end - start
        ),
        _ => warn!("Streaming: the server has no player"),
    }

    let speed = f64::from(blocks) / flight;
    let rate = f64::from(during) / flight;
    let cap = CHUNKS_PER_TICK_CAP / TICK.as_secs_f64();
    // Moving one chunk exposes a new row about as long as the view diameter.
    let diameter = 2.0 * (f64::from(join_chunks) / std::f64::consts::PI).sqrt();
    let estimate = (diameter * speed / 16.0).min(cap);
    let peak = per_second.iter().copied().max().unwrap_or(0);
    info!(
        "Streaming: {during} LevelChunk packets in {flight:.1} s of flight at {speed:.1} m/s, \
         {rate:.0} per second; estimate d·v/16 = {diameter:.1} x {speed:.1} / 16 = {estimate:.0} \
         per second; then {tail} more after stopping"
    );
    info!(
        "Streaming: per second {per_second:?}; peak {peak} against the cap of {cap:.0} \
         (9 per tick): {}",
        if f64::from(peak) >= 0.95 * cap {
            "reached"
        } else {
            "not reached"
        }
    );
}

pub fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e6
}
