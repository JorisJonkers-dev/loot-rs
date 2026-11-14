//! Unit tests of `version`: the extraction cases are libloot's own (`plugin/mod.rs`), the comparison cases are
//! those of the condition interpreter's `Version`.

use super::*;

fn extracted(text: &str) -> Option<String> {
    extract_version(text)
}

#[test]
fn a_version_of_one_digit_or_several_digits() {
    assert_eq!(extracted("5").as_deref(), Some("5"));
    assert_eq!(extracted("10").as_deref(), Some("10"));
}

#[test]
fn a_version_of_several_numbers() {
    assert_eq!(extracted("10.11.12.13").as_deref(), Some("10.11.12.13"));
}

#[test]
fn a_semantic_version_stops_before_the_build_metadata() {
    assert_eq!(
        extracted("1.0.0-x.7.z.92+exp.sha.5114f85").as_deref(),
        Some("1.0.0-x.7.z.92")
    );
}

#[test]
fn a_pseudosem_version_stops_at_the_first_space() {
    assert_eq!(
        extracted("01.0.0_alpha:1-2 3").as_deref(),
        Some("01.0.0_alpha:1-2")
    );
}

#[test]
fn a_version_with_a_leading_v() {
    assert_eq!(extracted("v5.0").as_deref(), Some("5.0"));
    assert_eq!(
        extracted("V2.11\r\n\r\n{{BASH:Invent}}").as_deref(),
        Some("2.11")
    );
}

#[test]
fn text_without_a_version_gives_none() {
    assert_eq!(
        extracted("The quick brown fox jumped over the lazy dog."),
        None
    );
}

#[test]
fn a_timestamp_with_slashes_is_a_version() {
    assert_eq!(
        extracted("Updated: 10/09/2016 13:15:18\r\n\r\nRecords Changed: 43").as_deref(),
        Some("10/09/2016 13:15:18")
    );
}

#[test]
fn a_trailing_period_is_not_part_of_the_version() {
    assert_eq!(extracted("Version 0.2.").as_deref(), Some("0.2"));
}

#[test]
fn a_version_after_text_and_a_version_colon() {
    assert_eq!(
        extracted("Legendary Edition\r\n\r\nVersion: 3.0.0").as_deref(),
        Some("3.0.0")
    );
}

#[test]
fn a_number_with_a_comma_is_ignored() {
    assert_eq!(
        extracted("fixing over 2,300 bugs so far! Version: 3.5.3").as_deref(),
        Some("3.5.3")
    );
}

#[test]
fn a_version_before_text() {
    assert_eq!(
        extracted("Version: 2.1 The Unofficial Fallout 3 Patch").as_deref(),
        Some("2.1")
    );
}

#[test]
fn a_version_after_colon_period_and_space() {
    assert_eq!(extracted("Version:. 1.09").as_deref(), Some("1.09"));
}

#[test]
fn letters_straight_after_the_numbers_belong_to_the_version() {
    assert_eq!(
        extracted("comprehensive bugfixing mod\r\n\r\nVersion: 2.1.3b\r\n\r\n").as_deref(),
        Some("2.1.3b")
    );
}

#[test]
fn a_version_with_a_period_and_no_identifier() {
    assert_eq!(extracted("SkyUI 5.1").as_deref(), Some("5.1"));
}

#[test]
fn a_single_digit_in_a_sentence_is_not_a_version() {
    assert_eq!(
        extracted("Adds 8 variants of Triss Merigold's outfit from \"The Witcher 2\""),
        None
    );
}

#[test]
fn a_version_prefixed_number_beats_a_version_in_a_sentence() {
    assert_eq!(
        extracted(
            "Requires Skyrim patch 1.9.32.0.8 or greater.\nRequires Unofficial Skyrim Legendary Edition Patch 3.0.0 or greater.\nVersion 2.0.0"
        )
        .as_deref(),
        Some("2.0.0")
    );
}

fn holds(actual: &str, operator: &str, given: &str) -> bool {
    compare(actual, operator, given).expect("an operator")
}

#[test]
fn versions_compare_number_by_number_with_missing_ids_as_zero() {
    assert!(holds("3.2.0", ">=", "3.2.0"));
    assert!(holds("3.10", ">", "3.9"));
    assert!(holds("3.2", "==", "3.2.0"));
    assert!(holds("1.0", "<", "1.0.1"));
    assert!(holds("3.2.1", "!=", "3.2"));
}

#[test]
fn an_empty_version_is_zero() {
    assert!(holds("", "==", "0"));
    assert!(holds("", "<", "1"));
}

#[test]
fn a_pre_release_is_less_than_its_release() {
    assert!(holds("1.0-alpha", "<", "1.0"));
    assert!(holds("1.0-alpha", "<", "1.0-beta"));
    assert!(holds("1.0-alpha.2", ">", "1.0-alpha.1"));
}

#[test]
fn build_metadata_is_ignored() {
    assert!(holds("1.0+abc", "==", "1.0"));
}

#[test]
fn a_release_id_with_letters_is_greater_than_the_bare_number() {
    assert!(holds("1.0b", ">", "1.0"));
    assert!(holds("1.0a", "<", "1.1"));
}

#[test]
fn comma_separated_ids_are_read_as_dots() {
    assert!(holds("0, 1, 2, 3", "==", "0.1.2.3"));
}

#[test]
fn an_unknown_operator_is_none() {
    assert_eq!(compare("1", "=~", "1"), None);
}
