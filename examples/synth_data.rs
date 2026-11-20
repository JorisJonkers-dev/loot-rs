// SPDX-License-Identifier: GPL-3.0-only
//! Write the synthetic `Data` directory a golden case describes (`data.txt`) into a real directory, so another
//! program (the LOOT application, see `scripts/loot-app-capture/`) can read it.
//!
//! `cargo run --example synth_data -- tests/golden_app/<case>/data.txt <Data directory>`

#[path = "../tests/common/mod.rs"]
mod common;

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(spec), Some(data)) = (args.next(), args.next()) else {
        eprintln!("usage: synth_data <data.txt> <Data directory>");
        std::process::exit(2);
    };
    let spec = std::fs::read_to_string(&spec).expect("read data.txt");
    let data = PathBuf::from(data);
    // The fixture's `Data` directory is `<root>/Data`; point it at the parent of the one asked for.
    let root = data
        .parent()
        .expect("Data directory has a parent")
        .to_path_buf();
    assert_eq!(data.file_name().and_then(|n| n.to_str()), Some("Data"));
    std::fs::create_dir_all(&data).expect("create Data");
    let fixture = common::Fixture { root };
    common::build_data(&fixture, &spec);
    // The fixture removes its root when dropped; this is a real directory, so keep it.
    std::mem::forget(fixture);
}
