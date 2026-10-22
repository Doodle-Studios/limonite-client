//! Bedrock's `__brarchive` format: the files of one pack folder, stored
//! uncompressed in one file.
//!
//! Mojang hasn't documented it. The layout was worked out from the archives
//! in a Bedrock 1.26.52 install (m4-plan Finding 3). All numbers are
//! little-endian:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 0..8 | the magic number, [`MAGIC`] |
//! | 8..12 | the entry count, `u32` |
//! | 12..16 | the version, `u32`: always [`VERSION`] |
//! | 16.. | the index: [`ENTRY_LEN`] bytes per entry |
//! | after the index | the data: every file's bytes |
//!
//! An index entry is a `u8` name length, the name in a [`NAME_FIELD_LEN`]-byte
//! field, then the file's offset from the start of the data and its size,
//! both `u32`.
//!
//! The reader is strict, because a client reads whatever the user's install
//! holds. Everything is checked when an archive is opened, every failure is a
//! typed [`ArchiveError`], and no input can make it panic.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    ops::Range,
};

/// The first eight bytes of every archive.
pub const MAGIC: [u8; 8] = [0x7D, 0x27, 0x25, 0xB1, 0xA0, 0x52, 0x70, 0x26];
/// The only version seen: all 1,244 archives of a 1.26.52 install have it.
pub const VERSION: u32 = 1;
/// The magic number, the entry count and the version.
pub const HEADER_LEN: usize = 16;
/// One index entry: the name length, the name field, the offset and the size.
pub const ENTRY_LEN: usize = 256;
/// The bytes an entry has for its name.
pub const NAME_FIELD_LEN: usize = 247;

/// Why an archive couldn't be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveError {
    /// The file doesn't start with [`MAGIC`], so it isn't an archive.
    Magic,
    /// The 16-byte header is cut short: the file is only `len` bytes.
    HeaderTruncated { len: usize },
    /// A version other than [`VERSION`].
    Version(u32),
    /// The index of `count` entries needs the file to be at least `needed`
    /// bytes, but it's only `len`.
    IndexTruncated { count: u32, needed: u64, len: usize },
    /// Entry number `entry` has a name of `len` bytes, longer than its field.
    NameTooLong { entry: u32, len: u8 },
    /// Entry number `entry` has a name that isn't UTF-8.
    NameNotUtf8 { entry: u32 },
    /// Two entries have this name, so a lookup would be ambiguous.
    DuplicateName(String),
    /// The file `name` reaches past the end of the data, which is `data_len`
    /// bytes. An archive cut short fails here.
    EntryOutOfBounds {
        name: String,
        offset: u32,
        size: u32,
        data_len: usize,
    },
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Magic => write!(f, "not a brarchive: wrong magic number"),
            Self::HeaderTruncated { len } => {
                write!(f, "header cut short: the file is only {len} bytes")
            }
            Self::Version(version) => {
                write!(f, "unsupported version {version}; only {VERSION} is known")
            }
            Self::IndexTruncated { count, needed, len } => write!(
                f,
                "index cut short: {count} entries need {needed} bytes, the file has {len}"
            ),
            Self::NameTooLong { entry, len } => write!(
                f,
                "entry {entry}: a {len}-byte name doesn't fit its {NAME_FIELD_LEN}-byte field"
            ),
            Self::NameNotUtf8 { entry } => write!(f, "entry {entry}: the name isn't UTF-8"),
            Self::DuplicateName(name) => write!(f, "two entries are named `{name}`"),
            Self::EntryOutOfBounds {
                name,
                offset,
                size,
                data_len,
            } => write!(
                f,
                "`{name}` (offset {offset}, size {size}) reaches past the end of the \
                 {data_len}-byte data"
            ),
        }
    }
}

impl std::error::Error for ArchiveError {}

/// An open archive: its bytes, and where each file is in them.
pub struct Archive {
    bytes: Box<[u8]>,
    /// Each file's range in `bytes`. Entries of size zero aren't here: they
    /// count as absent. Sound archives list their `.fsb` files that way, and
    /// keep the sounds themselves as loose files.
    files: HashMap<String, Range<usize>>,
}

