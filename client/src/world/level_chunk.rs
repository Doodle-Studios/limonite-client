//! Decodes a `LevelChunk` packet body into a [`Chunk`].
//!
//! It follows `CLevelChunk::encode_chunk` (`pumpkin-core`,
//! `net/bedrock/level_chunk.rs`) byte for byte, with the client cache off,
//! which is the only form our client asks for. The layout is summarised in
//! `docs/milestones/m3-plan.md`, Finding 1.
//!
//! Bad input is a [`DecodeError`], never a panic: a client must survive
//! whatever arrives.

use std::{fmt, io::Cursor};

use pumpkin_nbt::{Nbt, deserializer::NbtReadHelperBedrock};

use super::{
    chunk::{Chunk, SubChunk},
    storage::Storage,
};

/// The only sub-chunk format Pumpkin writes: version, layer count, then the
/// sub-chunk's y index.
const SUB_CHUNK_VERSION: u8 = 9;

/// Sub-chunk counts at or above this aren't counts: Bedrock uses
/// `u32::MAX` and `u32::MAX - 1` for "ask for sub-chunks separately".
const SUB_CHUNK_REQUEST_MODES: u32 = u32::MAX - 1;

/// Why a `LevelChunk` couldn't be decoded. `at` offsets count from the start
/// of the packet body, or of the payload for errors inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The data ended at `at` with more expected.
    Truncated { at: usize },
    /// `len` bytes were left after the payload.
    TrailingData { len: usize },
    /// A `VarInt` at `at` ran past five bytes.
    BadVarInt { at: usize },
    /// The chunk refers to cache blobs. Our client never enables the cache.
    CacheBlobs,
    /// The chunk asks the client to request its sub-chunks separately.
    SubChunkRequests,
    /// A sub-chunk format other than version 9.
    Version(u8),
    /// A sub-chunk with other than one or two layers.
    LayerCount(u8),
    /// Sub-chunks must be consecutive, from the bottom up.
    SubChunkOrder { expected: i8, found: i8 },
    /// A storage in the persistent form, with NBT block states.
    Persistent,
    /// An index width Bedrock doesn't use.
    Width(u8),
    /// A palette length of zero, or more than the width can address.
    PaletteLength(i32),
    /// A palette index past the end of the palette.
    IndexOutOfRange { index: u32, palette_len: usize },
    /// A block actor that isn't valid Bedrock network NBT.
    BlockActor(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at } => write!(f, "truncated at byte {at}"),
            Self::TrailingData { len } => write!(f, "{len} bytes after the payload"),
            Self::BadVarInt { at } => write!(f, "VarInt longer than 5 bytes at byte {at}"),
            Self::CacheBlobs => write!(f, "uses cache blobs, which we never enable"),
            Self::SubChunkRequests => write!(f, "uses sub-chunk requests"),
            Self::Version(v) => write!(f, "sub-chunk version {v}, expected {SUB_CHUNK_VERSION}"),
            Self::LayerCount(n) => write!(f, "sub-chunk with {n} layers, expected 1 or 2"),
            Self::SubChunkOrder { expected, found } => {
                write!(f, "sub-chunk y {found}, expected {expected}")
            }
            Self::Persistent => write!(f, "storage in the persistent (NBT) form"),
            Self::Width(bits) => write!(f, "unsupported index width {bits}"),
            Self::PaletteLength(len) => write!(f, "bad palette length {len}"),
            Self::IndexOutOfRange { index, palette_len } => {
                write!(f, "index {index} past a palette of {palette_len}")
            }
            Self::BlockActor(err) => write!(f, "bad block actor NBT: {err}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// A cursor over bytes whose reads fail with a [`DecodeError`] instead of
/// panicking.
///
/// The `VarInt` readers are our own rather than Pumpkin's, on purpose: the
/// round-trip tests then check Pumpkin's writer against independent code.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub const fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or(DecodeError::Truncated { at: self.at })?;
        self.at += 1;
        Ok(byte)
    }

    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        Ok(self.u8()? != 0)
    }

    pub fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(DecodeError::Truncated {
                at: self.bytes.len(),
            })?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    /// The rest of the bytes, consuming them.
    pub fn rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.at..];
        self.at = self.bytes.len();
        rest
    }

    /// An unsigned LEB128 `VarInt`: seven bits per byte, low bits first, the
    /// top bit set on every byte but the last.
    pub fn var_u32(&mut self) -> Result<u32, DecodeError> {
        let start = self.at;
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = self.u8()?;
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError::BadVarInt { at: start })
    }

    /// A signed `VarInt`, zigzag-encoded: 0, -1, 1, -2, ... become 0, 1, 2, 3, ...
    pub fn var_i32(&mut self) -> Result<i32, DecodeError> {
        let raw = self.var_u32()?;
        Ok((raw >> 1).cast_signed() ^ -((raw & 1).cast_signed()))
    }
}

