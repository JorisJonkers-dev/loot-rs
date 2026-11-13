#![no_main]

use libfuzzer_sys::fuzz_target;
use loot_rs::condition::{eval_str, ConditionError, Env};

/// An environment with a fixed, tiny world: enough for every function to take both branches.
struct Fixed;

impl Env for Fixed {
    fn count_files(&self, path: &str) -> Result<usize, ConditionError> {
        Ok(path.len() % 3)
    }
    fn count_active(&self, path: &str) -> Result<usize, ConditionError> {
        Ok(path.len() % 2)
    }
    fn file_size(&self, path: &str) -> Option<u64> {
        (!path.is_empty()).then_some(path.len() as u64)
    }
    fn readable(&self, path: &str) -> bool {
        path.len() % 2 == 0
    }
    fn is_master(&self, path: &str) -> bool {
        path.ends_with("esm")
    }
    fn checksum(&self, path: &str) -> Option<u32> {
        (!path.is_empty()).then_some(0xDEAD_BEEF)
    }
}

// Parsing and evaluating arbitrary text must return a value or an error, never panic or overflow the stack.
fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = eval_str(text, &Fixed);
    }
});
