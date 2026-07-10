# apple-archive

An idiomatic Rust library for reading and writing Apple Archive files, ported
from [libNeoAppleArchive](https://github.com/0xilis/libNeoAppleArchive).

> [!note]
>
> **LLM Disclosure**
>
> This library was developed with significant LLM usage (Claude models in a [pi.dev](https://pi.dev) harness). LLM output was reviewed by the human author and multiple rounds of refactoring were applied to make it more idiomatic; however the human author is not an expert in compression nor in Rust, so use at your own risk. (Contributions from any actual experts who may be reading this are welcome!)
>
> Transcripts of LLM conversations are provided in [`.dev/llm-threads`](.dev/llm-threads).

## Current Features

- Parse and build entry headers and their typed field values.
- Read and write uncompressed archives.
- Read and write compressed containers: raw, LZFSE (`pbze`), and zlib (`pbzz`).
- Build an archive from a directory tree and extract it back to disk,
  preserving mode/owner/xattrs on Unix.

LZBITMAP (`pbzb`) is recognized but not implemented. Apple Encrypted Archive
(AEA) is not yet supported.

Requires a nightly toolchain for now (uses `#![feature(read_array)]`).

## Installation

```
cargo add apple-archive
```

## Usage

Extract an archive to a directory (raw or compressed, detected automatically):

```rust
apple_archive::extract_to_path("input.aar", "output_dir")?;
```

Build a compressed archive from a directory:

```rust
use apple_archive::{PlainArchive, Compression};

let archive = PlainArchive::from_directory("my_dir")?;
archive.write_compressed_to_path("out.aar", Compression::Lzfse)?;
```

Round-trip a small archive in memory:

```rust
use apple_archive::{Header, ArchiveItem, PlainArchive, FieldKey, Uint};

let mut header = Header::new();
header.set_uint(FieldKey::TYP, Uint::Size1(b'F')).unwrap();
header.set_string(FieldKey::PAT, "hello.txt").unwrap();
header.set_blob(FieldKey::DAT, 0, 5).unwrap();
let item = ArchiveItem::with_blob(header, b"hello".to_vec());

let bytes = PlainArchive::new(vec![item]).encode().unwrap();
let parsed = PlainArchive::from_bytes(&bytes).unwrap();
```
