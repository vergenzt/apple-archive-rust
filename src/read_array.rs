//! Local vendoring of the unstable `Read::read_array` method so the crate
//! builds on stable Rust instead of requiring a nightly toolchain.
//!
//! Upstream tracking issue: <https://github.com/rust-lang/rust/issues/148848>
//! Upstream implementation: `library/std/src/io/mod.rs`, `Read::read_array`.
//!
//! The upstream body reads into an uninitialized array via `BorrowedBuf` /
//! `read_buf_exact` / `MaybeUninit::array_assume_init` (all themselves
//! unstable). We reproduce the same observable behavior on stable by reading
//! into a zero-initialized array with `read_exact`; like upstream, a short
//! read returns `ErrorKind::UnexpectedEof`.

use std::io::{self, Read};

/// Extension trait providing the vendored [`read_array`](ReadArrayExt::read_array).
pub(crate) trait ReadArrayExt: Read {
    /// Read and return a fixed array of bytes from this source.
    ///
    /// See module docs; mirrors the unstable `Read::read_array`
    /// (rust-lang/rust#148848).
    fn read_array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut buf = [0u8; N];
        self.read_exact(&mut buf)?;
        Ok(buf)
    }
}

impl<R: Read + ?Sized> ReadArrayExt for R {}

#[cfg(test)]
mod tests {
    use super::ReadArrayExt;
    use std::io::{Cursor, ErrorKind};

    #[test]
    fn reads_exact_and_advances() {
        let mut buf = Cursor::new([1u8, 2, 3, 4, 5, 6]);
        assert_eq!(buf.read_array::<4>().unwrap(), [1, 2, 3, 4]);
        assert_eq!(buf.read_array::<2>().unwrap(), [5, 6]);
    }

    #[test]
    fn short_read_is_unexpected_eof() {
        let mut buf = Cursor::new([1u8, 2]);
        let err = buf.read_array::<4>().unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
    }
}
