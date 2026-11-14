//! Unit tests of `env`.

use super::*;

#[test]
fn crc32_matches_the_standard_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn plugins_txt_active_only_style() {
    let slots = parse_plugins_txt("# header\r\nA.esm\r\n\r\nB.esp\r\n");
    assert_eq!(slots.len(), 2);
    assert!(slots.iter().all(|slot| slot.active));
}

#[test]
fn plugins_txt_starred_style() {
    let slots = parse_plugins_txt("*A.esm\n B.esp\n*C.esp\n");
    let flags: Vec<bool> = slots.iter().map(|slot| slot.active).collect();
    assert_eq!(flags, [true, false, true]);
    assert_eq!(slots[0].name, "A.esm");
}

use crate::support_tests::TempDir;

fn slot(name: &str, active: bool) -> Slot {
    Slot {
        name: name.into(),
        active,
    }
}

fn reference_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn crc32_matches_a_bitwise_reference_on_known_inputs() {
    for input in [
        &b"a"[..],
        b"abc",
        b"The quick brown fox jumps over the lazy dog",
        &[0; 1000],
        &[0xFF; 257],
    ] {
        assert_eq!(crc32(input), reference_crc32(input));
    }
    assert_eq!(
        crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414F_A339
    );
}

#[test]
fn plugins_txt_skips_comments_and_blank_lines_and_trims() {
    let slots = parse_plugins_txt("# comment\n\n  A.esm  \r\n\t\nB.esp\n");
    let names: Vec<&str> = slots.iter().map(|slot| slot.name.as_str()).collect();
    assert_eq!(names, ["A.esm", "B.esp"]);
    assert_eq!(parse_plugins_txt(""), []);
    assert_eq!(parse_plugins_txt("# only\n# comments\n"), []);
}

#[test]
fn plugins_txt_keeps_the_order_and_the_star_marks_active_only_in_the_star_style() {
    let slots = parse_plugins_txt("*B.esp\n*A.esp\nC.esp\n");
    assert_eq!(
        slots,
        [
            slot("B.esp", true),
            slot("A.esp", true),
            slot("C.esp", false)
        ]
    );
    let slots = parse_plugins_txt("* Spaced.esp\n");
    assert_eq!(slots, [slot("Spaced.esp", true)]);
}

fn world() -> (TempDir, FsEnv) {
    let dir = TempDir::new();
    let data = dir.path().join("Data");
    std::fs::create_dir_all(&data).expect("data");
    dir.plugin("Data/FalloutNV.esm", &[]);
    dir.plugin("Data/Mod.esp", &["FalloutNV.esm"]);
    dir.plugin("Data/Other.esp", &["FalloutNV.esm"]);
    dir.file("Data/NVSE/Plugins/hook.dll", b"0123456789");
    dir.file("Data/textures/a.dds", b"x");
    dir.file("outside.txt", b"hello");
    let env = FsEnv::new(
        &data,
        vec![
            slot("FalloutNV.esm", true),
            slot("Mod.esp", true),
            slot("Other.esp", false),
            slot("Missing.esp", true),
        ],
    );
    (dir, env)
}

#[test]
fn paths_resolve_case_insensitively_and_through_dot_dot() {
    let (dir, env) = world();
    assert_eq!(env.data_dir(), dir.path().join("Data"));
    assert!(env.resolve("nvse/plugins/HOOK.DLL").is_some());
    assert!(env.resolve("NVSE//Plugins/./hook.dll").is_some());
    assert_eq!(
        env.resolve("../outside.txt"),
        Some(dir.path().join("outside.txt"))
    );
    assert_eq!(env.resolve("absent/file"), None);
    assert_eq!(env.resolve(""), Some(dir.path().join("Data")));
}

#[test]
fn installed_active_and_position_use_the_load_order_and_the_directory() {
    let (_dir, env) = world();
    assert!(env.installed("mod.ESP"));
    assert!(!env.installed("Missing.esp"));
    assert!(
        !env.installed("NVSE"),
        "a directory is not an installed plugin"
    );
    assert!(env.is_active("MOD.esp"));
    assert!(!env.is_active("Other.esp"), "inactive");
    assert!(!env.is_active("Missing.esp"), "not installed");
    assert_eq!(env.position("other.esp"), Some(2));
    assert_eq!(env.position("nothing.esp"), None);
    assert_eq!(env.slots().len(), 4);
}