/// Decodes one `LevelChunk` packet body.
///
/// # Errors
/// Any [`DecodeError`]; see its variants.
pub fn decode(body: &[u8]) -> Result<Chunk, DecodeError> {
    let mut reader = Reader::new(body);
    let x = reader.var_i32()?;
    let z = reader.var_i32()?;
    let dimension = reader.var_i32()?;
    let sub_chunk_count = reader.var_u32()?;
    if sub_chunk_count >= SUB_CHUNK_REQUEST_MODES {
        return Err(DecodeError::SubChunkRequests);
    }
    // "Request limit present": always false from Pumpkin, which sends whole
    // chunks. What would follow it is undefined for us.
    if reader.bool()? {
        return Err(DecodeError::SubChunkRequests);
    }
    let cache_enabled = reader.bool()?;
    let blob_count = reader.var_u32()?;
    if cache_enabled || blob_count != 0 {
        return Err(DecodeError::CacheBlobs);
    }
    let payload_len = reader.var_u32()? as usize;
    let payload = reader.take(payload_len)?;
    if reader.remaining() != 0 {
        return Err(DecodeError::TrailingData {
            len: reader.remaining(),
        });
    }

    let mut payload = Reader::new(payload);
    // Never trust a count for an allocation; 64 covers every dimension.
    let capacity = (sub_chunk_count as usize).min(64);
    let mut sub_chunks = Vec::with_capacity(capacity);
    let mut min_section = 0i8;
    for i in 0..sub_chunk_count {
        let version = payload.u8()?;
        if version != SUB_CHUNK_VERSION {
            return Err(DecodeError::Version(version));
        }
        let layer_count = payload.u8()?;
        if !(1..=2).contains(&layer_count) {
            return Err(DecodeError::LayerCount(layer_count));
        }
        let y = payload.u8()?.cast_signed();
        if i == 0 {
            min_section = y;
        } else {
            // `wrapping_add` keeps a corrupt count from overflowing; the order
            // check then fails instead.
            let expected = min_section.wrapping_add(i as i8);
            if y != expected {
                return Err(DecodeError::SubChunkOrder { expected, found: y });
            }
        }
        let mut layers = Vec::with_capacity(usize::from(layer_count));
        for _ in 0..layer_count {
            layers.push(Storage::read(&mut payload)?);
        }
        sub_chunks.push(SubChunk { layers });
    }

    // One biome storage per sub-chunk, bottom-up, with no header of its own.
    let mut biomes = Vec::with_capacity(capacity);
    for _ in 0..sub_chunk_count {
        biomes.push(Storage::read(&mut payload)?);
    }

    // Border blocks: a count, then that many bytes. Pumpkin sends none.
    let border_blocks = payload.u8()?;
    payload.take(usize::from(border_blocks))?;

    // Block actors: Bedrock network NBT compounds back to back, to the end.
    let rest = payload.rest();
    let mut cursor = Cursor::new(rest);
    let mut block_actors = Vec::new();
    while (cursor.position() as usize) < rest.len() {
        let mut nbt = NbtReadHelperBedrock::new(&mut cursor);
        let actor = Nbt::read(&mut nbt).map_err(|err| DecodeError::BlockActor(err.to_string()))?;
        block_actors.push(actor.root_tag);
    }

    Ok(Chunk {
        x,
        z,
        dimension,
        min_section: i32::from(min_section),
        sub_chunks,
        biomes,
        block_actors,
    })
}
