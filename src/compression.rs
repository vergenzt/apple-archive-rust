//! Compressed-container handling (the `pbz*` formats) plus the [`Archive`]
//! type.
//!
//! Supported on read: raw (`AA01`/`YAA1`), `pbze` (LZFSE), `pbzz` (zlib).
//! Supported on write: raw, LZFSE, zlib. `pbzb` (LZBITMAP) is recognized but
//! unimplemented and yields [`Error::UnsupportedCompression`].

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::archive::PlainArchive;
use crate::error::{Error, Result};
use crate::header::{AAR_MAGIC, YAA_MAGIC};

/// The compression algorithm of an Apple Archive container.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Compression {
    /// Uncompressed (`AA01`/`YAA1`).
    None,
    /// LZFSE (`pbze`).
    Lzfse,
    /// zlib/DEFLATE (`pbzz`).
    Zlib,
    /// LZBITMAP (`pbzb`) — recognized but unimplemented.
    Lzbitmap,
}

impl Compression {
    /// The `pbz*` magic character for compressed variants, if any.
    fn magic_char(self) -> Option<u8> {
        match self {
            Compression::None => None,
            Compression::Lzfse => Some(b'e'),
            Compression::Zlib => Some(b'z'),
            Compression::Lzbitmap => Some(b'b'),
        }
    }
}

/// A decoded archive plus the compression metadata it was read with.
#[derive(Clone, Debug)]
pub struct Archive {
    /// The decompressed, parsed archive.
    pub plain: PlainArchive,
    /// The compression the container used.
    pub compression: Compression,
    /// Size of the uncompressed payload in bytes.
    pub uncompressed_size: usize,
    /// Size of the compressed payload in bytes.
    pub compressed_size: usize,
}

/// The 28-byte (`0x1C`) header prepended to single-block compressed archives.
const PBZX_HEADER_LEN: usize = 0x1C;

impl Archive {
    /// Detect the container format of `data`, decompress if needed, and parse
    /// the resulting plain archive.
    pub fn from_bytes(data: &[u8]) -> Result<Archive> {
        if data.len() < 4 {
            return Err(Error::Truncated);
        }
        let magic = &data[0..4];
        if magic == AAR_MAGIC || magic == YAA_MAGIC {
            let plain = PlainArchive::from_bytes(data)?;
            return Ok(Archive {
                plain,
                compression: Compression::None,
                uncompressed_size: data.len(),
                compressed_size: data.len(),
            });
        }
        if &data[0..3] == b"pbz" {
            let ctype = data[3];
            return match ctype {
                b'e' => decode_lzfse(data),
                b'z' => decode_zlib(data),
                b'b' => Err(Error::UnsupportedCompression(Compression::Lzbitmap)),
                other => Err(Error::Format(format!(
                    "unknown pbz compression type {:?}",
                    other as char
                ))),
            };
        }
        Err(Error::BadMagic)
    }

    /// Read and decode an archive from a file.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Archive> {
        let data = fs::read(path)?;
        Archive::from_bytes(&data)
    }
}

/// Decode a `pbze` (LZFSE) container.
///
/// The layout is a 12-byte file prefix followed by one or more blocks, each a
/// 16-byte header (`uncompressed` big-endian at `+4`, `compressed` big-endian
/// at `+12`) and then that many compressed bytes.
fn decode_lzfse(data: &[u8]) -> Result<Archive> {
    const FILE_PREFIX: usize = 0xC;
    const BLOCK_HEADER: usize = 0x10;

    let mut out = Vec::new();
    let mut total_compressed = 0usize;
    let mut pos = FILE_PREFIX;

    while pos < data.len() {
        if pos + BLOCK_HEADER > data.len() {
            return Err(Error::Format("lzfse: block header past end of buffer".into()));
        }
        let unc = be_u32(&data[pos + 0x4..pos + 0x8]) as usize;
        let comp = be_u32(&data[pos + 0xC..pos + 0x10]) as usize;
        let block_end = pos + BLOCK_HEADER + comp;
        if block_end > data.len() {
            return Err(Error::Format("lzfse: block data past end of buffer".into()));
        }
        let block = &data[pos + BLOCK_HEADER..block_end];

        let before = out.len();
        lzfse_rust::decode_bytes(block, &mut out)
            .map_err(|e| Error::Lzfse(format!("{e:?}")))?;
        if out.len() - before != unc {
            return Err(Error::Lzfse(format!(
                "block decompressed to {} bytes, expected {}",
                out.len() - before,
                unc
            )));
        }

        total_compressed += comp;
        pos = block_end;
    }

    let uncompressed_size = out.len();
    let plain = PlainArchive::from_bytes(&out)?;
    Ok(Archive {
        plain,
        compression: Compression::Lzfse,
        uncompressed_size,
        compressed_size: total_compressed,
    })
}