impl Archive {
    /// Opens an archive from its bytes, checking all of it.
    ///
    /// Every entry's range is checked here, not when the entry is read. Each
    /// archive in an install ends exactly where its last file does, so an
    /// archive cut short anywhere fails to open, rather than failing later
    /// on one file.
    ///
    /// # Errors
    /// The first problem found, checked in file order: the magic number, the
    /// header, the version, the index, then each entry.
    pub fn open(bytes: Vec<u8>) -> Result<Self, ArchiveError> {
        // The magic is compared against however much of it the file has, so
        // a file that isn't an archive is reported as such, even a short one.
        let magic_len = bytes.len().min(MAGIC.len());
        if bytes[..magic_len] != MAGIC[..magic_len] {
            return Err(ArchiveError::Magic);
        }
        let Some(header) = bytes.first_chunk::<HEADER_LEN>() else {
            return Err(ArchiveError::HeaderTruncated { len: bytes.len() });
        };
        let count = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        let version = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        if version != VERSION {
            return Err(ArchiveError::Version(version));
        }

        // In u64: 16 + 256 × u32::MAX doesn't fit in 32 bits, and a usize is
        // only 32 bits on some targets. A corrupt count fails here, before
        // anything is allocated for it.
        let needed = HEADER_LEN as u64 + ENTRY_LEN as u64 * u64::from(count);
        let data_start = usize::try_from(needed)
            .ok()
            .filter(|&n| n <= bytes.len())
            .ok_or(ArchiveError::IndexTruncated {
                count,
                needed,
                len: bytes.len(),
            })?;
        let data_len = bytes.len() - data_start;

        let mut files = HashMap::new();
        let mut names = HashSet::new();
        // `as_chunks` views the index as `[u8; 256]` arrays, one per entry.
        // The remainder is empty, because exactly `count` entries fit.
        let (entries, _) = bytes[HEADER_LEN..data_start].as_chunks::<ENTRY_LEN>();
        for (number, raw) in (0u32..).zip(entries) {
            // An array pattern names each field, and `name_field @ ..` binds
            // the 247 bytes in between. The compiler checks the sizes add up
            // to 256, so nothing here can index out of bounds.
            let &[
                name_len,
                ref name_field @ ..,
                o0,
                o1,
                o2,
                o3,
                s0,
                s1,
                s2,
                s3,
            ] = raw;
            let Some(name) = name_field.get(..usize::from(name_len)) else {
                return Err(ArchiveError::NameTooLong {
                    entry: number,
                    len: name_len,
                });
            };
            let name = std::str::from_utf8(name)
                .map_err(|_| ArchiveError::NameNotUtf8 { entry: number })?
                .to_string();
            let offset = u32::from_le_bytes([o0, o1, o2, o3]);
            let size = u32::from_le_bytes([s0, s1, s2, s3]);

            // Checked: offset + size can overflow.
            let range = usize::try_from(offset)
                .ok()
                .zip(usize::try_from(size).ok())
                .and_then(|(start, size)| Some(start..start.checked_add(size)?))
                .filter(|range| range.end <= data_len);
            let Some(range) = range else {
                return Err(ArchiveError::EntryOutOfBounds {
                    name,
                    offset,
                    size,
                    data_len,
                });
            };

            // Names are checked across all entries, zero-size ones included.
            if !names.insert(name.clone()) {
                return Err(ArchiveError::DuplicateName(name));
            }
            if size > 0 {
                files.insert(name, data_start + range.start..data_start + range.end);
            }
        }
        Ok(Self {
            bytes: bytes.into_boxed_slice(),
            files,
        })
    }

