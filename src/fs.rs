//! Filesystem convenience: building an archive from a directory tree and
//! extracting one back to disk.
//!
//! Ownership/mode/xattr preservation is Unix-only; on other platforms those
//! attributes are silently skipped and symlink extraction is unsupported.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::archive::{ArchiveItem, PlainArchive};
use crate::compression::Archive;
use crate::error::{Error, Result};
use crate::field::{FieldKey, Uint};
use crate::header::Header;

impl PlainArchive {
    /// Build a plain archive from a filesystem path.
    ///
    /// If `path` is a regular file, the archive contains a single entry named
    /// after the file. If it is a directory, its contents are archived
    /// recursively with paths relative to the directory.
    pub fn from_directory(path: impl AsRef<Path>) -> Result<PlainArchive> {
        let path = path.as_ref();
        let meta = fs::symlink_metadata(path)?;
        if meta.is_file() {
            return Ok(PlainArchive::new(vec![wrap_file(path)?]));
        }
        let mut items = Vec::new();
        add_directory_contents(path, "", &mut items)?;
        Ok(PlainArchive::new(items))
    }

    /// Extract every entry into the directory `root`, creating it if needed.
    pub fn extract_to_dir(&self, root: impl AsRef<Path>) -> Result<()> {
        let root = root.as_ref();
        fs::create_dir_all(root)?;
        for item in &self.items {
            extract_item(item, root)?;
        }
        Ok(())
    }
}

impl Archive {
    /// Extract the decoded archive into the directory `root`.
    pub fn extract_to_dir(&self, root: impl AsRef<Path>) -> Result<()> {
        self.plain.extract_to_dir(root)
    }
}

/// Wrap a single regular file as a one-item archive (path = file basename).
fn wrap_file(path: &Path) -> Result<ArchiveItem> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let data = fs::read(path)?;
    let mut header = Header::new();
    header.set_uint(FieldKey::TYP, Uint::Size1(b'F'))?;
    header.set_string(FieldKey::PAT, name.into_bytes())?;
    header.set_blob(FieldKey::DAT, 0, data.len() as u64)?;
    Ok(ArchiveItem::with_blob(header, data))
}

/// Recursively append entries under `dir` to `items`. `base_rel` is the archive
/// path prefix for entries in this directory.
fn add_directory_contents(dir: &Path, base_rel: &str, items: &mut Vec<ArchiveItem>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let full = entry.path();
        let rel = if base_rel.is_empty() {
            name_str.into_owned()
        } else {
            format!("{base_rel}/{name_str}")
        };

        let meta = fs::symlink_metadata(&full)?;
        let mut header = Header::new();
        set_owner_fields(&mut header, &meta)?;

        let file_type = meta.file_type();
        if file_type.is_dir() {
            header.set_string(FieldKey::PAT, rel.clone().into_bytes())?;
            header.set_uint(FieldKey::TYP, Uint::Size1(b'D'))?;
            items.push(ArchiveItem::new(header));
            add_directory_contents(&full, &rel, items)?;
        } else if file_type.is_symlink() {
            let target = fs::read_link(&full)?;
            let target_bytes = path_to_bytes(&target);
            header.set_string(FieldKey::PAT, rel.into_bytes())?;
            header.set_string(FieldKey::LNK, target_bytes)?;
            header.set_uint(FieldKey::TYP, Uint::Size1(b'L'))?;
            items.push(ArchiveItem::new(header));
        } else if file_type.is_file() {
            let data = fs::read(&full)?;
            header.set_string(FieldKey::PAT, rel.into_bytes())?;
            header.set_uint(FieldKey::TYP, Uint::Size1(b'F'))?;
            header.set_blob(FieldKey::DAT, 0, data.len() as u64)?;
            items.push(ArchiveItem::with_blob(header, data));
        }
        // Other node types (sockets, fifos, devices) are skipped.
    }
    Ok(())
}

