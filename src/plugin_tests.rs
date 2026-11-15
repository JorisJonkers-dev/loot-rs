// SPDX-License-Identifier: MIT
//
//! Unit tests of `plugin`.

use super::*;

#[test]
fn reads_masters_flags_and_text() {
    let bytes = synthetic_plugin(FLAG_ESM, &["FalloutNV.esm", "Fallout3.esm"], Some("hello"));
    let header = parse_header(&bytes).expect("parses");
    assert!(header.is_master());
    assert!(!header.is_light());
    assert_eq!(header.masters, ["FalloutNV.esm", "Fallout3.esm"]);
    assert_eq!(header.author.as_deref(), Some("tester"));
    assert_eq!(header.description.as_deref(), Some("hello"));
    assert_eq!(header.form_version, 15);
    assert!((header.version.expect("version") - 1.34).abs() < 1e-6);
    assert_eq!(header.record_count, Some(0));
}

#[test]
fn esp_without_masters() {
    let header = parse_header(&synthetic_plugin(0, &[], None)).expect("parses");
    assert!(!header.is_master());
    assert_eq!(header.masters, Vec::<String>::new());
}

#[test]
fn light_flag_is_read() {
    let header = parse_header(&synthetic_plugin(FLAG_ESM | FLAG_ESL, &[], None)).expect("ok");
    assert!(header.is_light());
}

#[test]
fn rejects_bad_input() {
    assert_eq!(parse_header(&[0; 10]), Err(PluginError::TooShort));
    let mut bytes = synthetic_plugin(0, &[], None);
    bytes[0..4].copy_from_slice(b"GRUP");
    assert_eq!(parse_header(&bytes), Err(PluginError::BadMagic(*b"GRUP")));
    let bytes = synthetic_plugin(0, &["A.esm"], None);
    assert_eq!(
        parse_header(&bytes[..bytes.len() - 3]),
        Err(PluginError::Truncated)
    );
}

#[test]
fn xxxx_extends_the_next_subrecord() {
    // A description of 70000 bytes announced through XXXX must not hide later masters.
    let long = vec![b'a'; 70_000];
    let mut data = Vec::new();
    data.extend_from_slice(b"XXXX");
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&70_001u32.to_le_bytes());
    data.extend_from_slice(b"SNAM");
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&long);
    data.push(0);
    data.extend_from_slice(b"MAST");
    data.extend_from_slice(&6u16.to_le_bytes());
    data.extend_from_slice(b"A.esm\0");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TES4");
    bytes.extend_from_slice(&u32::try_from(data.len()).expect("fits").to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&data);
    let header = parse_header(&bytes).expect("parses");
    assert_eq!(header.description.expect("description").len(), 70_000);
    assert_eq!(header.masters, ["A.esm"]);
}

fn header_bytes(size: u32) -> Vec<u8> {
    let mut bytes = Vec::from(*b"TES4");
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes
}

#[test]
fn header_data_size_reads_the_announced_size() {
    assert_eq!(header_data_size(&header_bytes(0)), Ok(0));
    assert_eq!(header_data_size(&header_bytes(1234)), Ok(1234));
    assert_eq!(header_data_size(&header_bytes(1234)[..24]), Ok(1234));
}

#[test]
fn header_data_size_refuses_short_foreign_and_huge_headers() {
    assert_eq!(header_data_size(&[]), Err(PluginError::TooShort));
    assert_eq!(
        header_data_size(&header_bytes(1)[..23]),
        Err(PluginError::TooShort)
    );
    let mut foreign = header_bytes(1);
    foreign[..4].copy_from_slice(b"TES3");
    assert_eq!(
        header_data_size(&foreign),
        Err(PluginError::BadMagic(*b"TES3"))
    );
    let limit = u32::try_from(MAX_HEADER_DATA).expect("fits");
    assert_eq!(header_data_size(&header_bytes(limit)), Ok(MAX_HEADER_DATA));
    assert_eq!(
        header_data_size(&header_bytes(limit + 1)),
        Err(PluginError::Truncated)
    );
    assert_eq!(
        header_data_size(&header_bytes(u32::MAX)),
        Err(PluginError::Truncated)
    );
}

#[test]
fn errors_read_as_sentences() {
    assert_eq!(
        PluginError::TooShort.to_string(),
        "file is shorter than a record header"
    );
    assert_eq!(
        PluginError::BadMagic(*b"GRUP").to_string(),
        "first record is \"GRUP\", expected \"TES4\""
    );
    assert!(PluginError::Truncated.to_string().contains("truncated"));
}

#[test]
fn strings_stop_at_the_first_zero_and_are_windows_1252() {
    assert_eq!(zstring(b"abc\0def"), "abc");
    assert_eq!(zstring(b"no terminator"), "no terminator");
    assert_eq!(zstring(b""), "");
    assert_eq!(zstring(&[b'c', b'a', b'f', 0xE9]), "caf\u{e9}");
    assert_eq!(
        zstring(&[0x80, 0x93, b'q', 0x94, 0x81]),
        "\u{20ac}\u{201c}q\u{201d}\u{81}"
    );
}