#[test]
fn count_files_handles_plain_paths_and_regexes() {
    let (_dir, env) = world();
    assert_eq!(env.count_files("mod.esp"), Ok(1));
    assert_eq!(env.count_files("nvse/plugins"), Ok(1), "directories count");
    assert_eq!(env.count_files("nothing.esp"), Ok(0));
    assert_eq!(env.count_files(".*\\.esp"), Ok(2));
    assert_eq!(env.count_files("(Mod|Other)\\.esp"), Ok(2));
    assert_eq!(env.count_files("NVSE/Plugins/.*\\.dll"), Ok(1));
    assert_eq!(env.count_files("nowhere/.*\\.dll"), Ok(0));
    assert!(matches!(
        env.count_files("(unclosed.*"),
        Err(ConditionError::Syntax(_))
    ));
}

#[test]
fn count_active_handles_plain_names_and_regexes() {
    let (_dir, env) = world();
    assert_eq!(env.count_active("Mod.esp"), Ok(1));
    assert_eq!(env.count_active("Other.esp"), Ok(0));
    assert_eq!(env.count_active("Missing.esp"), Ok(0));
    assert_eq!(env.count_active(".*\\.esp"), Ok(1));
    assert_eq!(env.count_active(".*\\.es[mp]"), Ok(2));
    assert_eq!(
        env.count_active("sub/.*\\.esp"),
        Ok(0),
        "a directory part never matches"
    );
    assert!(matches!(
        env.count_active("(unclosed.*"),
        Err(ConditionError::Syntax(_))
    ));
}

#[test]
fn file_size_readable_master_and_checksum_read_the_files() {
    let (_dir, env) = world();
    assert_eq!(env.file_size("nvse/plugins/hook.dll"), Some(10));
    assert_eq!(env.file_size("nvse"), None, "a directory has no size");
    assert_eq!(env.file_size("absent"), None);
    assert!(env.readable("textures/a.dds"));
    assert!(env.readable("textures"), "directories are readable");
    assert!(!env.readable("absent"));
    assert!(env.is_master("FalloutNV.esm"));
    assert!(!env.is_master("Mod.esp"));
    assert!(!env.is_master("nvse/plugins/hook.dll"), "not a plugin");
    assert!(!env.is_master("absent.esm"));
    assert_eq!(
        env.checksum("nvse/plugins/hook.dll"),
        Some(crc32(b"0123456789"))
    );
    assert_eq!(env.checksum("nvse"), None);
    assert_eq!(env.checksum("absent"), None);
}

#[test]
fn an_unreadable_directory_is_an_environment_error_for_regex_counts() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, env) = world();
    let locked = dir.path().join("Data/locked");
    std::fs::create_dir_all(&locked).expect("dir");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let readable_anyway = std::fs::read_dir(&locked).is_ok(); // running as root
    let result = env.count_files("locked/.*\\.esp");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    if readable_anyway {
        assert_eq!(result, Ok(0));
    } else {
        assert!(
            matches!(result, Err(ConditionError::Environment(_))),
            "{result:?}"
        );
    }
}

#[test]
fn directory_listings_are_cached() {
    let (dir, env) = world();
    assert_eq!(env.count_files(".*\\.esp"), Ok(2));
    dir.plugin("Data/Late.esp", &[]);
    assert_eq!(
        env.count_files(".*\\.esp"),
        Ok(2),
        "the first listing is reused"
    );
    assert_eq!(
        env.count_files("Late.esp"),
        Ok(1),
        "an exact name is found directly"
    );
    assert_eq!(
        env.count_files("late.esp"),
        Ok(0),
        "a differently cased name is looked up in the cached listing"
    );
}

#[test]
fn crc32_of_a_file_is_the_crc32_of_its_bytes_across_block_boundaries() {
    let dir = TempDir::new();
    for len in [0usize, 1, 65_535, 65_536, 65_537, 200_000] {
        let bytes: Vec<u8> = (0..len)
            .map(|n| u8::try_from(n * 7 % 251).expect("byte"))
            .collect();
        let path = dir.file("f.bin", &bytes);
        assert_eq!(
            crc32_file(&path).expect("reads"),
            crc32(&bytes),
            "{len} bytes"
        );
    }
    assert!(crc32_file(&dir.path().join("missing")).is_err());
}