/// Extract one item beneath `root`.
fn extract_item(item: &ArchiveItem, root: &Path) -> Result<()> {
    let header = &item.header;
    let typ = header
        .get_uint(FieldKey::TYP)
        .ok_or(Error::MissingField(FieldKey::TYP))? as u8;

    let pat = header
        .get_string(FieldKey::PAT)
        .ok_or(Error::MissingField(FieldKey::PAT))?;
    if pat.is_empty() {
        // Empty-named entry (used only to describe the output root); skip.
        return Ok(());
    }
    let target = safe_join(root, pat)?;

    match typ {
        b'D' => {
            fs::create_dir_all(&target)?;
            apply_metadata(&target, header);
            if let Some(blob) = item.blob_slice(FieldKey::XAT) {
                apply_xattr_blob(&target, blob);
            }
        }
        b'F' => {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let data = item.blob_slice(FieldKey::DAT).unwrap_or(&[]);
            fs::write(&target, data)?;
            apply_metadata(&target, header);
            if let Some(blob) = item.blob_slice(FieldKey::XAT) {
                apply_xattr_blob(&target, blob);
            }
        }
        b'L' => {
            let link = header
                .get_string(FieldKey::LNK)
                .ok_or(Error::MissingField(FieldKey::LNK))?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            create_symlink(link, &target)?;
        }
        other => return Err(Error::UnsupportedEntryType(other)),
    }
    Ok(())
}

/// Join `rel` (raw path bytes) onto `root`, rejecting absolute paths and any
/// `..` component so an entry cannot escape the extraction root.
fn safe_join(root: &Path, rel: &[u8]) -> Result<PathBuf> {
    let rel_path = bytes_to_path(rel);
    let mut out = root.to_path_buf();
    for component in rel_path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err(Error::PathTraversal(rel_path.to_path_buf()));
            }
        }
    }
    Ok(out)
}

// --- Platform-specific helpers -------------------------------------------------

#[cfg(unix)]
fn set_owner_fields(header: &mut Header, meta: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    // UID is stored in 2 bytes, GID in 1 byte.
    header.set_uint(FieldKey::UID, Uint::Size2(meta.uid() as u16))?;
    header.set_uint(FieldKey::GID, Uint::Size1(meta.gid() as u8))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_fields(_header: &mut Header, _meta: &fs::Metadata) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn apply_metadata(path: &Path, header: &Header) {
    use std::os::unix::fs::{PermissionsExt, chown};

    if let Some(mode) = header.get_uint(FieldKey::MOD) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode as u32));
    }
    let uid = header.get_uint(FieldKey::UID).map(|u| u as u32);
    let gid = header.get_uint(FieldKey::GID).map(|g| g as u32);
    if uid.is_some() || gid.is_some() {
        let _ = chown(path, uid, gid);
    }
}

#[cfg(not(unix))]
fn apply_metadata(_path: &Path, _header: &Header) {}

#[cfg(unix)]
fn create_symlink(target: &[u8], link: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    symlink(bytes_to_path(target), link)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_symlink(_target: &[u8], _link: &Path) -> Result<()> {
    Err(Error::Format(
        "symlink extraction is only supported on Unix".into(),
    ))
}

/// Parse and apply an `XAT` extended-attribute blob to `path`.
///
/// Each record is a `u32` little-endian size (including the 4-byte size field),
/// a NUL-terminated name, and then the attribute value.
#[cfg(unix)]
fn apply_xattr_blob(path: &Path, blob: &[u8]) {
    let mut pos = 0usize;
    while pos + 4 <= blob.len() {
        let mut item_size =
            u32::from_le_bytes([blob[pos], blob[pos + 1], blob[pos + 2], blob[pos + 3]]) as usize;
        if item_size < 4 || pos + item_size > blob.len() {
            item_size = blob.len() - pos;
        }
        let record = &blob[pos..pos + item_size];
        let name_region = &record[4..];
        let name_len = name_region
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_region.len());
        if 4 + name_len + 1 > item_size {
            break;
        }
        let name = &name_region[..name_len];
        let value = &record[4 + name_len + 1..];
        let name_os = <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(name);
        let _ = xattr::set(path, name_os, value);
        pos += item_size;
    }
}

#[cfg(not(unix))]
fn apply_xattr_blob(_path: &Path, _blob: &[u8]) {}

#[cfg(unix)]
fn path_to_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_to_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}