fn with_subrecords(subrecords: &[u8]) -> Vec<u8> {
    let mut bytes = header_bytes(u32::try_from(subrecords.len()).expect("fits"));
    bytes.extend_from_slice(subrecords);
    bytes
}

fn subrecord(kind: [u8; 4], declared: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::from(kind);
    out.extend_from_slice(&declared.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[test]
fn a_subrecord_running_past_the_data_ends_the_walk_and_keeps_what_was_read() {
    let mut data = subrecord(*b"MAST", 6, b"A.esm\0");
    data.extend(subrecord(*b"MAST", 200, b"B.esm\0"));
    let header = parse_header(&with_subrecords(&data)).expect("parses");
    assert_eq!(header.masters, ["A.esm"]);
}

#[test]
fn trailing_bytes_that_are_not_a_subrecord_are_ignored() {
    let mut data = subrecord(*b"MAST", 6, b"A.esm\0");
    data.extend_from_slice(&[1, 2, 3]);
    let header = parse_header(&with_subrecords(&data)).expect("parses");
    assert_eq!(header.masters, ["A.esm"]);
}

#[test]
fn an_xxxx_that_announces_more_than_remains_stops_the_walk() {
    let mut data = subrecord(*b"XXXX", 4, &u32::MAX.to_le_bytes());
    data.extend(subrecord(*b"MAST", 0, b""));
    let header = parse_header(&with_subrecords(&data)).expect("parses");
    assert_eq!(header.masters, Vec::<String>::new());
}

#[test]
fn hedr_and_unknown_subrecords() {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.5f32.to_le_bytes());
    hedr.extend_from_slice(&7u32.to_le_bytes());
    let mut data = subrecord(*b"HEDR", 8, &hedr);
    data.extend(subrecord(*b"ZZZZ", 3, b"abc"));
    data.extend(subrecord(*b"HEDR", 2, &[1, 2]));
    let header = parse_header(&with_subrecords(&data)).expect("parses");
    assert_eq!(header.version, None, "a later short HEDR replaces it");
    assert_eq!(header.record_count, None);
    let header = parse_header(&with_subrecords(&subrecord(*b"HEDR", 8, &hedr))).expect("parses");
    assert_eq!(header.version, Some(1.5));
    assert_eq!(header.record_count, Some(7));
}

#[test]
fn flags_and_form_version_come_from_the_record_header() {
    let mut bytes = with_subrecords(&[]);
    bytes[8..12].copy_from_slice(&(FLAG_ESM | FLAG_LOCALIZED).to_le_bytes());
    bytes[20..22].copy_from_slice(&15u16.to_le_bytes());
    let header = parse_header(&bytes).expect("parses");
    assert_eq!(header.flags, FLAG_ESM | FLAG_LOCALIZED);
    assert_eq!(header.form_version, 15);
    assert!(header.is_master());
    assert!(!header.is_light());
}

#[test]
fn read_header_reads_a_file() {
    let dir = crate::support_tests::TempDir::new();
    let path = dir.plugin("Mod.esm", &["FalloutNV.esm"]);
    let header = read_header(&path).expect("reads");
    assert!(header.is_master());
    assert_eq!(header.masters, ["FalloutNV.esm"]);
}

fn kind(path: &Path) -> io::ErrorKind {
    read_header(path).expect_err("fails").kind()
}

#[test]
fn read_header_reports_missing_short_foreign_and_truncated_files() {
    let dir = crate::support_tests::TempDir::new();
    assert_eq!(
        kind(&dir.path().join("absent.esp")),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        kind(&dir.file("empty.esp", b"")),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        kind(&dir.file("text.esp", b"this is not a plugin at all, just text")),
        io::ErrorKind::InvalidData
    );
    let good = synthetic_plugin(0, &["A.esm"], None);
    let cut = dir.file("cut.esp", &good[..good.len() - 4]);
    let error = read_header(&cut).expect_err("truncated");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("truncated"), "{error}");
}

#[test]
fn read_header_on_a_directory_is_an_error() {
    let dir = crate::support_tests::TempDir::new();
    assert!(read_header(dir.path()).is_err());
}

#[test]
fn synthetic_plugins_with_a_description_round_trip() {
    let bytes = synthetic_plugin(0, &["A.esm", "B.esm"], Some("Version: 1.2"));
    let header = parse_header(&bytes).expect("parses");
    assert_eq!(header.description.as_deref(), Some("Version: 1.2"));
    assert_eq!(header.masters, ["A.esm", "B.esm"]);
}

#[test]
fn a_size_field_larger_than_the_file_is_refused_before_allocating_by_it() {
    let dir = crate::support_tests::TempDir::new();
    let mut claim = header_bytes(u32::try_from(MAX_HEADER_DATA).expect("fits"));
    claim.extend_from_slice(b"MAST");
    let path = dir.file("claims.esp", &claim);
    let error = read_header(&path).expect_err("truncated");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
