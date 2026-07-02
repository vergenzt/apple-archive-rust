//! Field keys and values that make up an Apple Archive header.
//!
//! Each header field is encoded as three ASCII key bytes followed by a single
//! "subtype" byte that jointly encodes the field's kind and the size of its
//! value. A single `FIELD_KINDS` table is the one source of truth for that
//! mapping, used for both decoding and encoding.

use std::fmt;

use crate::error::{Error, Result};

/// A three-character field key such as `TYP`, `PAT`, or `DAT`.
///
/// Stored as a `u32` whose top three bytes hold the ASCII characters and whose
/// low byte is zero. Keys are an open set — archives may contain keys beyond the
/// named constants below — so this is a plain `u32` newtype rather than an enum.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct FieldKey(pub u32);

impl FieldKey {
    /// Construct a key from a three-byte ASCII literal.
    pub const fn from_ascii(bytes: &[u8; 3]) -> Self {
        FieldKey((bytes[0] as u32) << 24 | (bytes[1] as u32) << 16 | (bytes[2] as u32) << 8)
    }

    /// Construct a key from its raw `u32` representation.
    pub const fn from_u32(value: u32) -> Self {
        FieldKey(value)
    }

    /// The raw `u32` representation (top three bytes are ASCII, low byte zero).
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// The three ASCII key bytes.
    pub const fn as_bytes(self) -> [u8; 3] {
        [(self.0 >> 24) as u8, (self.0 >> 16) as u8, (self.0 >> 8) as u8]
    }

    // ---- Commonly used keys --------------------

    /// Entry type (`D`irectory, `F`ile, `L`ink).
    pub const TYP: FieldKey = FieldKey::from_ascii(b"TYP");
    /// Relative path of the entry.
    pub const PAT: FieldKey = FieldKey::from_ascii(b"PAT");
    /// File data blob.
    pub const DAT: FieldKey = FieldKey::from_ascii(b"DAT");
    /// Symlink target.
    pub const LNK: FieldKey = FieldKey::from_ascii(b"LNK");
    /// Owning user id.
    pub const UID: FieldKey = FieldKey::from_ascii(b"UID");
    /// Owning group id.
    pub const GID: FieldKey = FieldKey::from_ascii(b"GID");
    /// Access mode / permission bits.
    pub const MOD: FieldKey = FieldKey::from_ascii(b"MOD");
    /// Extended-attribute blob.
    pub const XAT: FieldKey = FieldKey::from_ascii(b"XAT");
}

impl fmt::Debug for FieldKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FieldKey({self})")
    }
}

impl fmt::Display for FieldKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.as_bytes() {
            write!(f, "{}", b as char)?;
        }
        Ok(())
    }
}

/// The kind of a field, used to drive the `FIELD_KINDS` table.
#[derive(Copy, Clone, PartialEq, Eq)]
enum FieldKind {
    Flag,
    Uint,
    Blob,
    String,
    Hash,
    Timespec,
}

/// The single source of truth mapping a subtype byte to its `(FieldKind, size)`.
///
/// Decoding scans by byte; encoding scans by `(FieldKind, size)`. For strings
/// the size is `0` because the real length is stored inline as a `u16`.
const FIELD_KINDS: &[(u8, FieldKind, usize)] = &[
    (b'*', FieldKind::Flag, 0),
    (b'1', FieldKind::Uint, 1),
    (b'2', FieldKind::Uint, 2),
    (b'4', FieldKind::Uint, 4),
    (b'8', FieldKind::Uint, 8),
    (b'A', FieldKind::Blob, 2),
    (b'B', FieldKind::Blob, 4),
    (b'C', FieldKind::Blob, 8),
    (b'F', FieldKind::Hash, 4),
    (b'G', FieldKind::Hash, 20),
    (b'H', FieldKind::Hash, 32),
    (b'I', FieldKind::Hash, 48),
    (b'J', FieldKind::Hash, 64),
    (b'S', FieldKind::Timespec, 8),
    (b'T', FieldKind::Timespec, 12),
    (b'P', FieldKind::String, 0),
];

/// An unsigned integer field, sized to one of the widths the format allows.
///
/// Each variant stores a native integer of the matching width, so the size and
/// value can never disagree.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Uint {
    /// 1-byte value (subtype `1`).
    U8(u8),
    /// 2-byte value (subtype `2`).
    U16(u16),
    /// 4-byte value (subtype `4`).
    U32(u32),
    /// 8-byte value (subtype `8`).
    U64(u64),
}

impl Uint {
    /// The value widened to `u64`.
    pub fn value(&self) -> u64 {
        match self {
            Uint::U8(v) => *v as u64,
            Uint::U16(v) => *v as u64,
            Uint::U32(v) => *v as u64,
            Uint::U64(v) => *v,
        }
    }

    /// The encoded width in bytes (1, 2, 4, or 8).
    pub fn byte_len(&self) -> usize {
        match self {
            Uint::U8(_) => 1,
            Uint::U16(_) => 2,
            Uint::U32(_) => 4,
            Uint::U64(_) => 8,
        }
    }