    /// A file's bytes, or `None` if the archive has no such file, or only a
    /// zero-size entry for it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        // Every range was checked when the archive was opened.
        self.files.get(name).map(|range| &self.bytes[range.clone()])
    }

    /// The names of the files with data, in no particular order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// How many files have data.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One index entry, written field by field, independently of the reader.
    struct RawEntry<'a> {
        name: &'a [u8],
        /// What the name length byte says; usually `name.len()`.
        name_len: u8,
        offset: u32,
        size: u32,
    }

    impl<'a> RawEntry<'a> {
        fn new(name: &'a str, offset: u32, size: u32) -> Self {
            Self {
                name: name.as_bytes(),
                name_len: name.len() as u8,
                offset,
                size,
            }
        }
    }

    /// An archive with the given header fields, index and data.
    fn build(magic: [u8; 8], version: u32, entries: &[RawEntry<'_>], data: &[u8]) -> Vec<u8> {
        let mut out = magic.to_vec();
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        for entry in entries {
            let mut raw = [0u8; ENTRY_LEN];
            raw[0] = entry.name_len;
            raw[1..=entry.name.len()].copy_from_slice(entry.name);
            raw[248..252].copy_from_slice(&entry.offset.to_le_bytes());
            raw[252..256].copy_from_slice(&entry.size.to_le_bytes());
            out.extend_from_slice(&raw);
        }
        out.extend_from_slice(data);
        out
    }

    /// Three files whose data is out of index order, a zero-size entry, and
    /// a name that fills its whole field.
    fn sample() -> Vec<u8> {
        let long_name = "n".repeat(NAME_FIELD_LEN);
        let data = b"{\"a\":1}\x89PNG..hello";
        build(
            MAGIC,
            VERSION,
            &[
                RawEntry::new("hello.txt", 13, 5),
                RawEntry::new("a.json", 0, 7),
                RawEntry::new("sound.fsb", 0, 0),
                RawEntry::new(&long_name, 7, 6),
            ],
            data,
        )
    }

    #[test]
    fn reads_every_field() {
        let archive = Archive::open(sample()).unwrap();
        assert_eq!(archive.get("hello.txt"), Some(&b"hello"[..]));
        assert_eq!(archive.get("a.json"), Some(&b"{\"a\":1}"[..]));
        assert_eq!(archive.get(&"n".repeat(NAME_FIELD_LEN)), Some(&b"\x89PNG.."[..]));
        assert_eq!(archive.len(), 3);
        let mut names: Vec<_> = archive.names().collect();
        names.sort_unstable();
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], "a.json");
        // A zero-size entry counts as absent, like a name that isn't listed.
        assert_eq!(archive.get("sound.fsb"), None);
        assert_eq!(archive.get("missing.png"), None);
    }

    #[test]
    fn an_empty_archive_opens() {
        let archive = Archive::open(build(MAGIC, VERSION, &[], &[])).unwrap();
        assert!(archive.is_empty());
    }

    #[test]
    fn every_proper_prefix_is_an_error() {
        let bytes = sample();
        assert!(Archive::open(bytes.clone()).is_ok());
        for cut in 0..bytes.len() {
            let result = Archive::open(bytes[..cut].to_vec());
            assert!(result.is_err(), "a {cut}-byte prefix opened");
        }
        // Where each part of the file is cut tells which error comes back.
        assert_eq!(
            Archive::open(bytes[..5].to_vec()).err(),
            Some(ArchiveError::HeaderTruncated { len: 5 })
        );
        assert!(matches!(
            Archive::open(bytes[..300].to_vec()),
            Err(ArchiveError::IndexTruncated { count: 4, .. })
        ));
        assert!(matches!(
            Archive::open(bytes[..bytes.len() - 1].to_vec()),
            Err(ArchiveError::EntryOutOfBounds { .. })
        ));
    }

    #[test]
    fn each_corrupt_field_has_its_own_error() {
        let one = |entry: RawEntry<'_>, data: &[u8]| build(MAGIC, VERSION, &[entry], data);

        let mut wrong_magic = MAGIC;
        wrong_magic[7] ^= 0xFF;
        let bytes = build(wrong_magic, VERSION, &[], &[]);
        assert_eq!(Archive::open(bytes).err(), Some(ArchiveError::Magic));
        // A short file that isn't an archive is still "not an archive".
        assert_eq!(Archive::open(b"PNG".to_vec()).err(), Some(ArchiveError::Magic));

        let bytes = build(MAGIC, 2, &[], &[]);
        assert_eq!(Archive::open(bytes).err(), Some(ArchiveError::Version(2)));

        let entry = RawEntry {
            name_len: 248,
            ..RawEntry::new("x", 0, 1)
        };
        assert_eq!(
            Archive::open(one(entry, b"x")).err(),
            Some(ArchiveError::NameTooLong { entry: 0, len: 248 })
        );

        let entry = RawEntry {
            name: &[0xC3, 0x28],
            ..RawEntry::new("xx", 0, 1)
        };
        assert_eq!(
            Archive::open(one(entry, b"x")).err(),
            Some(ArchiveError::NameNotUtf8 { entry: 0 })
        );

        assert_eq!(
            Archive::open(one(RawEntry::new("far.png", 3, 2), b"abcd")).err(),
            Some(ArchiveError::EntryOutOfBounds {
                name: "far.png".to_string(),
                offset: 3,
                size: 2,
                data_len: 4,
            })
        );
        // An offset and size whose sum overflows a u32.
        assert!(matches!(
            Archive::open(one(RawEntry::new("huge", u32::MAX, u32::MAX), b"abcd")),
            Err(ArchiveError::EntryOutOfBounds { .. })
        ));

        // A count far too large for the index to fit in the file.
        let mut bytes = build(MAGIC, VERSION, &[], &[]);
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            Archive::open(bytes).err(),
            Some(ArchiveError::IndexTruncated {
                count: u32::MAX,
                needed: 16 + 256 * u64::from(u32::MAX),
                len: 16,
            })
        );

        let bytes = build(
            MAGIC,
            VERSION,
            &[RawEntry::new("a.png", 0, 1), RawEntry::new("a.png", 1, 1)],
            b"ab",
        );
        assert_eq!(
            Archive::open(bytes).err(),
            Some(ArchiveError::DuplicateName("a.png".to_string()))
        );
        let bytes = build(
            MAGIC,
            VERSION,
            &[RawEntry::new("b.fsb", 0, 0), RawEntry::new("b.fsb", 0, 1)],
            b"b",
        );
        assert_eq!(
            Archive::open(bytes).err(),
            Some(ArchiveError::DuplicateName("b.fsb".to_string()))
        );
    }
}
