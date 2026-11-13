//! Property-based tests of the plugin header reader, `plugins.txt` and the CRC.
#![allow(clippy::format_collect, clippy::format_push_string)]

mod common;

use common::Fixture;
use loot_rs::env::{crc32, parse_plugins_txt};
use loot_rs::plugin::{
    FLAG_ESL, FLAG_ESM, MAX_HEADER_DATA, PluginError, RECORD_HEADER_SIZE, header_data_size,
    parse_header, read_header, synthetic_plugin,
};
use proptest::prelude::*;

fn master_names() -> impl Strategy<Value = Vec<String>> {
    proptest::collection::vec("[A-Za-z0-9 _.()-]{1,40}\\.es[mp]", 0..8)
}

fn plugin_bytes() -> impl Strategy<Value = (u32, Vec<String>, Option<String>, Vec<u8>)> {
    (
        any::<u32>(),
        master_names(),
        proptest::option::of("[ -~]{0,200}"),
    )
        .prop_map(|(flags, masters, description)| {
            let refs: Vec<&str> = masters.iter().map(String::as_str).collect();
            let bytes = synthetic_plugin(flags, &refs, description.as_deref());
            (flags, masters, description, bytes)
        })
}

proptest! {
    #![proptest_config(common::proptest_config(256))]

    // ---- arbitrary bytes never panic or read out of bounds -----------------------------------

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = header_data_size(&bytes);
        let _ = parse_header(&bytes);
    }

    #[test]
    fn bytes_that_start_like_a_plugin_never_panic(
        size in any::<u32>(), tail in proptest::collection::vec(any::<u8>(), 0..400),
    ) {
        let mut bytes = b"TES4".to_vec();
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&tail);
        let _ = parse_header(&bytes);
    }

    #[test]
    fn subrecord_soup_inside_a_valid_header_never_panics(
        subrecords in proptest::collection::vec(
            (proptest::sample::select(vec![*b"XXXX", *b"MAST", *b"SNAM", *b"CNAM", *b"HEDR", *b"DATA", *b"ZZZZ"]),
             any::<u16>(), proptest::collection::vec(any::<u8>(), 0..40)),
            0..12,
        ),
    ) {
        let mut data = Vec::new();
        for (kind, declared, payload) in &subrecords {
            data.extend_from_slice(kind);
            data.extend_from_slice(&declared.to_le_bytes());
            data.extend_from_slice(payload);
        }
        let mut bytes = b"TES4".to_vec();
        bytes.extend_from_slice(&u32::try_from(data.len()).expect("small").to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&data);
        let header = parse_header(&bytes).expect("a complete header record parses");
        // Everything read lies inside the input: no master can be longer than the data.
        prop_assert!(header.masters.iter().all(|m| m.len() <= data.len()));
    }

    #[test]
    fn the_announced_size_is_what_header_data_size_returns(size in any::<u32>()) {
        let mut bytes = b"TES4".to_vec();
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        let expected = usize::try_from(size).expect("32-bit sizes fit");
        match header_data_size(&bytes) {
            Ok(read) => { prop_assert_eq!(read, expected); prop_assert!(read <= MAX_HEADER_DATA); }
            Err(error) => { prop_assert_eq!(error, PluginError::Truncated); prop_assert!(expected > MAX_HEADER_DATA); }
        }
    }

    // ---- round trip --------------------------------------------------------------------------

    #[test]
    fn a_synthetic_plugin_reads_back_what_was_written((flags, masters, description, bytes) in plugin_bytes()) {
        let header = parse_header(&bytes).expect("parses");
        prop_assert_eq!(header.flags, flags);
        prop_assert_eq!(header.is_master(), flags & FLAG_ESM != 0);
        prop_assert_eq!(header.is_light(), flags & FLAG_ESL != 0);
        prop_assert_eq!(header.masters, masters);
        prop_assert_eq!(header.description, description);
        prop_assert_eq!(header.author.as_deref(), Some("tester"));
    }

    #[test]
    fn extra_bytes_after_the_header_record_are_ignored((_, masters, _, mut bytes) in plugin_bytes(), tail in proptest::collection::vec(any::<u8>(), 0..200)) {
        bytes.extend_from_slice(&tail);
        prop_assert_eq!(parse_header(&bytes).expect("parses").masters, masters);
    }

    // ---- truncation and corruption -----------------------------------------------------------

    #[test]
    fn every_truncation_is_an_error_never_a_partial_header((_, _, _, bytes) in plugin_bytes(), cut in any::<proptest::sample::Index>()) {
        let at = cut.index(bytes.len());
        let result = parse_header(&bytes[..at]);
        if at < RECORD_HEADER_SIZE {
            prop_assert_eq!(result, Err(PluginError::TooShort));
        } else {
            prop_assert_eq!(result, Err(PluginError::Truncated));
        }
    }

    #[test]
    fn flipping_any_byte_never_panics((_, _, _, mut bytes) in plugin_bytes(), at in any::<proptest::sample::Index>(), flip in 1u8..=255) {
        let index = at.index(bytes.len());
        bytes[index] ^= flip;
        let _ = parse_header(&bytes);
    }

    #[test]
    fn a_master_list_cut_inside_the_data_keeps_only_whole_masters(
        (_, masters, _, bytes) in plugin_bytes(),
    ) {
        // Shrink the announced size so the data ends inside the master list: the walk stops there.
        let full = bytes.len() - RECORD_HEADER_SIZE;
        for shrink in [1usize, 3, 7] {
            if full < shrink { continue; }
            let mut cut = bytes[..bytes.len() - shrink].to_vec();
            cut[4..8].copy_from_slice(&u32::try_from(full - shrink).expect("small").to_le_bytes());
            let header = parse_header(&cut).expect("a header with a consistent size parses");
            prop_assert!(header.masters.len() <= masters.len());
            prop_assert_eq!(&header.masters[..], &masters[..header.masters.len()]);
        }
    }

    // ---- files -------------------------------------------------------------------------------

    #[test]
    fn reading_a_file_agrees_with_parsing_its_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let fixture = Fixture::new();
        fixture.file("probe.esp", &bytes);
        let from_file = read_header(&fixture.data().join("probe.esp"));
        prop_assert_eq!(from_file.ok(), parse_header(&bytes).ok());
    }

    #[test]
    fn reading_a_valid_plugin_file_agrees_with_parsing_its_bytes((_, _, _, bytes) in plugin_bytes()) {
        let fixture = Fixture::new();
        fixture.file("probe.esp", &bytes);
        let from_file = read_header(&fixture.data().join("probe.esp")).expect("reads");
        prop_assert_eq!(from_file, parse_header(&bytes).expect("parses"));
    }

    // ---- plugins.txt -------------------------------------------------------------------------

    #[test]
    fn arbitrary_text_in_plugins_txt_never_panics(text in ".{0,300}") {
        let _ = parse_plugins_txt(&text);
    }

    #[test]
    fn plugins_txt_skips_comment_and_blank_lines_and_trims_names(
        lines in proptest::collection::vec(
            prop_oneof![
                "[ \t]*#[ -~]{0,12}".prop_map(|c| (c, None)),
                "[ \t]*".prop_map(|b| (b, None)),
                "[A-Za-z0-9][A-Za-z0-9 ._-]{0,10}[A-Za-z0-9]".prop_map(|n| (format!("  {n}\t"), Some(n))),
            ],
            0..12,
        ),
    ) {
        let text: String = lines.iter().map(|(line, _)| format!("{line}\r\n")).collect();
        let expected: Vec<&str> = lines.iter().filter_map(|(_, name)| name.as_deref()).collect();
        let names: Vec<String> = parse_plugins_txt(&text).into_iter().map(|slot| slot.name).collect();
        prop_assert_eq!(names, expected);
    }

    #[test]
    fn plugins_txt_keeps_the_order_and_has_one_slot_per_line(names in proptest::collection::vec("[A-Za-z0-9._-]{1,12}", 0..10), starred in any::<bool>()) {
        let text: String = names.iter().map(|n| format!("{}{n}\r\n", if starred { "*" } else { "" })).collect();
        let slots = parse_plugins_txt(&text);
        prop_assert_eq!(slots.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), names);
        prop_assert!(slots.iter().all(|s| s.active));
    }

    #[test]
    fn in_the_star_style_exactly_the_starred_lines_are_active(flags in proptest::collection::vec(any::<bool>(), 1..10)) {
        let mut text = String::from("*Always.esm\n");
        for (index, starred) in flags.iter().enumerate() {
            text.push_str(&format!("{}P{index}.esp\n", if *starred { "*" } else { "" }));
        }
        let slots = parse_plugins_txt(&text);
        prop_assert!(slots[0].active);
        for (slot, starred) in slots[1..].iter().zip(&flags) {
            prop_assert_eq!(slot.active, *starred);
        }
    }

    // ---- CRC-32 ------------------------------------------------------------------------------

    #[test]
    fn crc32_matches_a_bitwise_reference(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in &bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        prop_assert_eq!(crc32(&bytes), !crc);
    }

    #[test]
    fn crc32_detects_any_single_bit_flip(bytes in proptest::collection::vec(any::<u8>(), 1..200), at in any::<proptest::sample::Index>(), bit in 0u8..8) {
        let mut changed = bytes.clone();
        let index = at.index(changed.len());
        changed[index] ^= 1 << bit;
        prop_assert_ne!(crc32(&bytes), crc32(&changed));
    }
}
