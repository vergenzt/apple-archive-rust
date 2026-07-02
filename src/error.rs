//! Error and result types for the crate.

use std::fmt;
use std::path::PathBuf;

use crate::compression::Compression;
use crate::field::FieldKey;

/// A specialized [`Result`](std::result::Result) alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced while reading, writing, or manipulating Apple Archives.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An underlying I/O error.
    Io(std::io::Error),
    /// The magic bytes did not match `AA01` / `YAA1` (or a `pbz*` compressed
    /// container).
    BadMagic,
    /// The buffer ended before a complete structure could be read.
    Truncated,
    /// A field used a subtype byte that is not part of the format.
    InvalidSubtype(u8),
    /// A field value used a size the format does not allow for its kind.
    UnsupportedFieldSize {
        /// The offending size.
        size: usize,
    },
    /// A string field exceeded the `u16` length the format allows.
    StringTooLong(usize),
    /// The encoded header grew past the `u16` size the format allows.
    HeaderTooLarge(usize),
    /// LZFSE (de)compression failed.
    Lzfse(String),
    /// zlib (de)compression failed.
    Zlib(String),
    /// The requested compression algorithm is recognized but not implemented
    /// (currently only LZBITMAP).
    UnsupportedCompression(Compression),
    /// A required field was absent from a header during extraction.
    MissingField(FieldKey),
    /// An archive entry used a `TYP` that is not handled (only `D`, `F`, and
    /// `L` are supported).
    UnsupportedEntryType(u8),
    /// An entry's path attempted to escape the extraction root.
    PathTraversal(PathBuf),
    /// A catch-all for other malformed-data conditions.
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::BadMagic => write!(f, "data is not an Apple Archive (bad magic)"),
            Error::Truncated => write!(f, "data ended unexpectedly"),
            Error::InvalidSubtype(b) => write!(f, "invalid field subtype byte {b:#04x}"),
            Error::UnsupportedFieldSize { size } => {
                write!(f, "unsupported size {size} for field kind")
            }
            Error::StringTooLong(n) => write!(f, "string field length {n} exceeds u16::MAX"),
            Error::HeaderTooLarge(n) => write!(f, "encoded header size {n} exceeds u16::MAX"),
            Error::Lzfse(m) => write!(f, "lzfse error: {m}"),
            Error::Zlib(m) => write!(f, "zlib error: {m}"),
            Error::UnsupportedCompression(c) => write!(f, "unsupported compression: {c:?}"),
            Error::MissingField(k) => write!(f, "missing required field {k}"),
            Error::UnsupportedEntryType(t) => {
                write!(f, "unsupported entry type {:?}", *t as char)
            }
            Error::PathTraversal(p) => write!(f, "entry path escapes extraction root: {}", p.display()),
            Error::Format(m) => write!(f, "malformed archive: {m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
