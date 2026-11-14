// SPDX-License-Identifier: GPL-3.0-or-later
//! `libloot-driver <game path> <local path> <masterlist> <plugin name>...`: load the plugins (in full, so that
//! records and archives are known) from `<game path>/Data`, load the masterlist and print libloot's sorted order as
//! a JSON array, or `{"error": "..."}` when libloot refuses. The names are the current load order.

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
    let paths: Vec<PathBuf> = names.iter().map(PathBuf::from).collect();
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
