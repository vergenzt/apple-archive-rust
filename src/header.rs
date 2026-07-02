//! A parsed Apple Archive entry header.
//!
//! A header is an ordered list of [`Entry`]s (key + [`FieldValue`]) prefixed by
//! the `AA01`/`YAA1` magic and a `u16` total size. Entries are stored directly
//! and serialized on demand via [`Header::encode`].

use std::io::{Cursor, Read};

use crate::error::{Error, Result};
use crate::field::{Blob, FieldKey, FieldValue, Hash, Timespec, Uint};

/// Magic for modern Apple Archives (`AA01`).
pub(crate) const AAR_MAGIC: &[u8; 4] = b"AA01";
/// Magic for legacy YAA archives (`YAA1`), treated identically.
pub(crate) const YAA_MAGIC: &[u8; 4] = b"YAA1";

/// A single header entry: a key paired with its typed [`FieldValue`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The three-character field key.
    pub key: FieldKey,
    /// The entry's value.
    pub value: FieldValue,
}

/// An Apple Archive entry header — an ordered collection of [`Entry`]s.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    entries: Vec<Entry>,
}

impl Header {
    /// Create an empty header (encodes to `AA01` plus a 6-byte size prefix).
    pub fn new() -> Self {
        Header {
            entries: Vec::new(),
        }
    }

    /// Parse a header from encoded bytes.
    ///
    /// Data must begin with one of the magic sequences and be at least as long as the `u16`
    /// size stored at offset 4; trailing bytes beyond that size are ignored
    /// (they belong to the item's blob data).
    pub fn read(cursor: &mut Cursor<&[u8]>) -> Result<Header> {
        let start = cursor.position();
        let magic = cursor.read_array::<4>()?;
        if &magic != AAR_MAGIC && &magic != YAA_MAGIC {
            return Err(Error::BadMagic);
        }

        // The stored size spans the whole header, including this 6-byte prefix.
        let head_len = u16::from_le_bytes(cursor.read_array()?) as u64;
        let head_end = start + head_len;

        let mut entries = Vec::new();
        while cursor.position() < head_end {
            let key = FieldKey::from_ascii(&cursor.read_array()?);
            let value = FieldValue::read(cursor)?;
            entries.push(Entry { key, value });
        }

        // Land exactly on the declared header end, skipping any trailing padding,
        // so the caller's cursor is positioned at the start of the blob data.
        cursor.set_position(head_end);
        Ok(Header { entries })
    }

    /// The number of bytes this header serializes to.
    pub fn encoded_len(&self) -> usize {
        6 + self
            .entries
            .iter()
            .map(|e| 4 + e.value.encoded_value_len())
            .sum::<usize>()
    }

    /// Serialize this header into a fresh byte buffer.
    ///
    /// Returns [`Error::HeaderTooLarge`] if the header would exceed the `u16`
    /// size the format allows.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.encoded_len());
        self.write_into(&mut out)?;
        Ok(out)
    }

    /// Serialize this header, appending to `out`.
    pub fn write_into(&self, out: &mut Vec<u8>) -> Result<()> {
        let total = self.encoded_len();
        if total > u16::MAX as usize {
            return Err(Error::HeaderTooLarge(total));
        }
        out.extend_from_slice(AAR_MAGIC);
        out.extend_from_slice(&(total as u16).to_le_bytes());
        for entry in &self.entries {
            out.extend_from_slice(&entry.key.as_bytes());
            out.push(entry.value.subtype());
            entry.value.write_value(out);
        }
        Ok(())
    }

    /// Total size of blob data that trails this header in the archive item,
    /// i.e. the sum of every blob field's declared size.
    pub fn blob_data_len(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|e| match &e.value {
                FieldValue::Blob(b) => Some(b.blob_size()),
                _ => None,
            })
            .sum()
    }

    /// All entries in order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Index of the entry with the given key, if present.
    pub fn index_of(&self, key: FieldKey) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }

    /// Borrow the value for a key, if present.
    pub fn get(&self, key: FieldKey) -> Option<&FieldValue> {
        self.entries.iter().find(|e| e.key == key).map(|e| &e.value)
    }

    /// The declared size of an entry's value, if present.
    pub fn field_size(&self, key: FieldKey) -> Option<usize> {
        self.get(key).map(FieldValue::size)
    }

    /// Read a field as an unsigned integer.
    ///
    /// Works for `Uint` (returns the value) and `Blob` (returns the blob size).
    pub fn get_uint(&self, key: FieldKey) -> Option<u64> {
        match self.get(key)? {
            FieldValue::Uint(u) => Some(u.value()),
            FieldValue::Blob(b) => Some(b.blob_size()),
            _ => None,
        }
    }

    /// Borrow a string field's raw bytes.
    pub fn get_string(&self, key: FieldKey) -> Option<&[u8]> {
        match self.get(key)? {
            FieldValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Read a string field as a lossily-decoded [`String`].
    pub fn get_string_lossy(&self, key: FieldKey) -> Option<String> {
        self.get_string(key)
            .map(|s| String::from_utf8_lossy(s).into_owned())
    }

    /// Insert or replace an entry.
    ///
    /// Replacing a field with a value of a different size or kind is allowed,
    /// since the buffer is regenerated on encode.
    pub fn set(&mut self, key: FieldKey, value: FieldValue) -> Result<()> {
        if let Some(idx) = self.index_of(key) {
            self.entries[idx].value = value;
        } else {
            self.entries.push(Entry { key, value });
        }
        Ok(())
    }

    /// Set an unsigned-integer field.
    pub fn set_uint(&mut self, key: FieldKey, uint: Uint) -> Result<()> {
        self.set(key, FieldValue::Uint(uint))
    }

    /// Set a flag field.
    pub fn set_flag(&mut self, key: FieldKey) -> Result<()> {
        self.set(key, FieldValue::Flag)
    }

    /// Set a string field from raw bytes.
    pub fn set_string(&mut self, key: FieldKey, s: impl Into<Vec<u8>>) -> Result<()> {
        let string = crate::field::String::try_from(s.into())?;
        self.set(key, FieldValue::String(string))
    }

    /// Set a blob field.
    ///
    /// A `size` of `0` selects the smallest width (2, 4, or 8) that can hold
    /// `blob_size`.
    pub fn set_blob(&mut self, key: FieldKey, size: u8, blob_size: u64) -> Result<()> {
        let size = if size == 0 {
            if blob_size < u16::MAX as u64 {
                2
            } else if blob_size < u32::MAX as u64 {
                4
            } else {
                8
            }
        } else {
            size
        };
        self.set(
            key,
            FieldValue::Blob(Blob::from_width(size as usize, blob_size)?),
        )
    }

    /// Set a timespec field.
    pub fn set_timespec(&mut self, key: FieldKey, timespec: Timespec) -> Result<()> {
        self.set(key, FieldValue::Timespec(timespec))
    }

    /// Set a hash field.
    pub fn set_hash(&mut self, key: FieldKey, hash: Hash) -> Result<()> {
        self.set(key, FieldValue::Hash(hash))
    }

    /// Remove an entry by key, returning the value if it was present.
    pub fn remove(&mut self, key: FieldKey) -> Option<FieldValue> {
        if let Some(idx) = self.index_of(key) {
            let value = self.entries.remove(idx).value;
            Some(value)
        } else {
            None
        }
    }
}
