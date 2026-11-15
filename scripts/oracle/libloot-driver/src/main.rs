// SPDX-License-Identifier: GPL-3.0-or-later
//! `libloot-driver <game path> <local path> <masterlist> <plugin name>...`: load the plugins (in full, so that
//! records and archives are known) from `<game path>/Data`, load the masterlist and print libloot's sorted order as
//! a JSON array, or `{"error": "..."}` when libloot refuses. The names are the current load order.
//!
//! The environment variable `LIBLOOT_DRIVER_EXTRA_DATA`, a list of directories separated as `PATH` is, gives
//! additional data paths: libloot searches them in this order before `Data`, and a plugin that is in one of them
//! is loaded from there.

use std::path::{Path, PathBuf};

use libloot::metadata::{Message, MessageContent, PluginCleaningData, select_message_content};
use libloot::{EvalMode, Game, GameType, MergeMode};

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The userlist named by `LIBLOOT_DRIVER_USERLIST`, loaded into the game's database; true when there is one.
fn load_userlist(game: &Game) -> bool {
    let Some(path) = std::env::var_os("LIBLOOT_DRIVER_USERLIST") else {
        return false;
    };
    game.database()
        .write()
        .expect("the database lock")
        .load_userlist(Path::new(&path))
        .expect("the userlist");
    true
}

fn list(items: Vec<String>) -> String {
    format!("[{}]", items.join(","))
}

fn message_json(message: &Message) -> String {
    let content: &[MessageContent] = message.content();
    // The language asked for (`LIBLOOT_DRIVER_LANGUAGE`), English by default.
    let language = std::env::var("LIBLOOT_DRIVER_LANGUAGE")
        .unwrap_or_else(|_| MessageContent::DEFAULT_LANGUAGE.to_string());
    let text = select_message_content(content, &language).map_or("", MessageContent::text);
    format!(
        "{{\"type\":{},\"text\":{}}}",
        quote(&message.message_type().to_string()),
        quote(text)
    )
}

fn cleaning_json(plugin: &str, status: &str, data: &PluginCleaningData) -> String {
    format!(
        "{{\"plugin\":{},\"crc\":{},\"status\":\"{status}\",\"utility\":{},\"itm\":{},\"udr\":{},\"nav\":{}}}",
        quote(plugin),
        data.crc(),
        quote(data.cleaning_utility()),
        data.itm_count(),
        data.deleted_reference_count(),
        data.deleted_navmesh_count()
    )
}

fn metadata(args: &[String]) {
    let names: Vec<&str> = args[3..].iter().map(String::as_str).collect();
    let mut game = Game::with_local_path(
        GameType::FalloutNV,
        Path::new(&args[0]),
        Path::new(&args[1]),
    )
    .expect("a game handle (the game directory needs Data/FalloutNV.esm)");
    let extra: Vec<PathBuf> = std::env::var_os("LIBLOOT_DRIVER_EXTRA_DATA")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    game.set_additional_data_paths(extra.clone())
        .expect("the additional data paths");
    let active_file = game.active_plugins_file_path().clone();
    std::fs::create_dir_all(active_file.parent().expect("a parent")).expect("the local directory");
    std::fs::write(&active_file, names.join("\n") + "\n").expect("the active plugins file");
    game.load_current_load_order_state()
        .expect("the load order state");
    game.database()
        .write()
        .expect("the database lock")
        .load_masterlist(Path::new(&args[2]))
        .expect("the masterlist");
    let mode = if load_userlist(&game) {
        MergeMode::WithUserMetadata
    } else {
        MergeMode::WithoutUserMetadata
    };
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
    let database = game.database();
    let database = database.read().expect("the database lock");
    let general = database
        .general_messages(mode, EvalMode::Evaluate)
        .expect("general messages");
    let groups: Vec<String> = database
        .groups(mode)
        .iter()
        .map(|group| {
            let after: Vec<String> = group
                .after_groups()
                .iter()
                .map(|name| quote(name))
                .collect();
            format!(
                "{{\"name\":{},\"after\":{}}}",
                quote(group.name()),
                list(after)
            )
        })
        .collect();
    let mut plugins = Vec::new();
    let mut dirty = Vec::new();
    for name in &names {
        let found = database
            .plugin_metadata(name, mode, EvalMode::Evaluate)
            .expect("plugin metadata");
        let (mut group, mut messages, mut add, mut remove) =
            ("default".to_string(), Vec::new(), Vec::new(), Vec::new());
        if let Some(found) = found {
            if let Some(name) = found.group() {
                group = name.to_string();
            }
            messages = found.messages().iter().map(message_json).collect();
            for tag in found.tags() {
                if tag.is_addition() {
                    add.push(quote(tag.name()))
                } else {
                    remove.push(quote(tag.name()))
                }
            }
            let crc = game.plugin(name).and_then(|plugin| plugin.crc());
            for (status, entries) in [("dirty", found.dirty_info()), ("clean", found.clean_info())]
            {
                for data in entries.iter().filter(|data| Some(data.crc()) == crc) {
                    dirty.push(cleaning_json(name, status, data));
                }
            }
        }
        plugins.push(format!(
            "{}:{{\"group\":{},\"messages\":{},\"tags_add\":{},\"tags_remove\":{}}}",
            quote(name),
            quote(&group),
            list(messages),
            list(add),
            list(remove)
        ));
    }
    println!(
        "{{\"general_messages\":{},\"groups\":{},\"plugins\":{{{}}},\"dirty\":{}}}",
        list(general.iter().map(message_json).collect()),
        list(groups),
        plugins.join(","),
        list(dirty)
    );
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--metadata") {
        if args.len() < 6 {
            eprintln!(
                "usage: libloot-driver --metadata <game path> <local path> <masterlist> <plugin>..."
            );
            std::process::exit(2);
        }
        metadata(&args[2..]);
        return;
    }
    args.shrink_to_fit();
    if args.len() < 5 {
        eprintln!("usage: libloot-driver <game path> <local path> <masterlist> <plugin>...");
        std::process::exit(2);
    }
    let names: Vec<&str> = args[4..].iter().map(String::as_str).collect();
    let mut game = Game::with_local_path(
        GameType::FalloutNV,
        Path::new(&args[1]),
        Path::new(&args[2]),
    )
    .expect("a game handle (the game directory needs Data/FalloutNV.esm)");
    game.database()
        .write()
        .expect("the database lock")
        .load_masterlist(Path::new(&args[3]))
        .expect("the masterlist");
    load_userlist(&game);
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
