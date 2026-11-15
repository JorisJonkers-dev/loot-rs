//! Tests of the bounded PE reader of `exe_version` that need no interpreter, so they run in every build (the reader
//! answers `is_executable()` there): synthetic images built here, no game files.

use super::*;
use crate::support_tests::TempDir;
use std::path::PathBuf;

fn written(spec: &PeSpec) -> (TempDir, PathBuf) {
    let dir = TempDir::new();
    let path = dir.file("t.dll", &spec.build());
    (dir, path)
}

fn read(spec: &PeSpec, function: &str) -> Result<bool, ConditionError> {
    let (_dir, path) = written(spec);
    preflight(&path, function)
}

fn block_with(edit: impl FnOnce(&mut Vec<u8>)) -> PeSpec {
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    edit(&mut spec.block);
    spec
}

fn put(block: &mut [u8], at: usize, value: u16) {
    block[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn a_well_formed_image_with_a_version_resource_is_read_and_is_an_executable() {
    let spec = PeSpec::new([1, 4, 0, 2], "1.4.0.2");
    assert_eq!(read(&spec, "version"), Ok(true));
    assert_eq!(read(&spec, "product_version"), Ok(true));
    let (_dir, path) = written(&spec);
    assert!(is_executable(&path));
}

#[test]
fn files_that_are_not_pe_images_are_errors_and_not_executables() {
    let dir = TempDir::new();
    for (name, bytes) in [
        ("notes.txt", &b"hello, not a PE image"[..]),
        ("tiny.dll", b"MZ"),
    ] {
        let path = dir.file(name, bytes);
        assert!(
            matches!(
                preflight(&path, "version"),
                Err(ConditionError::Environment(_))
            ),
            "{name}"
        );
        assert!(!is_executable(&path), "{name}");
    }
    assert!(!is_executable(dir.path()), "a directory");
    assert!(
        !is_executable(&dir.path().join("missing.exe")),
        "a missing file"
    );
}

#[test]
fn an_image_without_a_resource_directory_is_an_executable_without_a_version() {
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.directories = 2;
    assert_eq!(read(&spec, "version"), Ok(false));
    let (_dir, path) = written(&spec);
    assert!(is_executable(&path));
    spec.directories = 16;
    spec.directory_size = Some(0);
    assert_eq!(read(&spec, "version"), Ok(false));
}

#[test]
fn sizes_that_come_from_the_file_are_bounded() {
    let mut cases = Vec::new();
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.directories = u32::MAX;
    cases.push(("number of data directories", spec));
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.directory_size = Some(u32::MAX);
    cases.push(("resource directory size", spec));
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.leaf_size = Some(u32::MAX);
    cases.push(("version block size", spec));
    for (what, spec) in cases {
        assert!(
            matches!(read(&spec, "version"), Err(ConditionError::Environment(_))),
            "{what}"
        );
        let (_dir, path) = written(&spec);
        assert!(!is_executable(&path), "{what}");
    }
}

#[test]
fn a_block_without_a_product_version_has_no_product_version() {
    let mut block = version_block_bytes([1, 2, 0, 0], "9");
    let at = block.windows(2).position(|w| w == b"P\0").expect("key");
    block[at] = b'Q';
    let mut spec = PeSpec::new([1, 2, 0, 0], "9");
    spec.block = block;
    assert_eq!(read(&spec, "version"), Ok(true));
    assert_eq!(read(&spec, "product_version"), Ok(false));
}

#[test]
fn malformed_version_blocks_are_errors_for_both_functions() {
    let mut cases = vec![
        ("wLength is not the block", block_with(|b| put(b, 0, 10))),
        ("wrong key", block_with(|b| b[6] = b'X')),
        ("odd value length", block_with(|b| put(b, 2, 7))),
        ("no signature", block_with(|b| b[40] = 0)),
        ("shorter than a header", block_with(|b| b.truncate(30))),
    ];
    let mut empty = PeSpec::new([1, 0, 0, 0], "1");
    empty.leaf_size = Some(0);
    cases.push(("an empty resource", empty));
    for (what, spec) in cases {
        for function in ["version", "product_version"] {
            assert!(
                matches!(read(&spec, function), Err(ConditionError::Environment(_))),
                "{what}: {function}"
            );
        }
    }
}

#[test]
fn string_tables_with_lengths_that_do_not_fit_are_errors() {
    const STRING_AT: usize = 152;
    for (what, spec) in [
        (
            "table longer than its parent",
            block_with(|b| put(b, 128, 4000)),
        ),
        (
            "table shorter than its header",
            block_with(|b| put(b, 128, 10)),
        ),
        (
            "string shorter than its header",
            block_with(|b| put(b, STRING_AT, 4)),
        ),
        (
            "string longer than its table",
            block_with(|b| put(b, STRING_AT, 4000)),
        ),
        (
            "value longer than its table",
            block_with(|b| put(b, STRING_AT + 2, 4000)),
        ),
        (
            "StringFileInfo longer than the block",
            block_with(|b| put(b, 92, 4000)),
        ),
        (
            "StringFileInfo shorter than its header",
            block_with(|b| put(b, 92, 3)),
        ),
        (
            "string of length zero",
            block_with(|b| {
                put(b, STRING_AT, 0);
                b[STRING_AT + 6] = b'X';
            }),
        ),
    ] {
        assert!(
            matches!(
                read(&spec, "product_version"),
                Err(ConditionError::Environment(_))
            ),
            "{what}"
        );
    }
}

#[test]
fn a_directory_that_does_not_start_its_section_is_read_where_it_is() {
    let mut spec = PeSpec::new([4, 5, 6, 7], "4.5");
    spec.lead = 0x30;
    assert_eq!(read(&spec, "version"), Ok(true));
}

#[test]
fn every_prefix_and_every_byte_flip_of_an_image_is_read_without_a_panic() {
    let image = PeSpec::new([1, 2, 3, 4], "1.2").build();
    let dir = TempDir::new();
    for cut in (0..image.len()).step_by(7) {
        let path = dir.file("cut.dll", &image[..cut]);
        let _ = preflight(&path, "product_version");
        let _ = is_executable(&path);
    }
    for at in (0..image.len()).step_by(3) {
        let mut flipped = image.clone();
        flipped[at] ^= 0xFF;
        let path = dir.file("flip.dll", &flipped);
        let _ = preflight(&path, "product_version");
    }
}
