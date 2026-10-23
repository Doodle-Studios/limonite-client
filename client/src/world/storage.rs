//! One palette-compressed 16×16×16 storage, kept exactly as Bedrock sends it.
//!
//! A storage holds 4,096 values: block network ids in a sub-chunk layer, or
//! biome ids. Instead of 4,096 full values it keeps a short palette of the
//! distinct values and, per position, a small index into that palette. The
//! indices are packed `bits` at a time into little-endian `u32` words.

use super::level_chunk::{DecodeError, Reader};

/// Positions in one storage.
pub const VOLUME: usize = 16 * 16 * 16;

/// The index widths Bedrock packs with. Any other width, such as 7, is invalid.
const WIDTHS: [u8; 8] = [1, 2, 3, 4, 5, 6, 8, 16];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    /// Every position holds this value (width 0 on the wire).
    Single(u32),
    /// `bits` per position, `32 / bits` positions per word, low bits first.
    /// Widths 3, 5 and 6 leave the top bits of each word unused.
    ///
    /// Every index is below `palette.len()`: [`Storage::read`] checks them all.
    Packed {
        bits: u8,
        words: Box<[u32]>,
        palette: Box<[u32]>,
    },
}

/// Bedrock's position order inside a storage: x, then z, then y (XZY), so the
/// 16 blocks of one column are next to each other.
///
/// Pumpkin's own sections use YZX; its encoder reorders them on the way out.
const fn index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < 16 && y < 16 && z < 16);
    (x << 8) | (z << 4) | y
}

