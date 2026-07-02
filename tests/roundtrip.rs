//! In-memory and on-disk round-trip tests for the Apple Archive port.

use apple_archive::{
    Archive, ArchiveItem, Compression, Field, FieldKey, Hash, Header, PlainArchive, Timespec, Uint,
};

/// A header should serialize to the exact byte layout the format specifies:
/// `AA01`, a u16 total size, then `key(3) + subtype(1) + value` per field.
#[test]
fn header_byte_layout() {
    let mut header = Header::new();
    header.set_uint(FieldKey::TYP, Uint::U8(b'F')).unwrap();

    let bytes = header.encode().unwrap();
    // 6-byte prefix + 4 (key+subtype) + 1 (value) = 11 bytes.
    assert_eq!(bytes.len(), 11);
    assert_eq!(&bytes[0..4], b"AA01");
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]) as usize, bytes.len());
    // Field: 'T' 'Y' 'P' '1' 'F'
    assert_eq!(&bytes[6..9], b"TYP");
    assert_eq!(bytes[9], b'1'); // uint, size 1
    assert_eq!(bytes[10], b'F');
}

#[test]
fn field_key_u32_representation() {
    // Top three bytes are ASCII, low byte zero.
    assert_eq!(
        FieldKey::TYP.as_u32(),
        (b'T' as u32) << 24 | (b'Y' as u32) << 16 | (b'P' as u32) << 8
    );
    assert_eq!(FieldKey::TYP.as_bytes(), *b"TYP");
    assert_eq!(FieldKey::from_ascii(b"TYP"), FieldKey::TYP);
    assert_eq!(FieldKey::TYP.to_string(), "TYP");
}

#[test]
fn unknown_key_roundtrips() {
    // An arbitrary key with no named constant must still round-trip — this is
    // why keys are a u32 newtype rather than a closed enum.
    let custom = FieldKey::from_ascii(b"ZZZ");
    let mut header = Header::new();
    header.set_uint(custom, Uint::U32(0xABCD)).unwrap();

    let parsed = Header::from_bytes(&header.encode().unwrap()).unwrap();
    assert_eq!(parsed, header);
    assert_eq!(parsed.get_uint(custom), Some(0xABCD));
    assert_eq!(parsed.entries()[0].key, custom);
}

#[test]
fn header_string_field_layout() {
    let mut header = Header::new();
    header.set_string(FieldKey::PAT, "hi").unwrap();
    let bytes = header.encode().unwrap();
    // prefix(6) + key(3) + subtype(1) + len(2) + "hi"(2) = 14
    assert_eq!(bytes.len(), 14);
    assert_eq!(&bytes[6..9], b"PAT");
    assert_eq!(bytes[9], b'P');
    assert_eq!(u16::from_le_bytes([bytes[10], bytes[11]]), 2);
    assert_eq!(&bytes[12..14], b"hi");
}

#[test]
fn header_roundtrip_all_types() {
    let mut header = Header::new();
    header.set_flag(FieldKey::TYP).unwrap();
    header.set_uint(FieldKey::UID, Uint::U32(12345)).unwrap();
    header.set_string(FieldKey::PAT, "some/path.txt").unwrap();
    header.set_blob(FieldKey::DAT, 0, 4096).unwrap();
    header
        .set_timespec(FieldKey::MOD, Timespec::B8([1, 2, 3, 4, 5, 6, 7, 8]))
        .unwrap();
    header.set_hash(FieldKey::LNK, Hash::B32([0xAB; 32])).unwrap();

    let bytes = header.encode().unwrap();
    let parsed = Header::from_bytes(&bytes).unwrap();
    assert_eq!(parsed, header);
    assert_eq!(parsed.encoded_len(), bytes.len());
}

