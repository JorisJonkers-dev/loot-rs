// SPDX-License-Identifier: GPL-3.0-only
//! The synthetic comparison: plugins built with loot-rs's own builders (and the extra pieces below for the corner
//! cases of the format), read by loot-rs and by esplugin, and required to agree. Invented bytes only.

use std::path::PathBuf;

use esplugin_diff::compare_all;
use loot_rs::plugin::{FLAG_ESM, FLAG_LOCALIZED, synthetic_plugin};

const COMPRESSED: u32 = 0x0004_0000;
const DELETED: u32 = 0x20;

/// One record: header (24 bytes) and `data` bytes (not valid subrecords unless the test says so).
fn record(kind: &[u8; 4], flags: u32, form_id: u32, data: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&form_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&15u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// A group holding `children` (records and groups); its size counts its own header.
fn group(label: &[u8; 4], group_type: u32, children: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    out.extend_from_slice(&u32::try_from(24 + children.len()).unwrap().to_le_bytes());
    out.extend_from_slice(label);
    out.extend_from_slice(&group_type.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(children);
    out
}

/// A plugin: the repository's header builder, the HEDR record count set to `count` and the version to `version`,
/// then `body`.
fn plugin(
    flags: u32,
    masters: &[&str],
    description: Option<&str>,
    count: u32,
    version: f32,
    body: &[u8],
) -> Vec<u8> {
    let mut out = synthetic_plugin(flags, masters, description);
    // TES4 header (24) + "HEDR" + size (6) + version (4) + count (4)
    out[30..34].copy_from_slice(&version.to_le_bytes());
    out[34..38].copy_from_slice(&count.to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// The records of one group of NPC_ records with these form ids.
fn npcs(form_ids: &[u32]) -> Vec<u8> {
    let records: Vec<u8> = form_ids
        .iter()
        .flat_map(|id| record(b"NPC_", 0, *id, &[0; 6]))
        .collect();
    group(b"NPC_", 0, &records)
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("esplugin-diff-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Compare every plugin and every pair; panics with the differences, naming no real data.
fn check(tag: &str, plugins: &[(&str, Vec<u8>)]) {
    let dir = scratch(tag);
    for (name, bytes) in plugins {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let names: Vec<String> = plugins
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    let (count, found) = compare_all(&dir, &names);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(count, plugins.len());
    assert!(
        found.is_empty(),
        "{tag}: header {:#?} records {:#?}",
        found.header,
        found.records
    );
}

#[test]
fn header_fields() {
    check(
        "header",
        &[
            ("A.esm", plugin(FLAG_ESM, &[], Some("plain"), 0, 1.34, &[])),
            (
                "B.esp",
                plugin(
                    0,
                    &["A.esm", "Other.esm"],
                    Some("with masters"),
                    3,
                    0.94,
                    &[],
                ),
            ),
            ("C.esp", plugin(0, &[], None, 0, 1.0, &[])),
            (
                "D.esp",
                plugin(0, &["A.esm"], Some("caf\u{e9} \u{fc}ber"), 9, 1.5, &[]),
            ),
            (
                "E.esp",
                plugin(FLAG_LOCALIZED, &["A.esm"], Some("localised"), 1, 1.34, &[]),
            ),
            (
                "F.esm",
                plugin(FLAG_ESM | FLAG_LOCALIZED, &[], Some(""), 0, 1.34, &[]),
            ),
        ],
    );
}

#[test]
fn records_and_overrides() {
    // Master indexes: 0 is A.esm, 1 is B.esp, 2 is the plugin itself (the list has two masters); 5 is past the list.
    let a = plugin(
        FLAG_ESM,
        &[],
        None,
        2,
        1.34,
        &npcs(&[0x0000_0800, 0x0000_0801]),
    );
    let b = plugin(
        0,
        &["A.esm"],
        None,
        3,
        1.34,
        &npcs(&[0x0000_0800, 0x0100_0801, 0x0100_0802]),
    );
    let c = plugin(
        0,
        &["A.esm", "B.esp"],
        None,
        7,
        1.34,
        &npcs(&[
            0x0000_0800,
            0x0100_0801,
            0x0200_0900,
            0x0300_0901,
            0x0500_0902,
        ]),
    );
    check("records", &[("A.esm", a), ("B.esp", b), ("C.esp", c)]);
}

#[test]
fn own_master_index_and_unused_masters() {
    // The master index equals the number of masters: the plugin's own record, not an override.
    let own = plugin(
        0,
        &["A.esm"],
        None,
        2,
        1.34,
        &npcs(&[0x0100_0800, 0x0100_0801]),
    );
    // Many masters, only the first one used.
    let masters: Vec<String> = (0..12).map(|i| format!("M{i}.esm")).collect();
    let refs: Vec<&str> = masters.iter().map(String::as_str).collect();
    let few = plugin(0, &refs, None, 1, 1.34, &npcs(&[0x0000_0800]));
    let m0 = plugin(FLAG_ESM, &[], None, 1, 1.34, &npcs(&[0x0000_0800]));
    check(
        "own",
        &[
            ("A.esm", m0.clone()),
            ("Own.esp", own),
            ("M0.esm", m0),
            ("Few.esp", few),
        ],
    );
}

#[test]
fn nested_groups() {
    let leaf = [
        record(b"REFR", 0, 0x0000_0810, &[1, 2, 3]),
        record(b"REFR", 0, 0x0000_0811, &[]),
    ]
    .concat();
    let cell = [
        record(b"CELL", 0, 0x0000_0805, &[0; 4]),
        group(b"\x05\x08\0\0", 6, &leaf),
    ]
    .concat();
    let block = group(b"\x00\0\0\0", 2, &group(b"\x00\0\0\0", 3, &cell));
    let world = [
        record(b"WRLD", 0, 0x0000_0804, &[]),
        group(b"\x04\x08\0\0", 1, &block),
    ]
    .concat();
    let top = group(b"WRLD", 0, &world);
    let master = plugin(FLAG_ESM, &[], None, 9, 1.34, &top);
    // The same structure in a plugin that overrides the master's records.
    let leaf2 = [
        record(b"REFR", 0, 0x0000_0810, &[1]),
        record(b"REFR", 0, 0x0100_0812, &[]),
    ]
    .concat();
    let cell2 = [
        record(b"CELL", 0, 0x0000_0805, &[]),
        group(b"\x05\x08\0\0", 6, &leaf2),
    ]
    .concat();
    let child = group(b"\x00\0\0\0", 2, &group(b"\x00\0\0\0", 3, &cell2));
    let top2 = group(
        b"WRLD",
        0,
        &[
            record(b"WRLD", 0, 0x0000_0804, &[]),
            group(b"\x04\x08\0\0", 1, &child),
        ]
        .concat(),
    );
    let patch = plugin(0, &["A.esm"], None, 8, 1.34, &top2);
    check("nested", &[("A.esm", master), ("Patch.esp", patch)]);
}

#[test]
fn deleted_and_compressed_records() {
    let records = [
        record(b"NPC_", DELETED, 0x0000_0800, &[]),
        record(
            b"NPC_",
            COMPRESSED,
            0x0000_0801,
            &[0x10, 0, 0, 0, 0x78, 0x9c, 3, 0, 0, 0, 0, 1],
        ),
        record(b"NPC_", DELETED | COMPRESSED, 0x0000_0802, &[0, 0, 0, 0]),
        record(b"NPC_", 0, 0x0000_0803, &[7; 5]),
    ]
    .concat();
    let master = plugin(FLAG_ESM, &[], None, 5, 1.34, &group(b"NPC_", 0, &records));
    let patch_records = [
        record(b"NPC_", DELETED, 0x0000_0800, &[]),
        record(
            b"NPC_",
            COMPRESSED,
            0x0000_0801,
            &[0x10, 0, 0, 0, 0x78, 0x9c, 3, 0, 0, 0, 0, 1],
        ),
        record(b"NPC_", 0, 0x0000_0899, &[]),
    ]
    .concat();
    let patch = plugin(
        0,
        &["A.esm"],
        None,
        4,
        1.34,
        &group(b"NPC_", 0, &patch_records),
    );
    check("deleted", &[("A.esm", master), ("Patch.esp", patch)]);
}

#[test]
fn repeated_form_ids_and_zero() {
    // The same form id twice in one plugin, and a record whose form id is 0.
    let body = [
        record(b"NPC_", 0, 0x0100_0800, &[]),
        record(b"NPC_", 0, 0x0100_0800, &[]),
        record(b"NPC_", 0, 0, &[]),
    ]
    .concat();
    let a = plugin(0, &["M.esm"], None, 4, 1.34, &group(b"NPC_", 0, &body));
    let m = plugin(FLAG_ESM, &[], None, 1, 1.34, &npcs(&[0x0000_0800]));
    check("repeated", &[("M.esm", m), ("Twice.esp", a)]);
}

#[test]
fn both_refuse_what_is_not_a_plugin() {
    let good = plugin(0, &[], None, 0, 1.34, &npcs(&[0x0000_0800]));
    let mut bad_magic = good.clone();
    bad_magic[..4].copy_from_slice(b"TES3");
    check(
        "refused",
        &[
            ("Good.esp", good),
            ("Magic.esp", bad_magic),
            ("Short.esp", vec![b'T', b'E']),
        ],
    );
}

/// A known difference, pinned so that a change on either side is noticed: a file whose last record is cut off.
/// esplugin seeks past the end of the file and reads what is there; loot-rs refuses the records (a cut-off plugin is
/// reported as unreadable and left out of the overlap edges instead of being read as far as it goes).
#[test]
fn a_cut_off_record_is_refused_by_loot_rs_only() {
    let mut cut = plugin(0, &[], None, 0, 1.34, &npcs(&[0x0000_0800]));
    cut.truncate(cut.len() - 5);
    let dir = scratch("cut");
    std::fs::write(dir.join("Cut.esp"), &cut).unwrap();
    let (_, found) = compare_all(&dir, &["Cut.esp".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(found.header.is_empty(), "{:?}", found.header);
    assert_eq!(found.records.len(), 1, "{:?}", found.records);
    assert!(found.records[0].contains("only esplugin reads the records"));
}

/// A small deterministic generator (no extra dependency).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        u32::try_from((self.0 >> 33) % u64::from(below)).unwrap()
    }
}

#[test]
fn random_plugins() {
    let mut random = Lcg(7);
    for round in 0..40 {
        let count = 2 + random.next(5) as usize;
        let mut plugins: Vec<(String, Vec<u8>)> = Vec::new();
        for index in 0..count {
            let name = if index == 0 {
                "P0.esm".to_string()
            } else {
                format!("P{index}.esp")
            };
            let master_names: Vec<String> = (0..index)
                .filter(|_| random.next(3) == 0)
                .map(|i| plugins[i].0.clone())
                .collect();
            let masters: Vec<&str> = master_names.iter().map(String::as_str).collect();
            let mut body = Vec::new();
            for _ in 0..random.next(3) {
                let mut records = Vec::new();
                for _ in 0..random.next(6) {
                    // Master indexes up to two past the end of the list: own records.
                    let index = random.next(u32::try_from(masters.len()).unwrap() + 3);
                    let flags = [0, 0, 0, DELETED, COMPRESSED][random.next(5) as usize];
                    records.extend(record(
                        b"NPC_",
                        flags,
                        (index << 24) | (0x800 + random.next(5)),
                        &vec![0; random.next(4) as usize],
                    ));
                }
                body.extend(group(b"NPC_", 0, &records));
            }
            let flags = if index == 0 { FLAG_ESM } else { 0 };
            let description = (random.next(2) == 0).then(|| format!("d{round}{index}"));
            plugins.push((
                name,
                plugin(
                    flags,
                    &masters,
                    description.as_deref(),
                    random.next(50),
                    1.34,
                    &body,
                ),
            ));
        }
        let borrowed: Vec<(&str, Vec<u8>)> = plugins
            .iter()
            .map(|(n, b)| (n.as_str(), b.clone()))
            .collect();
        check(&format!("random{round}"), &borrowed);
    }
}

#[test]
fn the_override_count_is_what_the_masters_say() {
    // Two masters: indexes 0 and 1 override, 2 (the plugin itself) and 3, 5 (past the list) are its own records.
    let c = plugin(
        0,
        &["A.esm", "B.esp"],
        None,
        7,
        1.34,
        &npcs(&[
            0x0000_0800,
            0x0100_0801,
            0x0200_0900,
            0x0300_0901,
            0x0500_0902,
        ]),
    );
    let dir = scratch("count");
    std::fs::write(dir.join("C.esp"), &c).unwrap();
    let mut names = loot_rs::records::NameIds::default();
    let (both, found) = esplugin_diff::compare_plugin(&dir.join("C.esp"), "C.esp", &mut names);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(found.is_empty());
    assert_eq!(both.records.unwrap().overridden, 2);
    assert_eq!(both.esplugin.unwrap().count_override_records().unwrap(), 2);
}

/// Descriptions are Windows-1252, decoded alike: the bytes 0x80 to 0x9f are typographic characters (0x93 is U+201C
/// for both readers), which `description_contains()` compares.
#[test]
fn windows_1252_descriptions_are_decoded_alike() {
    let mut bytes = plugin(0, &[], Some("XYZ"), 0, 1.34, &[]);
    let at = bytes
        .windows(3)
        .position(|window| window == b"XYZ")
        .unwrap();
    bytes[at..at + 3].copy_from_slice(&[0x93, b'q', 0x94]);
    let dir = scratch("cp1252");
    std::fs::write(dir.join("Q.esp"), &bytes).unwrap();
    let (_, found) = compare_all(&dir, &["Q.esp".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(found.header.is_empty(), "{:?}", found.header);
    assert!(found.records.is_empty());
}
