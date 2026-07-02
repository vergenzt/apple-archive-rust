//! Archive items and uncompressed ("plain") archives.
//!
//! An [`ArchiveItem`] is a [`Header`] plus the concatenated blob data that
//! trails it (file contents, extended attributes, ...). A [`PlainArchive`] is
//! just an ordered list of items, and its encoding is their concatenation.

use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::error::Result;
use crate::field::{FieldValue, FieldKey};
use crate::header::Header;

/// A single archive entry: a header and the blob bytes that follow it.
///
/// `blob_data` is the raw concatenation of every blob field's contents, in
/// field order (e.g. the `DAT` file data followed by the `XAT` xattr blob). Use
/// [`ArchiveItem::blob_slice`] to pull out an individual blob by key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveItem {
    /// The entry header.
    pub header: Header,
    /// Concatenated blob data referenced by the header's blob fields.
    pub blob_data: Vec<u8>,
}

impl ArchiveItem {
    /// Create an item from a header, with no blob data.
    pub fn new(header: Header) -> Self {
        ArchiveItem {
            header,
            blob_data: Vec::new(),
        }
    }

    /// Create an item from a header and its blob data.
    pub fn with_blob(header: Header, blob_data: impl Into<Vec<u8>>) -> Self {
        ArchiveItem {
            header,
            blob_data: blob_data.into(),
        }
    }

    /// Parse a single item from the cursor: its [`Header`] followed by the
    /// concatenated blob data the header's blob fields declare. The cursor is
    /// left positioned at the start of the next item.
    pub fn read(cursor: &mut Cursor<&[u8]>) -> Result<ArchiveItem> {
        let header = Header::read(cursor)?;
        let mut blob_data = vec![0u8; header.blob_data_len() as usize];
        cursor.read_exact(&mut blob_data)?;
        Ok(ArchiveItem { header, blob_data })
    }

    /// Number of bytes this item serializes to.
    pub fn encoded_len(&self) -> usize {
        self.header.encoded_len() + self.blob_data.len()
    }

    /// Serialize this item, appending to `out`.
    pub fn write_into(&self, out: &mut Vec<u8>) -> Result<()> {
        self.header.write_into(out)?;
        out.extend_from_slice(&self.blob_data);
        Ok(())
    }

    /// Borrow the slice of `blob_data` belonging to a particular blob field.
    ///
    /// Blob fields are laid out in header order; this walks that order to find
    /// the offset of `key`.
    pub fn blob_slice(&self, key: FieldKey) -> Option<&[u8]> {
        let mut offset = 0usize;
        for entry in self.header.entries() {
            if let FieldValue::Blob(b) = &entry.value {
                let len = b.blob_size() as usize;
                if entry.key == key {
                    return self.blob_data.get(offset..offset + len);
                }
                offset += len;
            }
        }
        None
    }
}

/// An uncompressed Apple Archive: an ordered list of [`ArchiveItem`]s.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlainArchive {
    /// The entries in this archive.
    pub items: Vec<ArchiveItem>,
}

impl PlainArchive {
    /// Create an archive from a list of items.
    pub fn new(items: Vec<ArchiveItem>) -> Self {
        PlainArchive { items }
    }

    /// Parse a plain archive from its full encoded bytes.
    pub fn from_bytes(data: &[u8]) -> Result<PlainArchive> {
        let mut cursor = Cursor::new(data);
        let mut items = Vec::new();
        while (cursor.position() as usize) < data.len() {
            items.push(ArchiveItem::read(&mut cursor)?);
        }
        Ok(PlainArchive { items })
    }

    /// Total encoded size of the archive.
    pub fn encoded_len(&self) -> usize {
        self.items.iter().map(ArchiveItem::encoded_len).sum()
    }

    /// Serialize the archive to a fresh byte buffer.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.encoded_len());
        for item in &self.items {
            item.write_into(&mut out)?;
        }
        Ok(out)
    }

    /// Write the encoded archive to a file at `path`.
    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        let buf = self.encode()?;
        fs::write(path, buf)?;
        Ok(())
    }
}