fn layout() -> (TempDir, TempDir) {
    let game = TempDir::new();
    game.file("Data/placeholder.txt", b"");
    let staging = TempDir::new();
    staging.file("data/placeholder.txt", b"");
    (game, staging)
}

#[test]
fn a_parent_path_reaches_the_game_directory_when_one_is_set() {
    let (game, staging) = layout();
    game.file("x.dll", b"x");
    let data = staging.path().join("data");
    let without = FsEnv::new(&data, vec![]);
    assert_eq!(without.count_files("../x.dll"), Ok(0));
    let with = FsEnv::new(&data, vec![]).with_game_dir(Some(game.path()));
    assert_eq!(with.count_files("../x.dll"), Ok(1));
    assert_eq!(with.count_files("../X.DLL"), Ok(1));
    assert_eq!(with.file_size("../x.dll"), Some(1));
}

#[test]
fn a_parent_path_is_the_parent_of_data_without_a_game_directory() {
    let (game, _staging) = layout();
    game.file("x.dll", b"x");
    let env = FsEnv::new(&game.path().join("Data"), vec![]);
    assert_eq!(env.count_files("../x.dll"), Ok(1));
}

#[test]
fn additional_directories_are_searched_before_data_in_the_order_given() {
    let (game, staging) = layout();
    let (first, second) = (TempDir::new(), TempDir::new());
    second.file("shared.txt", b"22");
    first.file("shared.txt", b"1");
    game.file("Data/shared.txt", b"333");
    game.file("Data/only-data.txt", b"4");
    let env = FsEnv::new(&game.path().join("Data"), vec![])
        .with_additional_data_dirs(&[first.path().to_path_buf(), second.path().to_path_buf()]);
    assert_eq!(env.file_size("shared.txt"), Some(1));
    assert_eq!(env.file_size("only-data.txt"), Some(1));
    let reversed = FsEnv::new(&game.path().join("Data"), vec![])
        .with_additional_data_dirs(&[second.path().to_path_buf(), first.path().to_path_buf()]);
    assert_eq!(reversed.file_size("shared.txt"), Some(2));
    drop(staging);
}

#[test]
fn a_name_pattern_counts_each_name_once_across_directories() {
    let (game, _staging) = layout();
    let extra = TempDir::new();
    extra.file("a1.txt", b"");
    extra.file("a2.txt", b"");
    game.file("Data/A1.txt", b"");
    let env = FsEnv::new(&game.path().join("Data"), vec![])
        .with_additional_data_dirs(&[extra.path().to_path_buf()]);
    assert_eq!(env.count_files("a[0-9]\\.txt"), Ok(2));
}

#[test]
fn a_ghosted_plugin_counts_as_the_plugin() {
    let dir = TempDir::new();
    dir.plugin("Mod.esp.ghost", &[]);
    dir.file("notes.txt.ghost", b"");
    let env = FsEnv::new(dir.path(), vec![slot("mod.esp", true)]);
    assert!(env.installed("Mod.esp"));
    assert!(env.is_active("Mod.esp"));
    assert_eq!(env.count_files("Mod\\.esp"), Ok(1));
    assert_eq!(env.count_files("notes.txt"), Ok(0));
    assert!(env.resolve("Mod.esp").is_some());
}

#[test]
fn names_that_differ_only_in_case_are_a_warning() {
    let dir = TempDir::new();
    dir.file("Thing.dll", b"1");
    dir.file("thing.DLL", b"22");
    dir.file("other.txt", b"");
    let env = FsEnv::new(dir.path(), vec![]);
    let warnings = env.case_warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Thing.dll, thing.DLL"), "{warnings:?}");
    assert!(warnings[0].contains("differ only in case"));
    assert_eq!(env.count_files("THING.dll"), Ok(1));
}

fn described(dir: &TempDir, name: &str, description: &str) {
    dir.file(
        name,
        &crate::plugin::synthetic_plugin(0, &[], Some(description)),
    );
}

fn version_env(dir: &TempDir, names: &[&str]) -> FsEnv {
    FsEnv::new(
        dir.path(),
        names.iter().map(|name| slot(name, true)).collect(),
    )
}

fn holds(env: &FsEnv, text: &str) -> Result<bool, ConditionError> {
    crate::condition::eval_str(text, env)
}