/// Decode a `pbzz` (zlib) container: sizes are big-endian, uncompressed at
/// `0x10`, compressed at `0x18`, and the zlib stream begins at `0x1C`.
fn decode_zlib(data: &[u8]) -> Result<Archive> {
    if data.len() < PBZX_HEADER_LEN {
        return Err(Error::Truncated);
    }
    let uncompressed_size = be_u32(&data[0x10..0x14]) as usize;
    let compressed_size = be_u32(&data[0x18..0x1C]) as usize;
    let stream = &data[PBZX_HEADER_LEN..];
    let raw = zlib_decompress(stream, uncompressed_size)?;
    let plain = PlainArchive::from_bytes(&raw)?;
    Ok(Archive {
        plain,
        compression: Compression::Zlib,
        uncompressed_size,
        compressed_size,
    })
}

impl PlainArchive {
    /// Serialize this archive with the given compression, returning the encoded
    /// container bytes.
    pub fn encode_compressed(&self, compression: Compression) -> Result<Vec<u8>> {
        let raw = self.encode()?;
        match compression {
            Compression::None => Ok(raw),
            Compression::Lzfse => {
                let mut compressed = Vec::new();
                lzfse_rust::encode_bytes(&raw, &mut compressed)
                    .map_err(|e| Error::Lzfse(format!("{e:?}")))?;
                Ok(build_pbzx_container(Compression::Lzfse, raw.len(), &compressed))
            }
            Compression::Zlib => {
                let compressed = zlib_compress(&raw)?;
                Ok(build_pbzx_container(Compression::Zlib, raw.len(), &compressed))
            }
            Compression::Lzbitmap => Err(Error::UnsupportedCompression(Compression::Lzbitmap)),
        }
    }

    /// Write this archive, compressed, to a file at `path`.
    pub fn write_compressed_to_path(
        &self,
        compression: Compression,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let buf = self.encode_compressed(compression)?;
        fs::write(path, buf)?;
        Ok(())
    }
}

/// Assemble a single-block `pbz*` container: the 28-byte header followed by the
/// compressed payload.
fn build_pbzx_container(compression: Compression, uncompressed: usize, compressed: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; PBZX_HEADER_LEN];
    out[0] = b'p';
    out[1] = b'b';
    out[2] = b'z';
    out[3] = compression.magic_char().expect("compressed variant has magic");
    out[9] = 0x40; // Fixed marker byte, always 0x40.
    out[0x10..0x14].copy_from_slice(&(uncompressed as u32).to_be_bytes());
    out[0x18..0x1C].copy_from_slice(&(compressed.len() as u32).to_be_bytes());
    out.extend_from_slice(compressed);
    out
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn zlib_compress(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::Compression as FlateLevel;
    use flate2::write::ZlibEncoder;
    let mut encoder = ZlibEncoder::new(Vec::new(), FlateLevel::best());
    encoder.write_all(data).map_err(|e| Error::Zlib(e.to_string()))?;
    encoder.finish().map_err(|e| Error::Zlib(e.to_string()))
}

fn zlib_decompress(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(expected);
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::Zlib(e.to_string()))?;
    Ok(out)
}
