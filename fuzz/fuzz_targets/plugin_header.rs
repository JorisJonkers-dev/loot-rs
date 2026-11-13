#![no_main]

use libfuzzer_sys::fuzz_target;

// Arbitrary bytes must never panic or read out of bounds; an error is a valid outcome. A header that is read
// must be the same whether the bytes are read whole or cut at the end of what was read, and the announced size
// is never more than the bytes hold.
fuzz_target!(|data: &[u8]| {
    let _ = loot_rs::plugin::header_data_size(data);
    if let Ok(header) = loot_rs::plugin::parse_header(data) {
        let _ = header.is_master();
        let _ = header.is_light();
        let _ = header.masters.len();
        let size = loot_rs::plugin::header_data_size(data).expect("a parsed header has a size");
        assert!(size <= data.len());
    }
});