impl Storage {
    /// The value at `(x, y, z)`, each below 16.
    ///
    /// It returns a plain `u32`, not an `Option`: the palette indices were all
    /// checked once, when the storage was read, so a lookup can't miss. M4's
    /// mesher reads every position of every sub-chunk, so checking per read
    /// would cost far more than checking once.
    #[must_use]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u32 {
        match self {
            Self::Single(value) => *value,
            Self::Packed {
                bits,
                words,
                palette,
            } => palette[packed_index(*bits, words, index(x, y, z))],
        }
    }

    /// The distinct values: what [`Storage::palette_index`] indexes.
    ///
    /// With [`Storage::palette_index`], a reader that visits every position,
    /// like the mesher, can translate each palette entry once instead of
    /// each of the 4,096 values.
    #[must_use]
    pub fn palette(&self) -> &[u32] {
        match self {
            Self::Single(value) => std::slice::from_ref(value),
            Self::Packed { palette, .. } => palette,
        }
    }

    /// The palette index at `(x, y, z)`, each below 16. Always in range of
    /// [`Storage::palette`]: every index was checked when the storage was
    /// read.
    #[must_use]
    pub fn palette_index(&self, x: usize, y: usize, z: usize) -> usize {
        match self {
            Self::Single(_) => 0,
            Self::Packed { bits, words, .. } => packed_index(*bits, words, index(x, y, z)),
        }
    }

    /// Sets `(x, y, z)` to `value`, growing the palette, and repacking at the
    /// next width when the palette outgrows the current one.
    ///
    /// The palette never shrinks: a value that's no longer used keeps its
    /// entry. That's harmless, and a later `LevelChunk` replaces the storage.
    pub fn set(&mut self, x: usize, y: usize, z: usize, value: u32) {
        let at = index(x, y, z);
        let mut palette = match self {
            Self::Single(old) if *old == value => return,
            Self::Single(old) => vec![*old],
            Self::Packed {
                bits,
                words,
                palette,
            } => {
                if let Some(found) = palette.iter().position(|&entry| entry == value) {
                    write_packed_index(*bits, words, at, found);
                    return;
                }
                if palette.len() < 1 << *bits {
                    let mut grown = palette.to_vec();
                    grown.push(value);
                    *palette = grown.into_boxed_slice();
                    write_packed_index(*bits, words, at, palette.len() - 1);
                    return;
                }
                palette.to_vec()
            }
        };
        // The palette is full (or this was a single value): repack wider.
        let mut indices = self.indices();
        indices[at] = palette.len() as u16;
        palette.push(value);
        *self = Self::pack(&indices, palette);
    }

    /// Reads one storage: a header byte, then the words and palette.
    pub(super) fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let header = reader.u8()?;
        // The low bit says how the palette is written: 1 for network ids
        // (VarInts), 0 for the "persistent" form with NBT block states, which
        // only appears on disk and in some server software. Pumpkin always
        // sends network ids.
        if header & 1 == 0 {
            return Err(DecodeError::Persistent);
        }
        let bits = header >> 1;
        if bits == 0 {
            // Width 0: no words, and a single id with no length in front.
            return Ok(Self::Single(reader.var_i32()?.cast_unsigned()));
        }
        if !WIDTHS.contains(&bits) {
            return Err(DecodeError::Width(bits));
        }

        let per_word = 32 / usize::from(bits);
        let bytes = reader.take(VOLUME.div_ceil(per_word) * 4)?;
        // `as_chunks` views the bytes as `[u8; 4]` arrays; the remainder is
        // empty because exactly whole words were taken.
        let words: Box<[u32]> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&word| u32::from_le_bytes(word))
            .collect();

        // A palette longer than the width can address would be pointless, and
        // the limit keeps a corrupt length from allocating gigabytes.
        let len = reader.var_i32()?;
        let palette_len = usize::try_from(len)
            .ok()
            .filter(|&n| n >= 1 && n <= 1 << bits)
            .ok_or(DecodeError::PaletteLength(len))?;
        let palette = (0..palette_len)
            .map(|_| reader.var_i32().map(i32::cast_unsigned))
            .collect::<Result<Box<[u32]>, _>>()?;

        let storage = Self::Packed {
            bits,
            words,
            palette,
        };
        storage.check_indices()?;
        Ok(storage)
    }

    /// Checks every index against the palette, once, so [`Storage::get`] never
    /// has to. Padding bits and the unused tail of the last word are ignored.
    fn check_indices(&self) -> Result<(), DecodeError> {
        let Self::Packed {
            bits,
            words,
            palette,
        } = self
        else {
            return Ok(());
        };
        // Every index a full palette's width can hold is in range.
        if palette.len() == 1 << bits {
            return Ok(());
        }
        let bits = u32::from(*bits);
        let per_word = 32 / bits as usize;
        let mask = (1u32 << bits) - 1;
        for (w, word) in words.iter().enumerate() {
            let in_word = per_word.min(VOLUME - w * per_word);
            for slot in 0..in_word as u32 {
                let found = (word >> (slot * bits)) & mask;
                if found as usize >= palette.len() {
                    return Err(DecodeError::IndexOutOfRange {
                        index: found,
                        palette_len: palette.len(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Every position's palette index, in XZY order.
    fn indices(&self) -> [u16; VOLUME] {
        let mut indices = [0u16; VOLUME];
        if let Self::Packed { bits, words, .. } = self {
            for (i, out) in indices.iter_mut().enumerate() {
                *out = packed_index(*bits, words, i) as u16;
            }
        }
        indices
    }

    /// Packs `indices` at the narrowest width that addresses `palette`.
    fn pack(indices: &[u16; VOLUME], palette: Vec<u32>) -> Self {
        let bits = WIDTHS
            .into_iter()
            .find(|&bits| palette.len() <= 1 << bits)
            .unwrap_or(16);
        let per_word = 32 / usize::from(bits);
        let mut words = vec![0u32; VOLUME.div_ceil(per_word)].into_boxed_slice();
        for (i, &found) in indices.iter().enumerate() {
            write_packed_index(bits, &mut words, i, usize::from(found));
        }
        Self::Packed {
            bits,
            words,
            palette: palette.into_boxed_slice(),
        }
    }
}

/// The palette index stored for position `i`.
fn packed_index(bits: u8, words: &[u32], i: usize) -> usize {
    let bits = usize::from(bits);
    let per_word = 32 / bits;
    let shift = (i % per_word) * bits;
    let mask = (1u32 << bits) - 1;
    ((words[i / per_word] >> shift) & mask) as usize
}

fn write_packed_index(bits: u8, words: &mut [u32], i: usize, found: usize) {
    let bits = usize::from(bits);
    let per_word = 32 / bits;
    let shift = (i % per_word) * bits;
    let mask = (1u32 << bits) - 1;
    let word = &mut words[i / per_word];
    *word = (*word & !(mask << shift)) | ((found as u32 & mask) << shift);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a storage the way Bedrock does, packing by hand rather than with
    /// the code under test.
    fn wire(bits: u8, palette: &[i32], index_at: impl Fn(usize) -> u32) -> Vec<u8> {
        let mut out = vec![(bits << 1) | 1];
        if bits == 0 {
            write_var_i32(&mut out, palette[0]);
            return out;
        }
        let per_word = 32 / usize::from(bits);
        for w in 0..VOLUME.div_ceil(per_word) {
            let mut word = 0u32;
            for slot in 0..per_word {
                let i = w * per_word + slot;
                if i < VOLUME {
                    word |= index_at(i) << (slot * usize::from(bits));
                }
            }
            out.extend_from_slice(&word.to_le_bytes());
        }
        write_var_i32(&mut out, palette.len() as i32);
        for &id in palette {
            write_var_i32(&mut out, id);
        }
        out
    }

    fn write_var_i32(out: &mut Vec<u8>, value: i32) {
        let mut v = ((value << 1) ^ (value >> 31)).cast_unsigned();
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn read(bytes: &[u8]) -> Result<Storage, DecodeError> {
        let mut reader = Reader::new(bytes);
        let storage = Storage::read(&mut reader)?;
        assert_eq!(reader.remaining(), 0, "bytes left over");
        Ok(storage)
    }

    #[test]
    fn reads_every_width() {
        for bits in [0u8, 1, 2, 3, 4, 5, 6, 8, 16] {
            // The largest palette this width can address, capped at 300 ids,
            // with negative ids in it: Bedrock's hashed ids are often negative
            // as an i32.
            let len = if bits == 0 { 1 } else { (1usize << bits).min(300) };
            let palette: Vec<i32> = (0..len as i32).map(|n| n * 7919 - 1000).collect();
            let index_at = |i: usize| ((i * 31 + i / 7) % len) as u32;
            let storage = read(&wire(bits, &palette, index_at)).unwrap();
            for x in 0..16 {
                for y in 0..16 {
                    for z in 0..16 {
                        let i = (x << 8) | (z << 4) | y;
                        let expected = palette[index_at(i) as usize].cast_unsigned();
                        assert_eq!(storage.get(x, y, z), expected, "width {bits} at {i}");
                    }
                }
            }
        }
    }

    #[test]
    fn hand_packed_words_pin_the_layout() {
        // Width 1: position 0 in bit 0 of word 0; position 33 in bit 1 of word 1.
        let mut bytes = vec![0b11];
        let mut words = vec![0u32; 128];
        words[0] = 0b1;
        words[1] = 0b10;
        for word in &words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[4, 10, 20]); // palette [5, 10] as zigzag VarInts
        let storage = read(&bytes).unwrap();
        assert_eq!(storage.get(0, 0, 0), 10);
        assert_eq!(storage.get(0, 1, 0), 5);
        assert_eq!(storage.get(0, 1, 2), 10); // i = 2*16 + 1 = 33
        assert_eq!(storage.get(0, 0, 2), 5);

        // Width 3: ten positions per word, then two padding bits, set here to
        // show they're ignored. Position 10 starts word 1.
        let mut bytes = vec![0b111];
        let mut words = vec![0xC000_0000u32; 410];
        words[0] |= 5 << 3; // position 1 -> index 5
        words[1] |= 7; // position 10 -> index 7
        for word in &words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.push(16); // palette length 8
        bytes.extend((0..8).map(|n| n * 2)); // ids 0..8
        let storage = read(&bytes).unwrap();
        assert_eq!(storage.get(0, 1, 0), 5);
        assert_eq!(storage.get(0, 10, 0), 7);
        assert_eq!(storage.get(0, 0, 0), 0);

        // Width 16: two positions per word, the second in the high half.
        let mut bytes = vec![0b10_0001];
        let mut words = vec![0u32; 2048];
        words[2047] = 1 << 16; // position 4095, the last one
        for word in &words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[4, 2, 4]); // palette length 2: [1, 2]
        let storage = read(&bytes).unwrap();
        assert_eq!(storage.get(15, 15, 15), 2);
        assert_eq!(storage.get(15, 14, 15), 1);
    }

    #[test]
    fn positions_are_in_xzy_order() {
        // Index 1 at exactly one position, i = 256: x = 1, z = 0, y = 0.
        let storage = read(&wire(1, &[100, 200], |i| u32::from(i == 256))).unwrap();
        assert_eq!(storage.get(1, 0, 0), 200);
        assert_eq!(storage.get(0, 0, 1), 100); // i = 16
        assert_eq!(storage.get(0, 1, 0), 100); // i = 1
    }

    #[test]
    fn rejects_bad_storages() {
        // A persistent (NBT palette) storage: low bit 0.
        assert_eq!(read(&[0b10]), Err(DecodeError::Persistent));
        // Width 7 doesn't exist.
        assert_eq!(read(&[(7 << 1) | 1]), Err(DecodeError::Width(7)));
        // Index 2 with a two-entry palette at width 2.
        let bytes = wire(2, &[1, 2], |i| if i == 4000 { 2 } else { 0 });
        assert_eq!(
            read(&bytes),
            Err(DecodeError::IndexOutOfRange {
                index: 2,
                palette_len: 2
            })
        );
        // A palette longer than the width can address.
        let mut bytes = wire(1, &[1, 2], |_| 0);
        let palette_at = bytes.len() - 3;
        bytes.truncate(palette_at);
        bytes.extend_from_slice(&[6, 2, 4, 6]);
        assert_eq!(read(&bytes), Err(DecodeError::PaletteLength(3)));
        // Every truncation is an error, never a panic.
        let bytes = wire(4, &[1, 2, 3], |i| (i % 3) as u32);
        for cut in 0..bytes.len() {
            assert!(
                matches!(read(&bytes[..cut]), Err(DecodeError::Truncated { .. })),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn set_grows_and_repacks() {
        let mut storage = Storage::Single(1);
        storage.set(3, 4, 5, 1);
        assert_eq!(storage, Storage::Single(1));

        storage.set(3, 4, 5, 2);
        assert!(matches!(storage, Storage::Packed { bits: 1, .. }));
        assert_eq!(storage.get(3, 4, 5), 2);
        assert_eq!(storage.get(3, 4, 6), 1);

        // Fill past every width up to 8 bits; earlier values must survive.
        for n in 0..200u32 {
            let (x, y, z) = (n as usize % 16, n as usize / 16 % 16, 9);
            storage.set(x, y, z, 1000 + n);
        }
        assert!(matches!(storage, Storage::Packed { bits: 8, .. }));
        assert_eq!(storage.get(3, 4, 5), 2);
        assert_eq!(storage.get(0, 0, 0), 1);
        for n in 0..200u32 {
            assert_eq!(storage.get(n as usize % 16, n as usize / 16 % 16, 9), 1000 + n);
        }

        // An existing value reuses its palette entry.
        storage.set(0, 0, 0, 2);
        assert_eq!(storage.get(0, 0, 0), 2);
    }
}
