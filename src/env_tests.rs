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
