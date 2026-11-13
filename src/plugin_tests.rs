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
