#![no_main]

use libfuzzer_sys::fuzz_target;

// plugins.txt is read lossily (as the CLI does), so any bytes are valid input.
fuzz_target!(|data: &[u8]| {
    let slots = loot_rs::env::parse_plugins_txt(&String::from_utf8_lossy(data));
    assert!(slots
        .iter()
        .all(|slot| !slot.name.is_empty() || slot.active));
});
