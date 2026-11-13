//! Unit tests of `metadata`.

use super::*;

const SAMPLE: &str = r#"
prelude:
  - &thanks
    type: say
    content: 'Thanks'
bash_tags:
  - Relev
globals:
  - *thanks
  - type: warn
    content:
      - lang: de
        text: 'Hallo'
      - lang: en
        text: 'Use {0} {1}'
    subs: ['this', 'mod']
    condition: 'file("a.esp")'
groups:
  - name: 'Late'
    after: ['default']
  - name: 'Late'
    description: 'later'
    after: ['default', 'Mid']
plugins:
  - name: 'A.esp'
    group: 'Late'
    after: ['B.esp', {name: 'C.esp', condition: 'file("c")'}]
    req:
      - name: 'D.esp'
        display: '[D](http://x)'
        detail: 'get it'
    inc: ['E.esp']
    tag: ['Relev', {name: '-Delev', condition: 'file("f")'}]
    dirty:
      - crc: 0x1234ABCD
        util: 'FNVEdit'
        itm: 3
    clean:
      - crc: 0x1
        util: 'FNVEdit'
  - name: 'Bad(?=x)\.esp'
  - name: 'Cat.*\.esp'
    msg:
      - type: say
        content: 'regex entry'
"#;

fn sample() -> Metadata {
    parse(SAMPLE).expect("parses")
}

#[test]
fn aliases_in_globals_are_resolved() {
    let metadata = sample();
    assert_eq!(metadata.globals.len(), 2);
    assert_eq!(metadata.globals[0].content, "Thanks");
    assert_eq!(metadata.globals[0].kind, MessageKind::Say);
}

#[test]
fn english_content_is_chosen_and_subs_are_applied() {
    let message = &sample().globals[1];
    assert_eq!(message.content, "Use this mod");
    assert_eq!(message.condition.as_deref(), Some("file(\"a.esp\")"));
}

#[test]
fn groups_merge_by_name_and_default_exists() {
    let metadata = sample();
    assert_eq!(metadata.groups[0].name, DEFAULT_GROUP);
    let late = metadata.groups.iter().find(|group| group.name == "Late");
    let late = late.expect("Late group");
    assert_eq!(late.description, "later");
    assert_eq!(late.after, ["default", "Mid"]);
}

#[test]
fn plugin_entry_fields_are_read() {
    let metadata = sample();
    let entry = &metadata.plugins[0];
    assert!(entry.pattern.matches("a.ESP"));
    assert_eq!(entry.group.as_deref(), Some("Late"));
    assert_eq!(entry.after.len(), 2);
    assert_eq!(entry.after[1].condition.as_deref(), Some("file(\"c\")"));
    assert_eq!(entry.req[0].label(), "[D](http://x)");
    assert_eq!(entry.req[0].detail.as_deref(), Some("get it"));
    assert_eq!(entry.inc[0].name, "E.esp");
    assert_eq!(entry.tag[1].name, "-Delev");
    assert_eq!(entry.dirty[0].crc, Some(0x1234_ABCD));
    assert_eq!(entry.dirty[0].itm, 3);
    assert_eq!(entry.clean[0].crc, Some(1));
}

#[test]
fn regex_names_match_whole_names_case_insensitively() {
    let metadata = sample();
    let entry = metadata.plugins.last().expect("regex entry");
    assert!(entry.pattern.matches("CATS.esp"));
    assert!(!entry.pattern.matches("xCats.esp"));
    assert!(!entry.pattern.matches("Cats.esp.bak"));
}

#[test]
fn a_regex_this_engine_cannot_compile_is_skipped_with_a_warning() {
    let metadata = sample();
    assert_eq!(metadata.plugins.len(), 2);
    assert_eq!(metadata.warnings.len(), 1);
    assert!(metadata.warnings[0].contains("Bad(?=x)"));
}

#[test]
fn merge_keys_are_expanded_and_explicit_keys_win() {
    let text = r#"
prelude:
  - &base
    type: warn
    content: 'Use {0}.'
    condition: 'file("x")'
  - &second
    type: error
    content: 'second'
    subs: ['never']
globals:
  - <<: *base
    subs: ['this']
  - <<: [*second, *base]
    content: 'own'
"#;
    let metadata = parse(text).expect("parses");
    assert_eq!(metadata.globals[0].content, "Use this.");
    assert_eq!(metadata.globals[0].kind, MessageKind::Warn);
    assert_eq!(
        metadata.globals[0].condition.as_deref(),
        Some("file(\"x\")")
    );
    assert_eq!(metadata.globals[1].content, "own");
    assert_eq!(metadata.globals[1].kind, MessageKind::Error);
}

#[test]
fn bad_message_type_is_an_error() {
    let error = parse("globals:\n  - type: shout\n    content: x\n").expect_err("fails");
    assert!(error.0.contains("say, warn or error"));
}

#[test]
fn entry_without_name_is_an_error() {
    assert!(parse("plugins:\n  - group: x\n").is_err());
}

#[test]
fn empty_and_non_map_files() {
    assert!(parse("").expect("empty is fine").plugins.is_empty());
    assert!(parse("- a\n- b\n").is_err());
    assert!(parse("a: [unclosed").is_err());
}
