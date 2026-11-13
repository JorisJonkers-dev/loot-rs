//! Unit tests of `exe_version`: synthetic PE images built here, no game files.

use super::*;
use crate::condition::eval_str;
use crate::env::{FsEnv, Slot};
use crate::support_tests::TempDir;

fn env_with(dir: &TempDir) -> FsEnv {
    FsEnv::new(dir.path(), Vec::<Slot>::new())
}

fn eval(dir: &TempDir, text: &str) -> Result<bool, ConditionError> {
    eval_str(text, &env_with(dir))
}

fn game() -> TempDir {
    let dir = TempDir::new();
    dir.file(
        "nvse_loader.exe",
        &PeSpec::new([1, 4, 0, 2], "1.4.0.2").build(),
    );
    dir
}

#[test]
fn the_fixed_file_version_is_compared_in_both_argument_orders() {
    let dir = game();
    for (text, expected) in [
        (r#"version("nvse_loader.exe", "1.4.0.2", ==)"#, true),
        (r#"version("nvse_loader.exe", ==, "1.4.0.2")"#, true),
        (r#"version("nvse_loader.exe", "1.4", >=)"#, true),
        (r#"version("nvse_loader.exe", "1.4.0.3", <)"#, true),
        (r#"version("nvse_loader.exe", "1.4.0.2", >)"#, false),
        (r#"version("nvse_loader.exe", "2", !=)"#, true),
        (r#"not version("nvse_loader.exe", "1.5", >=)"#, true),
    ] {
        assert_eq!(eval(&dir, text), Ok(expected), "{text}");
    }
}

#[test]
fn the_product_version_string_is_compared_with_loots_grammar() {
    let dir = TempDir::new();
    dir.file("a.dll", &PeSpec::new([0, 0, 0, 0], "2.0.1-beta.2").build());
    for (text, expected) in [
        (r#"product_version("a.dll", "2.0.1", <)"#, true),
        (r#"product_version("a.dll", "2.0.1-beta.2", ==)"#, true),
        (r#"product_version("a.dll", "2.0.0", >)"#, true),
        (r#"product_version("a.dll", "2.0.1-beta.10", <)"#, true),
    ] {
        assert_eq!(eval(&dir, text), Ok(expected), "{text}");
    }
}

#[test]
fn the_path_is_found_case_insensitively_and_below_directories() {
    let dir = TempDir::new();
    dir.file(
        "NVSE/Plugins/Hook.DLL",
        &PeSpec::new([3, 1, 0, 0], "3.1").build(),
    );
    assert_eq!(
        eval(&dir, r#"version("nvse/plugins/hook.dll", "3.1", ==)"#),
        Ok(true)
    );
}

#[test]
fn a_missing_file_is_false() {
    let dir = TempDir::new();
    assert_eq!(eval(&dir, r#"version("nope.exe", "1", >=)"#), Ok(false));
    assert_eq!(
        eval(&dir, r#"product_version("nope.exe", "1", >=)"#),
        Ok(false)
    );
}

#[test]
fn a_file_that_is_not_an_executable_is_an_error_not_a_guess() {
    let dir = TempDir::new();
    dir.file("notes.txt", b"hello, not a PE image at all");
    dir.file("tiny.dll", b"MZ");
    for text in [
        r#"version("notes.txt", "1", ==)"#,
        r#"version("tiny.dll", "1", ==)"#,
    ] {
        assert!(
            matches!(eval(&dir, text), Err(ConditionError::Environment(_))),
            "{text}"
        );
    }
}

#[test]
fn a_plugin_is_still_unsupported() {
    let dir = TempDir::new();
    dir.plugin("A.esp", &[]);
    assert_eq!(
        eval(&dir, r#"version("A.esp", "1", ==)"#),
        Err(ConditionError::Unsupported("version".into()))
    );
    assert_eq!(
        eval(&dir, r#"product_version("Missing.esm", "1", ==)"#),
        Err(ConditionError::Unsupported("product_version".into())),
        "also when the plugin is not installed"
    );
}

#[test]
fn an_image_without_a_resource_directory_has_no_version() {
    let dir = TempDir::new();
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.directories = 2;
    dir.file("bare.exe", &spec.build());
    assert_eq!(eval(&dir, r#"version("bare.exe", "1", ==)"#), Ok(false));
    spec.directories = 16;
    spec.directory_size = Some(0);
    dir.file("empty.exe", &spec.build());
    assert_eq!(eval(&dir, r#"version("empty.exe", "1", ==)"#), Ok(false));
}

#[test]
fn a_bad_call_is_a_syntax_error() {
    let dir = game();
    assert!(matches!(
        evaluate("version", &[Arg::Str("x".into())], None),
        Err(ConditionError::Syntax(_))
    ));
    assert!(matches!(
        eval(&dir, r#"version(nvse_loader, "1", ==)"#),
        Err(ConditionError::Syntax(_))
    ));
    assert!(matches!(
        eval(&dir, r#"version("nvse_loader.exe", "1", ===)"#),
        Err(ConditionError::Syntax(_))
    ));
    dir.file("q\"x.exe", &PeSpec::new([1, 0, 0, 0], "1").build());
    assert!(matches!(
        eval(&dir, r#"version("q\"x.exe", "1", ==)"#),
        Err(ConditionError::Syntax(_) | ConditionError::Environment(_))
    ));
}

fn refused(spec: &PeSpec) -> bool {
    let dir = TempDir::new();
    dir.file("h.exe", &spec.build());
    matches!(
        eval(&dir, r#"version("h.exe", "1", ==)"#),
        Err(ConditionError::Environment(_))
    )
}

#[test]
fn sizes_that_come_from_the_file_are_bounded() {
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.directories = u32::MAX;
    assert!(refused(&spec), "number of data directories");
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.directory_size = Some(u32::MAX);
    assert!(refused(&spec), "resource directory size");
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.leaf_size = Some(u32::MAX);
    assert!(refused(&spec), "version block size");
}

#[test]
fn a_directory_that_overflows_32_bits_is_refused() {
    let mut image = PeSpec::new([1, 0, 0, 0], "1").build();
    let section = 0x58 + 224;
    image[section + 12..section + 16].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes()); // section va
    image[section + 8..section + 12].copy_from_slice(&0x1000u32.to_le_bytes());
    let dir = TempDir::new();
    dir.file("o.exe", &image);
    assert!(matches!(
        eval(&dir, r#"version("o.exe", "1", ==)"#),
        Err(ConditionError::Environment(_)) | Ok(false)
    ));
}

#[test]
fn a_string_table_that_would_loop_forever_is_not_followed() {
    // A StringFileInfo whose only string has length 0 and is not ProductVersion: the interpreter's own walk
    // would never advance. The walk here refuses it before the interpreter is called.
    let mut block = version_block_bytes([1, 0, 0, 0], "1");
    let string = block.len() - 40; // the ProductVersion string starts 40 bytes before the end
    let key = utf16_key_bytes("Xroduct");
    block[string + 6..string + 6 + key.len()].copy_from_slice(&key);
    block[string..string + 2].copy_from_slice(&0u16.to_le_bytes());
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    spec.block = block;
    let dir = TempDir::new();
    dir.file("loop.dll", &spec.build());
    let started = std::time::Instant::now();
    assert_eq!(
        eval(&dir, r#"product_version("loop.dll", "1", ==)"#),
        Ok(false)
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

fn utf16_key_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn a_block_without_product_version_is_false_for_product_version_only() {
    let mut block = version_block_bytes([1, 2, 0, 0], "9");
    let at = block.windows(2).position(|w| w == b"P\0").expect("key");
    block[at] = b'Q';
    let mut spec = PeSpec::new([1, 2, 0, 0], "9");
    spec.block = block;
    let dir = TempDir::new();
    dir.file("n.dll", &spec.build());
    assert_eq!(eval(&dir, r#"product_version("n.dll", "0", >)"#), Ok(false));
    assert_eq!(eval(&dir, r#"version("n.dll", "1.2", ==)"#), Ok(true));
}

#[test]
fn a_truncated_block_never_panics_or_hangs() {
    let full = PeSpec::new([1, 2, 3, 4], "5.6").build();
    let dir = TempDir::new();
    for cut in (0..full.len()).step_by(7) {
        dir.file("t.dll", &full[..cut]);
        for text in [
            r#"version("t.dll", "1", ==)"#,
            r#"product_version("t.dll", "1", ==)"#,
        ] {
            let _ = eval(&dir, text);
        }
    }
}

#[test]
fn every_single_byte_flip_of_the_version_block_is_survived() {
    let spec = PeSpec::new([1, 2, 3, 4], "5.6");
    let dir = TempDir::new();
    for at in 0..spec.block.len() {
        for value in [0u8, 0xFF] {
            let mut flipped = spec.clone();
            flipped.block[at] = value;
            dir.file("f.dll", &flipped.build());
            for text in [
                r#"version("f.dll", "1", ==)"#,
                r#"product_version("f.dll", "1", ==)"#,
            ] {
                let _ = eval(&dir, text);
            }
        }
    }
}
