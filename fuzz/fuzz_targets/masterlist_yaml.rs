#![no_main]

use libfuzzer_sys::fuzz_target;
use loot_rs::condition::parse;
use loot_rs::metadata::{self, FileRef, Message};

fn check(condition: Option<&String>) {
    if let Some(condition) = condition {
        let _ = parse(condition);
    }
}

fn files(refs: &[FileRef]) {
    for file in refs {
        check(file.condition.as_ref());
        check(file.constraint.as_ref());
    }
}

fn messages(messages: &[Message]) {
    for message in messages {
        check(message.condition.as_ref());
    }
}

// Only the absence of panics, runaway recursion and runaway expansion is asserted: an error is a valid outcome for
// arbitrary text. The alias counter and the reader must agree on what is too big (a text the counter accepts is
// read, one it refuses is refused by the reader too), and every condition the masterlist carries is parsed, as
// the checker does.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let counted = metadata::expanded_node_count(text);
    let parsed = metadata::parse(text);
    if counted.is_err() {
        assert!(parsed.is_err(), "the reader accepted what the counter refused");
    }
    if let Ok(metadata) = parsed {
        messages(&metadata.globals);
        for entry in &metadata.plugins {
            messages(&entry.msg);
            files(&entry.after);
            files(&entry.req);
            files(&entry.inc);
            for tag in &entry.tag {
                check(tag.condition.as_ref());
            }
            let _ = entry.pattern.matches("Some Plugin.esp");
        }
    }
});
