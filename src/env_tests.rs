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
