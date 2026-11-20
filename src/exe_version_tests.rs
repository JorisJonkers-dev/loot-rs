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
fn every_comparator_holds_and_fails_where_it_should_for_both_functions() {
    let dir = TempDir::new();
    dir.file("a.dll", &PeSpec::new([1, 4, 0, 2], "1.4.0.2").build());
    // (comparator, version below, equal, above): the result of `file <comparator> given`.
    for (op, below, equal, above) in [
        ("==", false, true, false),
        ("!=", true, false, true),
        ("<", false, false, true),
        (">", true, false, false),
        ("<=", false, true, true),
        (">=", true, true, false),
    ] {
        for function in ["version", "product_version"] {
            for (given, expected) in [("1.3.0.0", below), ("1.4.0.2", equal), ("1.5.0.0", above)] {
                let text = format!(r#"{function}("a.dll", {op}, "{given}")"#);
                assert_eq!(eval(&dir, &text), Ok(expected), "{text}");
                let text = format!(r#"{function}("a.dll", "{given}", {op})"#);
                assert_eq!(eval(&dir, &text), Ok(expected), "{text}");
            }
        }
    }
}

#[test]
fn a_masterlist_guard_on_a_script_extender_beside_the_data_directory_is_evaluated() {
    let dir = TempDir::new();
    let nvse = |version: [u16; 4], text: &str| {
        dir.file("game/nvse_1_4.dll", &PeSpec::new(version, text).build());
    };
    dir.file("game/Data/readme.txt", b"x");
    nvse([0, 5, 0, 0], "0.5");
    // A fresh environment per evaluation: one caches what it has read.
    let env = || {
        FsEnv::new(&dir.path().join("game/Data"), Vec::<Slot>::new())
            .with_game_dir(Some(&dir.path().join("game")))
    };
    let guard = r#"not version("../nvse_1_4.dll", >=, "0.6.0.0")"#;
    assert_eq!(eval_str(guard, &env()), Ok(true));
    nvse([0, 6, 0, 0], "0.6");
    assert_eq!(eval_str(guard, &env()), Ok(false));
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
fn a_plugin_has_no_product_version() {
    let dir = TempDir::new();
    dir.plugin("A.esp", &[]);
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

/// Run `work` on another thread and fail the test, instead of hanging it, when it takes longer than `secs`.
fn within<T: Send + 'static>(secs: u64, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver
        .recv_timeout(std::time::Duration::from_secs(secs))
        .expect("finished within the time limit")
}

/// The product version string of a block starts at 152: 92 (`StringFileInfo`) + 36 + 24 (table header).
const STRING_AT: usize = 152;

fn block_with(edit: impl FnOnce(&mut Vec<u8>)) -> PeSpec {
    let mut spec = PeSpec::new([1, 0, 0, 0], "1");
    edit(&mut spec.block);
    spec
}

fn product(spec: &PeSpec) -> Result<bool, ConditionError> {
    let dir = TempDir::new();
    dir.file("m.dll", &spec.build());
    let text = r#"product_version("m.dll", "1", ==)"#;
    within(20, move || eval(&dir, text))
}

#[test]
fn a_string_whose_length_is_zero_is_refused_not_followed() {
    // The interpreter alone loops forever on this block (see the ignored test below).
    let spec = block_with(|block| {
        block[STRING_AT..STRING_AT + 2].copy_from_slice(&0u16.to_le_bytes());
        block[STRING_AT + 6] = b'X';
    });
    assert!(matches!(
        product(&spec),
        Err(ConditionError::Environment(_))
    ));
}

/// Confirms the claim in the module documentation. Ignored because the thread it leaks spins until the test
/// process ends; run it with `cargo test --features executable-versions -- --ignored` under a timeout.
#[test]
#[ignore = "demonstrates a hang in loot-condition-interpreter"]
fn the_interpreter_alone_loops_forever_on_a_string_of_length_zero() {
    let spec = block_with(|block| {
        block[STRING_AT..STRING_AT + 2].copy_from_slice(&0u16.to_le_bytes());
        block[STRING_AT + 6] = b'X';
    });
    let dir = TempDir::new();
    dir.file("e.dll", &spec.build());
    let path = dir.path().to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let state = loot_condition_interpreter::State::new(
            loot_condition_interpreter::GameType::FalloutNV,
            path,
        );
        let expression: loot_condition_interpreter::Expression =
            r#"product_version("e.dll", "1", ==)"#.parse().expect("parses");
        let _ = sender.send(expression.eval(&state).is_ok());
    });
    assert!(
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_err(),
        "it returned"
    );
}

#[test]
fn string_tables_with_lengths_that_do_not_fit_are_errors_not_clamped() {
    let put = |block: &mut Vec<u8>, at: usize, value: u16| {
        block[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
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
    ] {
        assert!(
            matches!(product(&spec), Err(ConditionError::Environment(_))),
            "{what}"
        );
    }
    assert_eq!(product(&PeSpec::new([1, 0, 0, 0], "1")), Ok(true));
}

#[test]
fn the_version_block_itself_is_validated_for_both_functions() {
    let put = |block: &mut Vec<u8>, at: usize, value: u16| {
        block[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
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
        let dir = TempDir::new();
        dir.file("b.dll", &spec.build());
        for text in [
            r#"version("b.dll", "1", ==)"#,
            r#"product_version("b.dll", "1", ==)"#,
        ] {
            assert!(
                matches!(eval(&dir, text), Err(ConditionError::Environment(_))),
                "{what}: {text}"
            );
        }
    }
}

#[test]
fn a_directory_that_does_not_start_its_section_is_read_where_it_is() {
    let dir = TempDir::new();
    let mut spec = PeSpec::new([4, 5, 6, 7], "4.5");
    spec.lead = 0x30;
    dir.file("lead.dll", &spec.build());
    assert_eq!(
        eval(&dir, r#"version("lead.dll", "4.5.6.7", ==)"#),
        Ok(true)
    );
    assert_eq!(
        eval(&dir, r#"product_version("lead.dll", "4.5", ==)"#),
        Ok(true)
    );
}

#[test]
fn the_pe_header_offset_is_32_bits() {
    let dir = TempDir::new();
    let mut spec = PeSpec::new([2, 0, 0, 0], "2");
    spec.pe_offset = 0x1_0040;
    dir.file("far.dll", &spec.build());
    assert_eq!(eval(&dir, r#"version("far.dll", "2", ==)"#), Ok(true));
    let mut image = PeSpec::new([2, 0, 0, 0], "2").build();
    image[0x3C..0x40].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
    dir.file("beyond.dll", &image);
    assert!(matches!(
        eval(&dir, r#"version("beyond.dll", "2", ==)"#),
        Err(ConditionError::Environment(_))
    ));
}

#[test]
fn the_file_name_is_never_part_of_what_the_interpreter_parses() {
    let dir = TempDir::new();
    let image = PeSpec::new([1, 2, 0, 0], "1.2").build();
    for name in [
        "q\"x.exe",
        "back\\slash.dll",
        "tab\there.dll",
        "new\nline.dll",
        "par(en).dll",
    ] {
        dir.file(name, &image);
        let text = format!("version(\"{}\", \"1.2\", ==)", name.replace('\\', "/"));
        let env = env_with(&dir);
        let args = [
            Arg::Str(name.into()),
            Arg::Str("1.2".into()),
            Arg::Word("==".into()),
        ];
        let path = dir.path().join(name);
        assert_eq!(evaluate("version", &args, Some(&path)), Ok(true), "{name}");
        let _ = (text, env);
    }
}

#[test]
fn versions_and_operators_that_could_not_be_rebuilt_as_text_are_syntax_errors() {
    let dir = game();
    let path = dir.path().join("nvse_loader.exe");
    for args in [
        [
            Arg::Str("x".into()),
            Arg::Str("1\n2".into()),
            Arg::Word("==".into()),
        ],
        [
            Arg::Str("x".into()),
            Arg::Str("1\u{7f}".into()),
            Arg::Word("==".into()),
        ],
        [
            Arg::Str("x".into()),
            Arg::Str("1".into()),
            Arg::Word("=~".into()),
        ],
        [
            Arg::Str("x".into()),
            Arg::Word("1".into()),
            Arg::Word("==".into()),
        ],
    ] {
        assert!(
            matches!(
                evaluate("version", &args, Some(&path)),
                Err(ConditionError::Syntax(_))
            ),
            "{args:?}"
        );
    }
}

#[test]
#[cfg(unix)]
fn the_scratch_directory_is_private_and_removed_with_its_content() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new().expect("creates");
    let path = scratch.0.clone();
    std::fs::write(path.join("v.dll"), b"x").expect("writes");
    assert_eq!(
        std::fs::metadata(&path)
            .expect("exists")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_ne!(Scratch::new().expect("creates").0, path);
    drop(scratch);
    assert!(!path.exists());
}

#[test]
fn evaluating_leaves_no_scratch_directory_behind() {
    let dir = game();
    let before = scratch_dirs();
    for _ in 0..5 {
        assert_eq!(
            eval(&dir, r#"version("nvse_loader.exe", "1.4.0.2", ==)"#),
            Ok(true)
        );
    }
    // Other tests of this process create and remove scratch directories at the same time. One of theirs is there for
    // a moment (empty, or with its file), a leak of this test stays: wait for the others to finish.
    let leaked = || {
        scratch_dirs()
            .into_iter()
            .filter(|path| !before.contains(path))
            .collect::<Vec<_>>()
    };
    for _ in 0..500 {
        if leaked().is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(leaked(), Vec::<std::path::PathBuf>::new());
}

fn scratch_dirs() -> Vec<std::path::PathBuf> {
    let me = format!("loot-rs-pe-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .expect("lists")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&me))
        })
        .collect()
}

/// A resource tree in a `Vec`, read the way `version_block` reads the file.
fn walk(tree: &[u8], size: u32) -> Result<Option<(u32, u32)>, &'static str> {
    find_version_entry(
        &|at, len| {
            tree.get(usize::try_from(at).ok()?..usize::try_from(at).ok()? + len)
                .map(<[u8]>::to_vec)
        },
        size,
    )
}

fn table(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut out = vec![0; 12];
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&u16::try_from(entries.len()).expect("few").to_le_bytes());
    for (id, target) in entries {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&target.to_le_bytes());
    }
    out
}

fn leaf(rva: u32, size: u32) -> Vec<u8> {
    let mut out = rva.to_le_bytes().to_vec();
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out
}

#[test]
fn the_walk_takes_the_first_version_leaf_as_the_interpreter_does() {
    const T: u32 = 0x8000_0000;
    // root @0: icon (id 3) then RT_VERSION -> names @32; names: a data entry (skipped), then a table @56;
    // languages: a table (skipped), then a data entry @80; a second data entry @96 is never looked at.
    let mut tree = table(&[(3, T + 200), (16, T + 32)]); // 32 bytes
    tree.extend(table(&[(1, 40), (2, T + 56)])); // @32, 32 bytes
    tree.extend(table(&[(7, T + 200), (9, 80)])); // @64 ... see below
    let mut padded = tree.clone();
    padded.resize(256, 0);
    // fix the offsets: names table is at 32, its second entry points at 64, the languages table
    padded[32 + 16 + 8 + 4..32 + 16 + 8 + 8].copy_from_slice(&(T + 64).to_le_bytes());
    padded[64 + 16 + 8 + 4..64 + 16 + 8 + 8].copy_from_slice(&96u32.to_le_bytes());
    padded[96..112].copy_from_slice(&leaf(0x1234, 0x40));
    assert_eq!(walk(&padded, 256), Ok(Some((0x1234, 0x40))));
}

#[test]
fn the_walk_bounds_tables_entries_and_offsets() {
    const T: u32 = 0x8000_0000;
    assert_eq!(walk(&table(&[(5, 0)]), 1000), Ok(None), "no RT_VERSION");
    let wide = table(&vec![(1, 0); usize::from(MAX_TABLE_ENTRIES) + 1]);
    assert!(walk(&wide, 100_000).is_err(), "too many entries");
    assert!(
        walk(&table(&[(16, T + 5000)]), 64).is_err(),
        "table beyond the directory"
    );
    // A tree that points every entry back at the root is cut by the table budget.
    // (the depth is fixed at three, so two entries are cheap; ten make a hundred tables)
    assert!(walk(&table(&[(16, T), (16, T)]), 4096).is_ok());
    let looping = table(&[(16, T); 10]);
    assert_eq!(
        walk(&looping, 4096),
        Err("too many resource directory tables")
    );
    assert!(walk(&[], 0).is_err() || walk(&[], 0) == Ok(None));
}

#[test]
fn offsets_are_computed_in_64_bits() {
    let sections = [(0x1000u32, 0x500u32, 0x200u32)];
    assert_eq!(
        rva_to_offset(&sections, 0x1010, 0x20, 0x10000),
        Ok(Some(0x210))
    );
    assert_eq!(rva_to_offset(&sections, u32::MAX, 0x20, 0x10000), Ok(None));
    assert_eq!(
        rva_to_offset(&sections, 0x1000, u32::MAX, 0x10000),
        Ok(None)
    );
    assert_eq!(
        rva_to_offset(&sections, 0x1000, 0x20, 0x100),
        Err("data is cut off by the end of the file")
    );
    let high = [(u32::MAX - 10, 100, u32::MAX)];
    assert_eq!(
        rva_to_offset(&high, u32::MAX - 5, 4, u64::MAX),
        Ok(Some(u64::from(u32::MAX) + 5))
    );
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