    /// Append the little-endian value bytes to `out`.
    fn write_value(&self, out: &mut Vec<u8>) {
        match self {
            Uint::U8(v) => out.extend_from_slice(&v.to_le_bytes()),
            Uint::U16(v) => out.extend_from_slice(&v.to_le_bytes()),
            Uint::U32(v) => out.extend_from_slice(&v.to_le_bytes()),
            Uint::U64(v) => out.extend_from_slice(&v.to_le_bytes()),
        }
    }
}

impl TryFrom<&[u8]> for Uint {
    type Error = Error;

    /// Build a [`Uint`] from little-endian bytes whose length is 1, 2, 4, or 8;
    /// the length selects the width.
    fn try_from(bytes: &[u8]) -> Result<Uint> {
        Ok(match bytes.len() {
            1 => Uint::U8(bytes[0]),
            2 => Uint::U16(u16::from_le_bytes(bytes.try_into().unwrap())),
            4 => Uint::U32(u32::from_le_bytes(bytes.try_into().unwrap())),
            8 => Uint::U64(u64::from_le_bytes(bytes.try_into().unwrap())),
            n => return Err(Error::UnsupportedFieldSize { size: n }),
        })
    }
}

/// A raw hash digest, sized to one of the fixed lengths the format allows.
///
/// Each variant owns a fixed-size array, so an out-of-range length cannot be
/// constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hash {
    /// 4-byte digest (subtype `F`).
    B4([u8; 4]),
    /// 20-byte digest, e.g. SHA-1 (subtype `G`).
    B20([u8; 20]),
    /// 32-byte digest, e.g. SHA-256 (subtype `H`).
    B32([u8; 32]),
    /// 48-byte digest, e.g. SHA-384 (subtype `I`).
    B48([u8; 48]),
    /// 64-byte digest, e.g. SHA-512 (subtype `J`).
    B64([u8; 64]),
}

impl Hash {
    /// The digest bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Hash::B4(b) => b,
            Hash::B20(b) => b,
            Hash::B32(b) => b,
            Hash::B48(b) => b,
            Hash::B64(b) => b,
        }
    }

    /// The digest length in bytes.
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Whether the digest has no bytes (never true; a digest is always sized).
    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }
}

impl TryFrom<&[u8]> for Hash {
    type Error = Error;

    /// Build a [`Hash`](enum@Hash) from bytes whose length is one of 4, 20, 32, 48, or 64.
    fn try_from(bytes: &[u8]) -> Result<Hash> {
        Ok(match bytes.len() {
            4 => Hash::B4(bytes.try_into().unwrap()),
            20 => Hash::B20(bytes.try_into().unwrap()),
            32 => Hash::B32(bytes.try_into().unwrap()),
            48 => Hash::B48(bytes.try_into().unwrap()),
            64 => Hash::B64(bytes.try_into().unwrap()),
            n => return Err(Error::UnsupportedFieldSize { size: n }),
        })
    }
}

/// A timespec value, in one of the two forms the format allows.
///
/// Each variant owns a fixed-size array of raw bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Timespec {
    /// 8-byte form: seconds (subtype `S`).
    B8([u8; 8]),
    /// 12-byte form: seconds and nanoseconds (subtype `T`).
    B12([u8; 12]),
}

impl Timespec {
    /// The raw timespec bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Timespec::B8(b) => b,
            Timespec::B12(b) => b,
        }
    }

    /// The length in bytes (8 or 12).
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Whether there are no bytes (never true; a timespec is always sized).
    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }
}

impl TryFrom<&[u8]> for Timespec {
    type Error = Error;

    /// Build a [`Timespec`] from bytes whose length is 8 or 12.
    fn try_from(bytes: &[u8]) -> Result<Timespec> {
        Ok(match bytes.len() {
            8 => Timespec::B8(bytes.try_into().unwrap()),
            12 => Timespec::B12(bytes.try_into().unwrap()),
            n => return Err(Error::UnsupportedFieldSize { size: n }),
        })
    }
}

/// A decoded field value, carrying enough information to re-encode it exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    /// A valueless flag (subtype `*`).
    Flag,
    /// An unsigned integer; see [`Uint`] for the allowed widths.
    Uint(Uint),
    /// A blob reference: `blob_size` bytes of data follow the header, and the
    /// size itself is stored in `size` bytes within the header.
    Blob {
        /// Encoded width of the size field (2, 4, or 8).
        size: u8,
        /// Length of the trailing blob data.
        blob_size: u64,
    },
    /// A length-prefixed string (raw bytes; not required to be UTF-8).
    String(Vec<u8>),
    /// A raw hash digest; see [`Hash`](enum@Hash) for the allowed lengths.
    Hash(Hash),
    /// A timespec value; see [`Timespec`] for the allowed forms.
    Timespec(Timespec),
}

