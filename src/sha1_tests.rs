use super::*;

#[test]
fn sha1_matches_the_standard_test_vectors() {
    assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(
        sha1_hex(&vec![b'a'; 1_000_000]),
        "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
    );
}

#[test]
fn a_blob_id_is_what_git_hash_object_prints() {
    // `printf 'hello\n' | git hash-object --stdin` and `git hash-object` of an empty file.
    assert_eq!(
        git_blob_sha1(b"hello\n"),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
    assert_eq!(
        git_blob_sha1(b""),
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    );
}

#[test]
fn every_length_around_a_block_boundary_hashes_like_one_feed() {
    // Lengths 55 to 65 cross the padding boundary; the one-part and two-part forms must agree.
    for length in 50..70 {
        let bytes: Vec<u8> = (0..length)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        let (left, right) = bytes.split_at(length / 2);
        assert_eq!(sha1(&[&bytes]), sha1(&[left, right]), "{length}");
    }
}
