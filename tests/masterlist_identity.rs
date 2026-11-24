//! A masterlist's identity (its git blob id and LOOT's `.metadata.toml`) and the replacement of its prelude.

mod common;

use common::{covers, ids};

use common::Fixture;
use loot_rs::api::{Masterlist, MasterlistError};

/// A masterlist whose tag list is an alias that only the new prelude defines.
const MASTERLIST: &str =
    "prelude:\n  common: &old\n    - Delev\nplugins:\n  - name: A.esp\n    tag: *new\n";
const PRELUDE: &str = "common: &new\n  - Relev\n";

#[test]
fn the_digest_of_a_masterlist_file_is_what_git_hash_object_prints() {
    covers(&[ids::metadata::file::IDENTITY]);
    let fixture = Fixture::new();
    let path = fixture.masterlist(MASTERLIST.replace("*new", "*old").as_str());
    let masterlist = Masterlist::from_file(&path).expect("reads");
    // `git hash-object` of these bytes, computed with git; a plain SHA-1 of them is different.
    assert_eq!(
        masterlist.digest(),
        "4afe0338fecbc4351d02ccf3da137599ce380701"
    );
    if let Ok(output) = std::process::Command::new("git")
        .arg("hash-object")
        .arg(&path)
        .output()
        && output.status.success()
    {
        assert_eq!(
            masterlist.digest(),
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
}

#[test]
fn a_masterlist_whose_aliases_resolve_only_through_the_prelude_needs_the_prelude() {
    let fixture = Fixture::new();
    let path = fixture.masterlist(MASTERLIST);
    let prelude = fixture.root.join("prelude.yaml");
    std::fs::write(&prelude, PRELUDE).expect("write prelude");
    assert!(matches!(
        Masterlist::from_file(&path),
        Err(MasterlistError::Invalid(_))
    ));
    let masterlist =
        Masterlist::from_file_with_prelude(&path, &prelude).expect("parses with the prelude");
    assert_eq!(
        masterlist.prelude_digest(),
        Some(loot_rs::sha1::git_blob_sha1(PRELUDE.as_bytes()).as_str())
    );
}

#[test]
fn the_digest_is_of_the_masterlist_as_read_not_of_the_text_after_the_prelude_was_replaced() {
    let with = Masterlist::parse_with_prelude(MASTERLIST, PRELUDE).expect("parses");
    let alone = Masterlist::parse(&MASTERLIST.replace("*new", "*old")).expect("parses");
    assert_ne!(with.digest(), alone.digest());
    assert_eq!(
        with.digest(),
        loot_rs::sha1::git_blob_sha1(MASTERLIST.as_bytes())
    );
}

#[test]
fn the_metadata_file_next_to_the_masterlist_gives_its_source() {
    let fixture = Fixture::new();
    let text = "plugins: []\n";
    let path = fixture.masterlist(text);
    let digest = loot_rs::sha1::git_blob_sha1(text.as_bytes());
    std::fs::write(
        fixture.root.join("masterlist.yaml.metadata.toml"),
        format!("blob_sha1 = \"{digest}\"\nupdate_timestamp = \"2026-02-10\"\n"),
    )
    .expect("write metadata");
    let masterlist = Masterlist::from_file(&path).expect("reads");
    let source = masterlist.source().expect("has a source");
    assert_eq!(source.blob_sha1, digest);
    assert_eq!(source.update_timestamp.as_deref(), Some("2026-02-10"));
    assert_eq!(masterlist.matches_source(), Some(true));
}

#[test]
fn a_masterlist_without_a_metadata_file_has_no_source() {
    let fixture = Fixture::new();
    let masterlist = Masterlist::from_file(&fixture.masterlist("plugins: []\n")).expect("reads");
    assert_eq!(masterlist.source(), None);
    assert_eq!(masterlist.matches_source(), None);
}
