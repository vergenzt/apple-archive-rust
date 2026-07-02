//! A pure-Rust reader and writer for **Apple Archive** files (`.aar`, magic
//! `AA01`, and the legacy `YAA1`), ported from libNeoAppleArchive.
//!
//! The crate covers the core Apple Archive format:
//!
//! * Parsing and building entry [`Header`]s and their typed [`FieldValue`]s.
//! * Reading and writing uncompressed [`PlainArchive`]s.
//! * Reading and writing compressed containers ([`Archive`]) — raw, LZFSE
//!   (`pbze`), and zlib (`pbzz`). LZBITMAP (`pbzb`) is recognized but not
//!   implemented.
//! * Building an archive from a directory tree and extracting one back to disk,
//!   preserving mode/owner/xattrs on Unix.
//!
//! Apple Encrypted Archive (AEA) format is not yet supported.
//!
//! # Examples
//!
//! Round-trip a small archive in memory:
//!
//! ```
//! use apple_archive::{Header, ArchiveItem, PlainArchive, FieldKey, Uint};
//!
//! let mut header = Header::new();
//! header.set_uint(FieldKey::TYP, Uint::Size1(b'F')).unwrap();
//! header.set_string(FieldKey::PAT, "hello.txt").unwrap();
//! header.set_blob(FieldKey::DAT, 0, 5).unwrap();
//! let item = ArchiveItem::with_blob(header, b"hello".to_vec());
//!
//! let archive = PlainArchive::new(vec![item]);
//! let bytes = archive.encode().unwrap();
//!
//! let parsed = PlainArchive::from_bytes(&bytes).unwrap();
//! assert_eq!(parsed, archive);
//! ```

#![feature(read_array)]

pub mod archive;
pub mod compression;
pub mod error;
pub mod field;
mod fs;
pub mod header;

pub use archive::{ArchiveItem, PlainArchive};
pub use compression::{Archive, Compression};
pub use error::{Error, Result};
pub use field::{Blob, FieldValue, FieldKey, Hash, Timespec, Uint};
pub use header::{Entry, Header};

use std::path::Path;

/// Extract an Apple Archive file to a directory, handling raw and compressed
/// containers automatically.
pub fn extract_to_path(
    archive_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<()> {
    let archive = Archive::from_path(archive_path)?;
    archive.extract_to_dir(output_path)
}
