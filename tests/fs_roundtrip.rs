//! Filesystem round-trip tests, including cross-validation against Apple's
//! native `aa` tool when it is available (macOS).

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use apple_archive::{Archive, Compression, PlainArchive};

/// A unique scratch directory under the system temp dir.
fn scratch(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("apple_archive_test_{tag}_{nanos}_{}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}

fn make_tree(root: &Path) {
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("top.txt"), b"top level file\n").unwrap();
    fs::write(root.join("sub/nested.txt"), b"nested content 12345\n").unwrap();
    fs::write(root.join("sub/empty.txt"), b"").unwrap();
}

/// Recursively collect (relative-path, contents) for every regular file.
fn collect_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let ft = entry.file_type().unwrap();
            if ft.is_dir() {
                walk(base, &path, out);
            } else if ft.is_file() {
                let rel = path.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Build an archive from a directory with this crate, then extract it with this
/// crate, and confirm the file contents survive.
#[test]
fn archive_then_extract_self() {
    let base = scratch("self");
    let src = base.join("src");
    make_tree(&src);

    let archive = PlainArchive::from_directory(&src).unwrap();
    let out = base.join("out");
    archive.extract_to_dir(&out).unwrap();

    assert_eq!(collect_files(&src), collect_files(&out));
    fs::remove_dir_all(&base).ok();
}

fn aa_available() -> bool {
    Path::new("/usr/bin/aa").exists()
}

/// Archive a directory with this crate (raw), then extract it with Apple's `aa`.
#[test]
fn extract_with_apple_aa() {
    if !aa_available() {
        eprintln!("skipping: /usr/bin/aa not available");
        return;
    }
    let base = scratch("apple_extract");
    let src = base.join("src");
    make_tree(&src);

    let archive_path = base.join("archive.aar");
    PlainArchive::from_directory(&src)
        .unwrap()
        .write_to_path(&archive_path)
        .unwrap();

    let out = base.join("aa_out");
    fs::create_dir_all(&out).unwrap();
    let status = Command::new("/usr/bin/aa")
        .args(["extract", "-i"])
        .arg(&archive_path)
        .arg("-d")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "aa extract failed");

    assert_eq!(collect_files(&src), collect_files(&out));
    fs::remove_dir_all(&base).ok();
}

/// Archive a directory with Apple's `aa` (raw), then read + extract it with this
/// crate.
#[test]
fn read_apple_aa_archive() {
    if !aa_available() {
        eprintln!("skipping: /usr/bin/aa not available");
        return;
    }
    let base = scratch("apple_read");
    let src = base.join("src");
    make_tree(&src);

    let archive_path = base.join("archive.aar");
    let status = Command::new("/usr/bin/aa")
        .args(["archive", "-d"])
        .arg(&src)
        .arg("-o")
        .arg(&archive_path)
        .status()
        .unwrap();
    assert!(status.success(), "aa archive failed");

    let archive = Archive::from_path(&archive_path).unwrap();
    let out = base.join("out");
    archive.extract_to_dir(&out).unwrap();

    assert_eq!(collect_files(&src), collect_files(&out));
    fs::remove_dir_all(&base).ok();
}

/// Archive with Apple's `aa` using LZFSE compression, then decode with this
/// crate — validates the `pbze` block reader against real output.
#[test]
fn read_apple_aa_lzfse() {
    if !aa_available() {
        eprintln!("skipping: /usr/bin/aa not available");
        return;
    }
    let base = scratch("apple_lzfse");
    let src = base.join("src");
    make_tree(&src);
    // Add a larger file so LZFSE actually engages.
    fs::write(src.join("big.txt"), "the quick brown fox. ".repeat(4096)).unwrap();

    let archive_path = base.join("archive.aar");
    let status = Command::new("/usr/bin/aa")
        .args(["archive", "-a", "lzfse", "-d"])
        .arg(&src)
        .arg("-o")
        .arg(&archive_path)
        .status()
        .unwrap();
    assert!(status.success(), "aa archive (lzfse) failed");

    let archive = Archive::from_path(&archive_path).unwrap();
    assert_eq!(archive.compression, Compression::Lzfse);
    let out = base.join("out");
    archive.extract_to_dir(&out).unwrap();

    assert_eq!(collect_files(&src), collect_files(&out));
    fs::remove_dir_all(&base).ok();
}

/// Compress with this crate (LZFSE), then extract with Apple's `aa`.
#[test]
fn lzfse_extract_with_apple_aa() {
    if !aa_available() {
        eprintln!("skipping: /usr/bin/aa not available");
        return;
    }
    let base = scratch("lzfse_apple_extract");
    let src = base.join("src");
    make_tree(&src);

    let archive_path = base.join("archive.aar");
    PlainArchive::from_directory(&src)
        .unwrap()
        .write_compressed_to_path(Compression::Lzfse, &archive_path)
        .unwrap();

    let out = base.join("aa_out");
    fs::create_dir_all(&out).unwrap();
    let status = Command::new("/usr/bin/aa")
        .args(["extract", "-i"])
        .arg(&archive_path)
        .arg("-d")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "aa extract (lzfse) failed");

    assert_eq!(collect_files(&src), collect_files(&out));
    fs::remove_dir_all(&base).ok();
}
