//! What the `LocalChannel` costs: per-packet byte counts, the client's
//! framing and decode time, and the server's chunk encode time.
//!
//! Only meaningful in release builds; debug timings say nothing.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use pumpkin_core::{net::bedrock::level_chunk::CLevelChunk, server::Server};
use pumpkin_protocol::{
    bedrock::{SubClient, client::CStartGame, packet_encoder::BedrockBatchEncoder},
    packet::Packet,
    serial::PacketWrite,
};
use tracing::info;

use crate::packets::PacketName;

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

/// The server's chunk encode cost, measured on its loaded chunks.
pub struct ChunkEncode {
    pub chunks: usize,
    pub passes: u32,
    /// Per chunk: block actors, `encode_chunk` and the batch wrapper, as the
    /// server's `send_chunks` does them.
    pub per_chunk: Duration,
    pub bytes_per_chunk: f64,
}

/// Encodes every loaded overworld chunk `passes` times, exactly as the server
/// does before sending one, and averages the time per chunk.
///
/// Runs on the calling thread. The server doesn't need to help: the loaded
/// chunks are shared, and encoding is plain synchronous code.
pub fn chunk_encode(server: &Server, passes: u32) -> Option<ChunkEncode> {
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

    let encoder = BedrockBatchEncoder::new();
    let mut bytes = 0usize;
    let start = Instant::now();
    for _ in 0..passes {
        for chunk in &chunks {
            let block_actors = world.bedrock_chunk_block_actors(chunk);
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
        }
    }
    let encodes = chunks.len() as u32 * passes;
    Some(ChunkEncode {
        chunks: chunks.len(),
        passes,
        per_chunk: start.elapsed() / encodes,
        bytes_per_chunk: bytes as f64 / f64::from(encodes),
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

/// Logs the numbers PLAN.md's "special-case chunks with `Arc`" decision needs.
pub fn log_decision(stats: &Stats, encode: &ChunkEncode, start_game_bytes_per_us: Option<f64>) {
    let chunks_received = stats
        .by_id
        .get(&CLevelChunk::PACKET_ID)
        .map_or(0, |r| r.count);
    let join_chunk_encode = encode.per_chunk * u32::try_from(chunks_received).unwrap_or(u32::MAX);
    let tick = Duration::from_millis(50);
    info!(
        "Chunk encode: {:.1} µs and {:.0} bytes per chunk ({} loaded chunks x {} passes)",
        micros(encode.per_chunk),
        encode.bytes_per_chunk,
        encode.chunks,
        encode.passes,
    );
    info!(
        "This join: {chunks_received} LevelChunk packets = {:.2} ms of chunk encode, {:.1}% of one 50 ms tick",
        micros(join_chunk_encode) / 1000.0,
        100.0 * join_chunk_encode.as_secs_f64() / tick.as_secs_f64(),
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
        let chunk_us = micros(join_chunk_encode);
        info!(
            "Non-chunk encode, estimated at StartGame's {bytes_per_us:.0} bytes/µs: {other_us:.0} µs \
             for {other_bytes} bytes; chunks are ~{:.1}% of encode time",
            100.0 * chunk_us / (chunk_us + other_us),
        );
    }
}

pub fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e6
}
