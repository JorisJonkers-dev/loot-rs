#![no_main]

use libfuzzer_sys::fuzz_target;

// Arbitrary bytes must never panic or read out of bounds; an error is a valid outcome.
fuzz_target!(|data: &[u8]| {
    let _ = loot_rs::plugin::header_data_size(data);
    if let Ok(header) = loot_rs::plugin::parse_header(data) {
        let _ = header.is_master();
        let _ = header.masters.len();
    }
});
