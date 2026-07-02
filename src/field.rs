//! Field keys and values that make up an Apple Archive header.
//!
//! Each header field is encoded as three ASCII key bytes followed by a single
//! "subtype" byte that jointly encodes the field's kind and the size of its
//! value. [`FieldValue::read`] and [`FieldValue::subtype`] are the paired
//! source of truth for that mapping, used for decoding and encoding.

use std::{
    fmt,
    io::{Cursor, Read},
};

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
        [
            (self.0 >> 24) as u8,
            (self.0 >> 16) as u8,
            (self.0 >> 8) as u8,
        ]
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

/// An unsigned integer field, sized to one of the widths the format allows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Uint {
    Size1(u8),
    Size2(u16),
    Size4(u32),
    Size8(u64),
}

impl Uint {
    /// The value widened to `u64`.
    pub fn value(&self) -> u64 {
        match self {
            Uint::Size1(v) => *v as u64,
            Uint::Size2(v) => *v as u64,
            Uint::Size4(v) => *v as u64,
            Uint::Size8(v) => *v,
        }
    }

    /// The encoded width in bytes (1, 2, 4, or 8).
    pub fn byte_len(&self) -> usize {
        match self {
            Uint::Size1(_) => 1,
            Uint::Size2(_) => 2,
            Uint::Size4(_) => 4,
            Uint::Size8(_) => 8,
        }
    }

    /// Append the little-endian value bytes to `out`.
    fn write_value(&self, out: &mut Vec<u8>) {
        match self {
            Uint::Size1(v) => out.extend_from_slice(&v.to_le_bytes()),
            Uint::Size2(v) => out.extend_from_slice(&v.to_le_bytes()),
            Uint::Size4(v) => out.extend_from_slice(&v.to_le_bytes()),
            Uint::Size8(v) => out.extend_from_slice(&v.to_le_bytes()),
        }
    }
}

/// A blob reference, whose trailing data length is stored in a header size
/// field of one of the widths the format allows.
///
/// Each variant carries the length of the trailing blob data; the variant
/// itself selects the width used to encode that length in the header, so the
/// width and the value can never disagree.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Blob {
    Size2(u16),
    Size4(u32),
    Size8(u64),
}

impl Blob {
    /// The length of the trailing blob data.
    pub fn blob_size(&self) -> u64 {
        match self {
            Blob::Size2(v) => *v as u64,
            Blob::Size4(v) => *v as u64,
            Blob::Size8(v) => *v,
        }
    }

    /// The encoded width of the size field in bytes (2, 4, or 8).
    pub fn blob_size_width(&self) -> usize {
        match self {
            Blob::Size2(_) => 2,
            Blob::Size4(_) => 4,
            Blob::Size8(_) => 8,
        }
    }

    /// Build a blob from its size-field width (2, 4, or 8) and data length,
    /// erroring if the length does not fit the chosen width.
    pub(crate) fn from_width(size_bytes: usize, blob_size: u64) -> Result<Blob> {
        let too_big = || Error::UnsupportedFieldSize { size: size_bytes };
        Ok(match size_bytes {
            2 => Blob::Size2(blob_size.try_into().map_err(|_| too_big())?),
            4 => Blob::Size4(blob_size.try_into().map_err(|_| too_big())?),
            8 => Blob::Size8(blob_size),
            n => return Err(Error::UnsupportedFieldSize { size: n }),
        })
    }

    /// Append the little-endian size bytes to `out`.
    fn write_value(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.blob_size().to_le_bytes()[..self.blob_size_width()]);
    }
}

/// A raw hash digest, sized to one of the fixed lengths the format allows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hash {
    Size4([u8; 4]),
    Size20([u8; 20]),
    Size32([u8; 32]),
    Size48([u8; 48]),
    Size64([u8; 64]),
}

impl Hash {
    /// The digest bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Hash::Size4(b) => b,
            Hash::Size20(b) => b,
            Hash::Size32(b) => b,
            Hash::Size48(b) => b,
            Hash::Size64(b) => b,
        }
    }

}

/// A timespec value, in one of the two forms the format allows.
///
/// Each variant owns a fixed-size array of raw bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Timespec {
    /// 8-byte form: seconds (subtype `S`).
    Size8([u8; 8]),
    /// 12-byte form: seconds and nanoseconds (subtype `T`).
    Size12([u8; 12]),
}

impl Timespec {
    /// The raw timespec bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Timespec::Size8(b) => b,
            Timespec::Size12(b) => b,
        }
    }
}

/// A "string" field: raw bytes (not required to be UTF-8) whose length always
/// fits the `u16` length prefix the format uses. The invariant is enforced at
/// construction, so any `String` is guaranteed encodable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct String {
    buf: Box<[u8]>,
}

impl std::ops::Deref for String {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buf
    }
}

impl TryFrom<Vec<u8>> for String {
    type Error = Error;

    fn try_from(value: Vec<u8>) -> Result<Self> {
        if value.len() > u16::MAX as usize {
            return Err(Error::StringTooLong(value.len()));
        }
        Ok(Self { buf: value.into_boxed_slice() })
    }
}

impl TryFrom<&mut Cursor<&[u8]>> for String {
    type Error = Error;

    fn try_from(cursor: &mut Cursor<&[u8]>) -> Result<String> {
        let len = u16::from_le_bytes(cursor.read_array()?) as usize;
        let mut buf = vec![0u8; len];
        cursor.read_exact(&mut buf)?;
        Ok(String { buf: buf.into_boxed_slice() })
    }
}

