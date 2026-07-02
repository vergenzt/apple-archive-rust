//! Archive items and uncompressed ("plain") archives.
//!
//! An [`ArchiveItem`] is a [`Header`] plus the concatenated blob data that
//! trails it (file contents, extended attributes, ...). A [`PlainArchive`] is
//! just an ordered list of items, and its encoding is their concatenation.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result};
use crate::field::{Field, FieldKey};
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

    /// Parse a single item from the start of `data`, returning the item and the
    /// number of bytes it consumed.
    pub fn from_bytes(data: &[u8]) -> Result<(ArchiveItem, usize)> {
        if data.len() < 6 {
            return Err(Error::Truncated);
        }
        let item_declared_len = u16::from_le_bytes([data[4], data[5]]) as usize;
        if data.len() < item_declared_len {
            return Err(Error::Truncated);
        }
        let header = Header::from_bytes(&data[..item_declared_len])?;
        let blob_len = header.blob_data_len() as usize;
        let total = item_declared_len + blob_len;
        if data.len() < total {
            return Err(Error::Truncated);
        }
        let blob_data = data[item_declared_len..total].to_vec();
        Ok((ArchiveItem { header, blob_data }, total))
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
            if let Field::Blob { blob_size, .. } = &entry.value {
                let len = *blob_size as usize;
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
        let mut items = Vec::new();
        let mut pos = 0usize;
        while pos < data.len() {
            let (item, consumed) = ArchiveItem::from_bytes(&data[pos..])?;
            if consumed == 0 {
                return Err(Error::Format("zero-length archive item".into()));
            }
            pos += consumed;
            items.push(item);
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

    /// Write the encoded archive to any [`Write`] sink.
    pub fn write<W: Write>(&self, mut writer: W) -> Result<()> {
        let buf = self.encode()?;
        writer.write_all(&buf)?;
        Ok(())
    }

    /// Write the encoded archive to a file at `path`.
    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        let buf = self.encode()?;
        fs::write(path, buf)?;
        Ok(())
    }
}