impl Field {
    /// The `(FieldKind, size)` used to look this value up in `FIELD_KINDS`.
    fn table_key(&self) -> (FieldKind, usize) {
        match self {
            Field::Flag => (FieldKind::Flag, 0),
            Field::Uint(u) => (FieldKind::Uint, u.byte_len()),
            Field::Blob { size, .. } => (FieldKind::Blob, *size as usize),
            Field::String(_) => (FieldKind::String, 0),
            Field::Hash(h) => (FieldKind::Hash, h.len()),
            Field::Timespec(t) => (FieldKind::Timespec, t.len()),
        }
    }

    /// The logical size reported for this field.
    pub fn size(&self) -> usize {
        match self {
            Field::String(s) => s.len(),
            other => other.table_key().1,
        }
    }

    /// The subtype byte used to encode this value.
    ///
    /// Only ever called after [`Field::validate`] has confirmed the value is
    /// representable, so the table lookup always succeeds.
    pub(crate) fn subtype(&self) -> u8 {
        let (kind, size) = self.table_key();
        FIELD_KINDS
            .iter()
            .find(|(_, k, s)| *k == kind && *s == size)
            .map(|(b, _, _)| *b)
            .expect("validate() guarantees a representable (kind, size)")
    }

    /// Number of value bytes this field occupies in the encoded stream,
    /// excluding the fixed 4-byte key+subtype prefix. For strings this includes
    /// the 2-byte length prefix.
    pub(crate) fn encoded_value_len(&self) -> usize {
        match self {
            Field::String(s) => 2 + s.len(),
            other => other.table_key().1,
        }
    }

    /// Append the value bytes (after the key+subtype prefix) to `out`.
    pub(crate) fn write_value(&self, out: &mut Vec<u8>) {
        match self {
            Field::Flag => {}
            Field::Uint(u) => u.write_value(out),
            Field::Blob { size, blob_size } => {
                out.extend_from_slice(&blob_size.to_le_bytes()[..*size as usize]);
            }
            Field::String(s) => {
                out.extend_from_slice(&(s.len() as u16).to_le_bytes());
                out.extend_from_slice(s);
            }
            Field::Hash(h) => out.extend_from_slice(h.as_bytes()),
            Field::Timespec(t) => out.extend_from_slice(t.as_bytes()),
        }
    }

    /// Validate that this value is representable.
    ///
    /// A value is representable exactly when its `(kind, size)` appears in the
    /// `FIELD_KINDS` table (with strings additionally bounded by `u16`, and
    /// timespec byte lengths matching their declared size).
    pub(crate) fn validate(&self) -> Result<()> {
        let ok = match self {
            Field::String(s) => s.len() <= u16::MAX as usize,
            // Uint, Hash, and Timespec are valid by construction.
            Field::Uint(_) | Field::Hash(_) | Field::Timespec(_) => true,
            other => in_table(other.table_key()),
        };
        if ok {
            Ok(())
        } else {
            Err(Error::UnsupportedFieldSize { size: self.size() })
        }
    }

    /// Decode a single field value from `data` starting at `pos`, given its
    /// subtype byte. Returns the value and the position just past it.
    ///
    /// `end` bounds the header, so a malformed length can never read past it.
    pub(crate) fn decode(sub: u8, data: &[u8], mut pos: usize, end: usize) -> Result<(Field, usize)> {
        let (kind, size) = FIELD_KINDS
            .iter()
            .find(|(b, _, _)| *b == sub)
            .map(|(_, k, s)| (*k, *s))
            .ok_or(Error::InvalidSubtype(sub))?;

        let value = match kind {
            FieldKind::Flag => Field::Flag,
            FieldKind::String => {
                if pos + 2 > end {
                    return Err(Error::Truncated);
                }
                let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                if pos + len > end {
                    return Err(Error::Truncated);
                }
                let s = data[pos..pos + len].to_vec();
                pos += len;
                return Ok((Field::String(s), pos));
            }
            other => {
                if pos + size > end {
                    return Err(Error::Truncated);
                }
                let raw = &data[pos..pos + size];
                pos += size;
                match other {
                    FieldKind::Uint => {
                        Field::Uint(Uint::try_from(raw).expect("table size is a valid uint width"))
                    }
                    FieldKind::Blob => Field::Blob {
                        size: size as u8,
                        blob_size: read_uint_le(raw),
                    },
                    FieldKind::Hash => Field::Hash(
                        Hash::try_from(raw).expect("table size is a valid hash length"),
                    ),
                    FieldKind::Timespec => Field::Timespec(
                        Timespec::try_from(raw).expect("table size is a valid timespec length"),
                    ),
                    // Flag/String handled above.
                    FieldKind::Flag | FieldKind::String => unreachable!(),
                }
            }
        };
        Ok((value, pos))
    }
}

/// Whether a `(FieldKind, size)` pairing exists in the subtype table.
fn in_table(key: (FieldKind, usize)) -> bool {
    FIELD_KINDS.iter().any(|(_, k, s)| *k == key.0 && *s == key.1)
}

/// Read a little-endian unsigned integer from up to 8 bytes.
fn read_uint_le(bytes: &[u8]) -> u64 {
    let mut value = 0u64;
    for (i, &b) in bytes.iter().enumerate().take(8) {
        value |= (b as u64) << (8 * i);
    }
    value
}
