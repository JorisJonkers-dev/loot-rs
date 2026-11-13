#![no_main]

use libfuzzer_sys::fuzz_target;

// Only the absence of panics and runaway recursion is asserted: a parse error is a valid outcome for
// arbitrary text. When the text is a masterlist, every condition it carries is parsed too, as the checker does.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(metadata) = loot_rs::metadata::parse(text) {
        let messages = metadata
            .globals
            .iter()
            .chain(metadata.plugins.iter().flat_map(|entry| entry.msg.iter()));
        for message in messages {
            if let Some(condition) = &message.condition {
                let _ = loot_rs::condition::parse(condition);
            }
        }
    }
});
