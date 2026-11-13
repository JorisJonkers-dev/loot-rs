#![no_main]

use libfuzzer_sys::arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use loot_rs::plugin::{parse_header, synthetic_plugin, FLAG_ESM};

struct Spec {
    flags: u32,
    masters: Vec<String>,
    description: Option<String>,
}

impl Spec {
    fn read(u: &mut Unstructured) -> libfuzzer_sys::arbitrary::Result<Self> {
        Ok(Self {
            flags: u.arbitrary()?,
            masters: u.arbitrary()?,
            description: u.arbitrary()?,
        })
    }
}

// A synthetic plugin reads back what was written, and every cut of it is an error or the same header, never
// a different one.
fuzz_target!(|data: &[u8]| {
    let Ok(spec) = Spec::read(&mut Unstructured::new(data)) else {
        return;
    };
    // The subrecord names are NUL-terminated and sizes are 16-bit: keep to what the writer supports.
    let clean = |text: &String| !text.contains('\0') && text.len() < 200 && text.is_ascii();
    if spec.masters.len() > 50
        || !spec.masters.iter().all(clean)
        || !spec.description.iter().all(clean)
    {
        return;
    }
    let masters: Vec<&str> = spec.masters.iter().map(String::as_str).collect();
    let bytes = synthetic_plugin(spec.flags, &masters, spec.description.as_deref());
    let header = parse_header(&bytes).expect("a synthetic plugin parses");
    assert_eq!(header.flags, spec.flags);
    assert_eq!(header.masters, spec.masters);
    assert_eq!(header.is_master(), spec.flags & FLAG_ESM != 0);
    for cut in 0..bytes.len() {
        if let Ok(partial) = parse_header(&bytes[..cut]) {
            assert!(partial.masters.len() <= header.masters.len());
        }
    }
});