/// A decoded field value, carrying enough information to re-encode it exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FieldValue {
    /// A valueless flag.
    Flag,
    /// An unsigned integer; see [`Uint`] for the allowed widths.
    Uint(Uint),
    /// A blob reference; see [`Blob`] for the allowed size-field widths.
    Blob(Blob),
    /// A raw hash digest; see [`Hash`](enum@Hash) for the allowed lengths.
    Hash(Hash),
    /// A timespec value; see [`Timespec`] for the allowed forms.
    Timespec(Timespec),
    /// A length-prefixed string (raw bytes; not required to be UTF-8).
    String(String),
}

impl FieldValue {
    /// The subtype byte used to encode this value.
    ///
    /// Every variant maps to exactly one subtype byte; the enum types make
    /// unrepresentable `(kind, size)` pairings impossible to construct.
    pub(crate) fn subtype(&self) -> u8 {
        match self {
            FieldValue::Flag => b'*',
            FieldValue::Uint(uint) => match uint {
                Uint::Size1(_) => b'1',
                Uint::Size2(_) => b'2',
                Uint::Size4(_) => b'4',
                Uint::Size8(_) => b'8',
            },
            FieldValue::Blob(blob) => match blob {
                Blob::Size2(_) => b'A',
                Blob::Size4(_) => b'B',
                Blob::Size8(_) => b'C',
            },
            FieldValue::Hash(hash) => match hash {
                Hash::Size4(_) => b'F',
                Hash::Size20(_) => b'G',
                Hash::Size32(_) => b'H',
                Hash::Size48(_) => b'I',
                Hash::Size64(_) => b'J',
            },
            FieldValue::Timespec(timespec) => match timespec {
                Timespec::Size8(_) => b'S',
                Timespec::Size12(_) => b'T',
            },
            FieldValue::String(_) => b'P',
        }
    }

    /// The logical size reported for this field: the string byte length, the
    /// uint width, the blob size-field width, or the fixed hash/timespec length.
    pub fn size(&self) -> usize {
        match self {
            FieldValue::Flag => 0,
            FieldValue::Uint(u) => u.byte_len(),
            FieldValue::Blob(b) => b.blob_size_width(),
            FieldValue::Hash(h) => h.as_bytes().len(),
            FieldValue::Timespec(t) => t.as_bytes().len(),
            FieldValue::String(s) => s.len(),
        }
    }

    /// Number of value bytes this field occupies in the encoded stream,
    /// excluding the fixed 4-byte key+subtype prefix. For strings this includes
    /// the 2-byte length prefix.
    pub(crate) fn encoded_value_len(&self) -> usize {
        match self {
            FieldValue::String(s) => 2 + s.len(),
            other => other.size(),
        }
    }

    /// Append the value bytes (after the key+subtype prefix) to `out`.
    pub(crate) fn write_value(&self, out: &mut Vec<u8>) {
        match self {
            FieldValue::Flag => {}
            FieldValue::Uint(u) => u.write_value(out),
            FieldValue::Blob(b) => b.write_value(out),
            FieldValue::String(s) => {
                out.extend_from_slice(&(s.len() as u16).to_le_bytes());
                out.extend_from_slice(s);
            }
            FieldValue::Hash(h) => out.extend_from_slice(h.as_bytes()),
            FieldValue::Timespec(t) => out.extend_from_slice(t.as_bytes()),
        }
    }

    /// Decode a single field value from the cursor, given that the key bytes
    /// have already been consumed. Reads the subtype byte and then exactly the
    /// bytes that subtype implies.
    pub(crate) fn read(cursor: &mut Cursor<&[u8]>) -> Result<FieldValue> {
        let subtype = cursor.read_array::<1>()?[0];
        Ok(match subtype {
            b'*' => FieldValue::Flag,
            b'1' => FieldValue::Uint(Uint::Size1(u8::from_le_bytes(cursor.read_array()?))),
            b'2' => FieldValue::Uint(Uint::Size2(u16::from_le_bytes(cursor.read_array()?))),
            b'4' => FieldValue::Uint(Uint::Size4(u32::from_le_bytes(cursor.read_array()?))),
            b'8' => FieldValue::Uint(Uint::Size8(u64::from_le_bytes(cursor.read_array()?))),
            b'A' => FieldValue::Blob(Blob::Size2(u16::from_le_bytes(cursor.read_array()?))),
            b'B' => FieldValue::Blob(Blob::Size4(u32::from_le_bytes(cursor.read_array()?))),
            b'C' => FieldValue::Blob(Blob::Size8(u64::from_le_bytes(cursor.read_array()?))),
            b'F' => FieldValue::Hash(Hash::Size4(cursor.read_array()?)),
            b'G' => FieldValue::Hash(Hash::Size20(cursor.read_array()?)),
            b'H' => FieldValue::Hash(Hash::Size32(cursor.read_array()?)),
            b'I' => FieldValue::Hash(Hash::Size48(cursor.read_array()?)),
            b'J' => FieldValue::Hash(Hash::Size64(cursor.read_array()?)),
            b'S' => FieldValue::Timespec(Timespec::Size8(cursor.read_array()?)),
            b'T' => FieldValue::Timespec(Timespec::Size12(cursor.read_array()?)),
            b'P' => FieldValue::String(String::try_from(cursor)?),
            _ => return Err(Error::InvalidSubtype(subtype)),
        })
    }
}
