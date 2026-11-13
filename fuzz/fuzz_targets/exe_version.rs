#![no_main]

use libfuzzer_sys::fuzz_target;
use loot_rs::condition::Arg;
use loot_rs::exe_version::{evaluate, preflight, PeSpec};

// A valid synthetic image with bytes overwritten as the input says (three bytes per edit: position and value),
// so the fuzzer reaches the resource walk and the version block without having to invent a PE header. The bounded
// walk and then the interpreter must answer (a value or an error) without panicking, allocating from a size in the
// file or looping (libFuzzer's own timeout catches that).
fuzz_target!(|data: &[u8]| {
    let mut image = PeSpec::new([1, 2, 3, 4], "1.2.3-beta").build();
    for edit in data.chunks_exact(3) {
        let at = usize::from(u16::from_le_bytes([edit[0], edit[1]])) % image.len();
        image[at] = edit[2];
    }
    let path = std::env::temp_dir().join(format!("loot-rs-fuzz-exe-{}.dll", std::process::id()));
    if std::fs::write(&path, &image).is_err() {
        return;
    }
    for name in ["version", "product_version"] {
        let _ = preflight(&path, name);
        let args = [
            Arg::Str("x.dll".into()),
            Arg::Str("1.0".into()),
            Arg::Word(">=".into()),
        ];
        let _ = evaluate(name, &args, Some(&path));
    }
});