#[test]
fn blob_auto_size() {
    let mut header = Header::new();
    header.set_blob(FieldKey::DAT, 0, 100).unwrap();
    assert!(matches!(
        header.get(FieldKey::DAT),
        Some(Field::Blob { size: 2, blob_size: 100 })
    ));

    header.set_blob(FieldKey::XAT, 0, 70000).unwrap();
    assert!(matches!(
        header.get(FieldKey::XAT),
        Some(Field::Blob { size: 4, .. })
    ));
}

#[test]
fn plain_archive_roundtrip() {
    let mut h1 = Header::new();
    h1.set_uint(FieldKey::TYP, Uint::U8(b'F')).unwrap();
    h1.set_string(FieldKey::PAT, "a.txt").unwrap();
    h1.set_blob(FieldKey::DAT, 0, 5).unwrap();
    let i1 = ArchiveItem::with_blob(h1, b"hello".to_vec());

    let mut h2 = Header::new();
    h2.set_uint(FieldKey::TYP, Uint::U8(b'D')).unwrap();
    h2.set_string(FieldKey::PAT, "dir").unwrap();
    let i2 = ArchiveItem::new(h2);

    let archive = PlainArchive::new(vec![i1, i2]);
    let bytes = archive.encode().unwrap();
    let parsed = PlainArchive::from_bytes(&bytes).unwrap();
    assert_eq!(parsed, archive);
}

#[test]
fn blob_slice_extraction() {
    // Item with two blob fields (DAT then XAT) to verify offset walking.
    let mut header = Header::new();
    header.set_string(FieldKey::PAT, "f").unwrap();
    header.set_blob(FieldKey::DAT, 0, 3).unwrap();
    header.set_blob(FieldKey::XAT, 0, 2).unwrap();
    let mut blob = Vec::new();
    blob.extend_from_slice(b"abc"); // DAT
    blob.extend_from_slice(b"XY"); // XAT
    let item = ArchiveItem::with_blob(header, blob);

    assert_eq!(item.blob_slice(FieldKey::DAT), Some(&b"abc"[..]));
    assert_eq!(item.blob_slice(FieldKey::XAT), Some(&b"XY"[..]));
}

#[test]
fn lzfse_container_roundtrip() {
    let archive = sample_archive();
    let container = archive.encode_compressed(Compression::Lzfse).unwrap();
    assert_eq!(&container[0..4], b"pbze");

    let decoded = Archive::from_bytes(&container).unwrap();
    assert_eq!(decoded.compression, Compression::Lzfse);
    assert_eq!(decoded.plain, archive);
}

#[test]
fn zlib_container_roundtrip() {
    let archive = sample_archive();
    let container = archive.encode_compressed(Compression::Zlib).unwrap();
    assert_eq!(&container[0..4], b"pbzz");

    let decoded = Archive::from_bytes(&container).unwrap();
    assert_eq!(decoded.compression, Compression::Zlib);
    assert_eq!(decoded.plain, archive);
}

#[test]
fn raw_container_detection() {
    let archive = sample_archive();
    let raw = archive.encode().unwrap();
    let decoded = Archive::from_bytes(&raw).unwrap();
    assert_eq!(decoded.compression, Compression::None);
    assert_eq!(decoded.plain, archive);
}

#[test]
fn lzbitmap_unsupported() {
    let archive = sample_archive();
    assert!(matches!(
        archive.encode_compressed(Compression::Lzbitmap),
        Err(apple_archive::Error::UnsupportedCompression(Compression::Lzbitmap))
    ));
}

fn sample_archive() -> PlainArchive {
    let mut items = Vec::new();
    for i in 0..8 {
        let mut header = Header::new();
        header.set_uint(FieldKey::TYP, Uint::U8(b'F')).unwrap();
        header.set_string(FieldKey::PAT, format!("file{i}.txt")).unwrap();
        let data = format!("contents of file number {i}, repeated. ").repeat(20);
        header.set_blob(FieldKey::DAT, 0, data.len() as u64).unwrap();
        items.push(ArchiveItem::with_blob(header, data.into_bytes()));
    }
    PlainArchive::new(items)
}
