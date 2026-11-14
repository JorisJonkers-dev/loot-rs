// SPDX-License-Identifier: GPL-3.0-or-later
//! `libloot-driver <game path> <local path> <masterlist> <plugin name>...`: load the plugins (in full, so that
//! records and archives are known) from `<game path>/Data`, load the masterlist and print libloot's sorted order as
//! a JSON array, or `{"error": "..."}` when libloot refuses. The names are the current load order.
//!
//! The environment variable `LIBLOOT_DRIVER_EXTRA_DATA`, a list of directories separated as `PATH` is, gives
//! additional data paths: libloot searches them in this order before `Data`, and a plugin that is in one of them
//! is loaded from there.

use std::path::{Path, PathBuf};

use libloot::{Game, GameType};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: libloot-driver <game path> <local path> <masterlist> <plugin>...");
        std::process::exit(2);
    }
    let names: Vec<&str> = args[4..].iter().map(String::as_str).collect();
    let mut game = Game::with_local_path(GameType::FalloutNV, Path::new(&args[1]), Path::new(&args[2]))
        .expect("a game handle (the game directory needs Data/FalloutNV.esm)");
    game.database()
        .write()
        .expect("the database lock")
        .load_masterlist(Path::new(&args[3]))
        .expect("the masterlist");
    let extra: Vec<PathBuf> = std::env::var_os("LIBLOOT_DRIVER_EXTRA_DATA")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    game.set_additional_data_paths(extra.clone())
        .expect("the database lock");
    // A plugin in an additional directory is loaded by its full path; the first directory that has it wins.
    let paths: Vec<PathBuf> = names
        .iter()
        .map(|name| {
            extra
                .iter()
                .map(|dir| dir.join(name))
                .find(|path| path.exists())
                .unwrap_or_else(|| PathBuf::from(name))
        })
        .collect();
    let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    game.load_plugins(&refs).expect("the plugins");
    match game.sort_plugins(&names) {
        Ok(sorted) => {
            let quoted: Vec<String> = sorted.iter().map(|name| format!("{name:?}")).collect();
            println!("[{}]", quoted.join(","));
        }
        Err(error) => println!("{{\"error\":{:?}}}", error.to_string()),
    }
}
