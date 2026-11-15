//! SHA-1 (FIPS 180-4), only to name a masterlist the way LOOT does: the git blob id `sha1("blob <length>\0" +
//! bytes)` that LOOT stores as `blob_sha1` in `masterlist.yaml.metadata.toml`. Not used for anything that needs
//! collision resistance.

/// The SHA-1 of `bytes` as 40 lower-case hex digits.
#[must_use]
pub fn sha1_hex(bytes: &[u8]) -> String {
    hex(&sha1(&[bytes]))
}

/// The git blob id of `bytes`: what `git hash-object` prints for a file with these contents.
#[must_use]
pub fn git_blob_sha1(bytes: &[u8]) -> String {
    let header = format!("blob {}\0", bytes.len());
    hex(&sha1(&[header.as_bytes(), bytes]))
}

fn hex(digest: &[u8; 20]) -> String {
    use std::fmt::Write as _;
    digest
        .iter()
        .fold(String::with_capacity(40), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// SHA-1 of the concatenation of `parts`.
fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let length: u64 = parts.iter().map(|part| part.len() as u64).sum();
    let mut block = [0u8; 64];
    let mut filled = 0;
    let feed = |bytes: &[u8], state: &mut [u32; 5], block: &mut [u8; 64], filled: &mut usize| {
        for &byte in bytes {
            block[*filled] = byte;
            *filled += 1;
            if *filled == 64 {
                compress(state, block);
                *filled = 0;
            }
        }
    };
    for part in parts {
        feed(part, &mut state, &mut block, &mut filled);
    }
    let mut padding = vec![0x80u8];
    let after = (filled + 1) % 64;
    let zeros = if after <= 56 { 56 - after } else { 120 - after };
    padding.resize(1 + zeros, 0);
    padding.extend_from_slice(&length.wrapping_mul(8).to_be_bytes());
    feed(&padding, &mut state, &mut block, &mut filled);
    let mut digest = [0u8; 20];
    for (chunk, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *chunk = word.to_be_bytes();
    }
    digest
}

// The names are the standard's (FIPS 180-4, section 6.1.2), so the code can be read next to it.
#[allow(clippy::many_single_char_names)]
fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for (slot, chunk) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *slot = u32::from_be_bytes(*chunk);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, &word) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let next = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(word);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = next;
    }
    for (slot, value) in state.iter_mut().zip([a, b, c, d, e]) {
        *slot = slot.wrapping_add(value);
    }
}

#[cfg(test)]
#[path = "sha1_tests.rs"]
mod tests;