#[test]
fn the_version_of_a_plugin_comes_from_its_description() {
    let dir = TempDir::new();
    described(
        &dir,
        "TaleOfTwoWastelands.esm",
        "Tale of Two Wastelands\r\nVersion: 3.2.0",
    );
    let env = version_env(&dir, &["TaleOfTwoWastelands.esm"]);
    for (text, expected) in [
        (r#"version("TaleOfTwoWastelands.esm", "3.2.0", >=)"#, true),
        (r#"version("TaleOfTwoWastelands.esm", "3.2.1", >=)"#, false),
        (r#"version("TaleOfTwoWastelands.esm", "3.2", ==)"#, true),
        (r#"version("taleoftwowastelands.esm", "3.1", >)"#, true),
        (r#"version("TaleOfTwoWastelands.esm", >=, "3.2.0")"#, true),
        (r#"version("TaleOfTwoWastelands.esm", "3.2.0", !=)"#, false),
    ] {
        assert_eq!(holds(&env, text), Ok(expected), "{text}");
    }
}

#[test]
fn a_plugin_without_a_version_in_its_description_is_false() {
    let dir = TempDir::new();
    described(&dir, "A.esp", "Adds a thing.");
    dir.plugin("B.esp", &[]);
    let env = version_env(&dir, &["A.esp", "B.esp"]);
    for name in ["A.esp", "B.esp"] {
        for operator in ["==", "!=", "<", ">=", ">"] {
            let text = format!(r#"version("{name}", "1.0", {operator})"#);
            assert_eq!(holds(&env, &text), Ok(false), "{text}");
        }
    }
}

#[test]
fn a_plugin_that_is_missing_or_not_in_the_load_order_is_false() {
    let dir = TempDir::new();
    described(&dir, "A.esp", "Version 1.0");
    described(&dir, "Unlisted.esp", "Version 1.0");
    let env = version_env(&dir, &["A.esp", "Gone.esp"]);
    assert_eq!(holds(&env, r#"version("Gone.esp", "1.0", ==)"#), Ok(false));
    assert_eq!(
        holds(&env, r#"version("Unlisted.esp", "1.0", ==)"#),
        Ok(false)
    );
    assert_eq!(holds(&env, r#"version("A.esp", "1.0", ==)"#), Ok(true));
}

#[test]
fn a_plugin_that_cannot_be_read_is_a_condition_error() {
    let dir = TempDir::new();
    dir.file("Broken.esp", b"not a plugin");
    let env = version_env(&dir, &["Broken.esp"]);
    assert!(matches!(
        holds(&env, r#"version("Broken.esp", "1.0", ==)"#),
        Err(ConditionError::Environment(_))
    ));
}

#[test]
fn the_version_of_a_plugin_is_read_from_the_first_directory_that_has_it() {
    let (game, staging) = layout();
    described(&game, "Data/A.esp", "Version 1.0");
    described(&staging, "extra/A.esp", "Version 2.0");
    let env = FsEnv::new(&game.path().join("Data"), vec![slot("A.esp", true)])
        .with_additional_data_dirs(&[staging.path().join("extra")]);
    assert_eq!(holds(&env, r#"version("A.esp", "2.0", ==)"#), Ok(true));
}

#[test]
fn a_ghosted_plugin_has_a_version() {
    let dir = TempDir::new();
    described(&dir, "A.esp.ghost", "Version 1.5");
    let env = version_env(&dir, &["A.esp"]);
    assert_eq!(holds(&env, r#"version("A.esp", "1.5", ==)"#), Ok(true));
}

#[test]
fn a_malformed_plugin_version_call_is_a_syntax_error() {
    let dir = TempDir::new();
    described(&dir, "A.esp", "Version 1.0");
    let env = version_env(&dir, &["A.esp"]);
    for text in [
        r#"version("A.esp", "1.0")"#,
        r#"version("A.esp", "1.0", "1.0")"#,
        r#"version("A.esp", "1.0", =~)"#,
    ] {
        assert!(
            matches!(holds(&env, text), Err(ConditionError::Syntax(_))),
            "{text}"
        );
    }
}

#[cfg(not(feature = "executable-versions"))]
#[test]
fn product_version_of_a_plugin_stays_unsupported() {
    let dir = TempDir::new();
    described(&dir, "A.esp", "Version 1.0");
    let env = version_env(&dir, &["A.esp"]);
    assert_eq!(
        holds(&env, r#"product_version("A.esp", "1.0", ==)"#),
        Err(ConditionError::Unsupported("product_version".into()))
    );
}
