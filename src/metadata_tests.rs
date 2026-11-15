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
        detail: [{lang: en, text: 'Clean it.'}]
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
    assert_eq!(entry.dirty[0].detail.as_deref(), Some("Clean it."));
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

#[test]
fn message_kinds_have_their_keywords() {
    assert_eq!(MessageKind::Say.as_str(), "say");
    assert_eq!(MessageKind::Warn.as_str(), "warn");
    assert_eq!(MessageKind::Error.as_str(), "error");
}

#[test]
fn file_labels_prefer_the_display_text() {
    let metadata = sample();
    let required = &metadata.plugins[0].req[0];
    assert_eq!(required.label(), "[D](http://x)");
    assert_eq!(required.name, "D.esp");
    assert_eq!(metadata.plugins[0].inc[0].label(), "E.esp");
}

#[test]
fn regex_detection_and_compilation() {
    for text in ["a:b", "a\\.esp", "a*", "a?", "a|b"] {
        assert!(looks_like_regex(text), "{text}");
    }
    assert!(!looks_like_regex("Plain Name (1).esp"));
    let regex = compile_whole_name("Cat.*\\.esp").expect("compiles");
    assert!(regex.is_match("CATS.ESP"));
    assert!(!regex.is_match("Cats.espx"));
    assert!(compile_whole_name("(?=x)").is_err());
    assert!(compile_whole_name("(").is_err());
}

#[test]
fn name_patterns_expose_their_text_and_match_exact_names_case_insensitively() {
    let exact = NamePattern::Exact("A.esp".into());
    assert!(exact.matches("a.ESP"));
    assert!(!exact.matches("A.esp.bak"));
    assert_eq!(exact.text(), "A.esp");
    let regex = NamePattern::Regex(compile_whole_name("A.*").expect("compiles"));
    assert_eq!(regex.text(), "^(?:A.*)$");
}

#[test]
fn scalars_of_any_type_are_text() {
    let metadata = parse(
        "globals:\n  - type: say\n    content: 42\n  - type: say\n    content: true\n  - type: say\n    content: 1.5\n",
    )
    .expect("parses");
    let texts: Vec<&str> = metadata
        .globals
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(texts, ["42", "true", "1.5"]);
}

#[test]
fn content_without_english_falls_back_to_the_first_translation() {
    let text = "globals:\n  - type: say\n    content:\n      - lang: fr\n        text: Bonjour\n      - lang: de\n        text: Hallo\n  - type: say\n    content:\n      - lang: en_GB\n        text: Hello\n";
    let metadata = parse(text).expect("parses");
    assert_eq!(metadata.globals[0].content, "Bonjour");
    assert_eq!(metadata.globals[1].content, "Hello");
}

#[test]
fn a_message_without_content_or_with_empty_content_list_is_an_error() {
    assert!(parse("globals:\n  - type: say\n").is_err());
    assert!(parse("globals:\n  - type: say\n    content: []\n").is_err());
    assert!(parse("globals:\n  - content: x\n").is_err());
}

#[test]
fn substitutions_replace_numbered_placeholders_and_ignore_missing_ones() {
    let text =
        "globals:\n  - type: say\n    content: '{1} before {0}, {2} stays'\n    subs: [one, 2]\n";
    let metadata = parse(text).expect("parses");
    assert_eq!(metadata.globals[0].content, "2 before one, {2} stays");
}

#[test]
fn entries_without_a_required_name_are_errors() {
    assert!(parse("plugins:\n  - req: [{display: x}]\n    name: A.esp\n").is_err());
    assert!(parse("plugins:\n  - name: A.esp\n    tag: [{condition: 'file(\"x\")'}]\n").is_err());
    assert!(parse("groups:\n  - description: nameless\n").is_err());
}

#[test]
fn tags_files_and_cleaning_data_are_read() {
    let text = r#"
plugins:
  - name: A.esp
    req:
      - name: B.esp
        condition: 'file("c")'
        constraint: 'active("d")'
        detail:
          - lang: en
            text: 'details'
    tag: ['Relev', '-Delev']
    dirty:
      - crc: 0xDEADBEEF
        util: 'FNVEdit'
        itm: 2
        udr: 3
        nav: 4
      - crc: -5
        itm: -1
    clean:
      - crc: 7
"#;
    let metadata = parse(text).expect("parses");
    let entry = &metadata.plugins[0];
    assert_eq!(entry.req[0].condition.as_deref(), Some("file(\"c\")"));
    assert_eq!(entry.req[0].constraint.as_deref(), Some("active(\"d\")"));
    assert_eq!(entry.req[0].detail.as_deref(), Some("details"));
    assert_eq!(entry.tag[1].name, "-Delev");
    let dirty = &entry.dirty[0];
    assert_eq!(
        (dirty.crc, dirty.itm, dirty.udr, dirty.nav),
        (Some(0xDEAD_BEEF), 2, 3, 4)
    );
    assert_eq!(dirty.util, "FNVEdit");
    assert_eq!(entry.dirty[1].crc, None, "a negative CRC is dropped");
    assert_eq!(entry.dirty[1].itm, 0, "a negative count is dropped");
    assert_eq!(entry.clean[0].crc, Some(7));
}

#[test]
fn bash_tags_and_unknown_keys() {
    let metadata =
        parse("bash_tags: [Relev, Delev]\nurl: x\nplugins:\n  - name: A.esp\n    url: [x]\n    unknown: 1\n")
            .expect("parses");
    assert_eq!(metadata.bash_tags, ["Relev", "Delev"]);
    assert_eq!(metadata.plugins.len(), 1);
}

#[test]
fn only_the_first_document_is_read() {
    let metadata =
        parse("plugins:\n  - name: A.esp\n---\nplugins:\n  - name: B.esp\n").expect("parses");
    assert_eq!(metadata.plugins.len(), 1);
}

#[test]
fn group_after_lists_are_a_union_and_the_default_group_comes_first() {
    let metadata = parse(
        "groups:\n  - name: G\n    after: [a]\n  - name: G\n    after: [a, b]\n  - name: default\n    description: 'the default'\n",
    )
    .expect("parses");
    assert_eq!(metadata.groups[0].name, "default");
    assert_eq!(metadata.groups[0].description, "the default");
    let group = metadata.groups.iter().find(|g| g.name == "G").expect("G");
    assert_eq!(group.after, ["a", "b"]);
}

#[test]
fn merge_keys_work_in_nested_maps_and_with_chained_merges() {
    let text = r#"
prelude:
  - &inner
    condition: 'file("i")'
  - &outer
    <<: *inner
    type: say
globals:
  - <<: *outer
    content: chained
"#;
    let metadata = parse(text).expect("parses");
    assert_eq!(
        metadata.globals[0].condition.as_deref(),
        Some("file(\"i\")")
    );
    assert_eq!(metadata.globals[0].kind, MessageKind::Say);
}

#[test]
fn a_merge_of_something_that_is_not_a_map_is_ignored() {
    let text = "globals:\n  - <<: 5\n    type: say\n    content: ok\n  - <<: [1, 2]\n    type: say\n    content: ok too\n";
    assert_eq!(parse(text).expect("parses").globals.len(), 2);
}

#[test]
fn deeply_nested_yaml_is_an_error_or_a_value_never_a_crash() {
    let hostile = format!("a: {}{}", "[".repeat(10_000), "]".repeat(10_000));
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || parse(&hostile).is_ok())
        .expect("spawn");
    handle.join().expect("no panic and no stack overflow");
}

fn laughs(levels: usize) -> String {
    use std::fmt::Write;
    let mut text =
        String::from("prelude:\n  - &a0 [lol, lol, lol, lol, lol, lol, lol, lol, lol, lol]\n");
    for level in 1..=levels {
        let previous = format!("*a{}", level - 1);
        let _ = writeln!(text, "  - &a{level} [{}]", vec![previous; 10].join(", "));
    }
    text + "globals: []\n"
}

#[test]
fn aliases_that_multiply_on_every_level_are_refused_before_they_are_expanded() {
    let started = std::time::Instant::now();
    let error = parse(&laughs(12)).expect_err("a billion laughs document");
    assert!(error.0.contains("expand to more than"), "{error}");
    assert!(started.elapsed().as_secs() < 5, "refused quickly");
}

#[test]
fn moderate_alias_use_is_accepted() {
    assert!(parse(&laughs(4)).is_ok());
}

#[test]
fn the_expansion_check_reports_yaml_errors_like_the_loader() {
    assert!(
        parse("a: [unclosed")
            .expect_err("fails")
            .0
            .starts_with("YAML: ")
    );
}

#[test]
fn a_masterlist_over_the_size_limit_is_refused_without_being_read() {
    let big = "#".repeat(MAX_MASTERLIST_BYTES + 1);
    assert!(
        parse(&big)
            .expect_err("refused")
            .0
            .contains("more than the limit")
    );
}

#[test]
fn brackets_nested_beyond_the_limit_are_refused() {
    let ok = format!(
        "a: {}{}\n",
        "[".repeat(MAX_FLOW_DEPTH),
        "]".repeat(MAX_FLOW_DEPTH)
    );
    assert!(parse(&ok).is_ok());
    let deep = format!("a: {}", "[".repeat(MAX_FLOW_DEPTH + 1));
    assert!(
        parse(&deep)
            .expect_err("refused")
            .0
            .contains("nested more than")
    );
}

#[test]
fn the_expanded_node_count_counts_every_alias_use() {
    assert_eq!(expanded_node_count("a: [x, y]\n").expect("counts"), 5);
    let aliased = "p: &l [x, y]\nq: [*l, *l]\n";
    // root map, p's list and two scalars, q's list and two aliases of three nodes each... plus the keys.
    let plain = expanded_node_count("p: [x, y]\nq: [[x, y], [x, y]]\n").expect("counts");
    assert_eq!(expanded_node_count(aliased).expect("counts"), plain);
}

#[test]
fn the_merge_key_budget_stops_the_expansion_before_it_clones_more() {
    let yaml = YamlLoader::load_from_str("a: &x {k: v}\nb: {<<: [*x, *x]}\n").expect("loads");
    let mut budget = Budget { left: 3 };
    let error = expand_merge_keys(&yaml[0], &mut budget, 0).expect_err("budget runs out");
    assert!(
        error.0.contains("merge keys expand to more than"),
        "{error}"
    );
    let mut enough = Budget { left: 1000 };
    assert!(expand_merge_keys(&yaml[0], &mut enough, 0).is_ok());
}

#[test]
fn the_merge_key_expansion_refuses_to_recurse_past_the_depth_limit() {
    let mut node = Yaml::String("leaf".into());
    for _ in 0..(MAX_COUNTED_DEPTH + 5) {
        node = Yaml::Array(vec![node]);
    }
    let mut budget = Budget { left: 1_000_000 };
    let error = expand_merge_keys(&node, &mut budget, 0).expect_err("too deep");
    assert!(error.0.contains("nested too deeply"));
    // Dropping the 260-deep tree is fine; a deeper one never gets built by the loader.
}

#[test]
fn the_entries_for_a_name_are_the_entries_whose_pattern_matches_it_in_file_order() {
    let metadata = parse(
        "plugins:\n  - name: 'A.esp'\n    group: g\n  - name: 'A.*\\.esp'\n  - name: 'a.ESP'\n  - name: 'B.esp'\n",
    )
    .expect("parses");
    for name in ["A.esp", "a.esp", "AB.esp", "B.esp", "C.esp"] {
        let expected: Vec<&PluginEntry> = metadata
            .plugins
            .iter()
            .filter(|entry| entry.pattern.matches(name))
            .collect();
        let found = metadata.entries_for(name);
        assert_eq!(found.len(), expected.len(), "{name}");
        assert!(
            found
                .iter()
                .zip(&expected)
                .all(|(x, y)| std::ptr::eq(*x, *y)),
            "{name}"
        );
    }
}

// libloot's tests of `replace_prelude` (src/metadata/metadata_document.rs), translated.

const NOTE_PRELUDE: &str = "globals:\n  - type: note\n    content: A message.\n";
const REPLACED: &str = "prelude:\n  globals:\n    - type: note\n      content: A message.\n\nplugins:\n  - name: a.esp\n";

#[test]
fn replacing_the_prelude_of_empty_text_gives_empty_text() {
    assert_eq!(replace_prelude("", ""), "");
}

#[test]
fn a_masterlist_without_a_prelude_is_not_changed() {
    let masterlist = "plugins:\n  - name: a.esp\n";
    assert_eq!(replace_prelude(masterlist, NOTE_PRELUDE), masterlist);
}

#[test]
fn a_flow_style_masterlist_is_not_changed() {
    let masterlist = "{prelude: {}, plugins: [{name: a.esp}]}";
    assert_eq!(
        replace_prelude(masterlist, "globals: [{type: note, content: A message.}]"),
        masterlist
    );
}

#[test]
fn a_prelude_at_the_start_of_the_masterlist_is_replaced() {
    let masterlist = "prelude:\n  a: b\n\nplugins:\n  - name: a.esp\n";
    assert_eq!(replace_prelude(masterlist, NOTE_PRELUDE), REPLACED);
}

#[test]
fn a_prelude_followed_directly_by_the_next_key_is_replaced() {
    let masterlist = "prelude:\n  a: b\nplugins:\n  - name: a.esp\n";
    assert_eq!(replace_prelude(masterlist, NOTE_PRELUDE), REPLACED);
}

#[test]
fn a_new_prelude_without_a_final_line_break_is_joined_to_the_next_key() {
    let prelude = "globals:\n  - type: note\n    content: A message.";
    let expected = "prelude:\n  globals:\n    - type: note\n      content: A message.\nplugins:\n  - name: a.esp\n";
    for masterlist in [
        "prelude:\n  a: b\n\nplugins:\n  - name: a.esp\n",
        "prelude:\n  a: b\nplugins:\n  - name: a.esp\n",
    ] {
        assert_eq!(replace_prelude(masterlist, prelude), expected);
    }
}

#[test]
fn a_prelude_at_the_end_of_the_masterlist_is_replaced() {
    let masterlist = "plugins:\n  - name: a.esp\nprelude:\n  a: b\n\n";
    let expected = "plugins:\n  - name: a.esp\nprelude:\n  globals:\n    - type: note\n      content: A message.\n";
    assert_eq!(replace_prelude(masterlist, NOTE_PRELUDE), expected);
}

#[test]
fn only_the_prelude_section_is_replaced() {
    let prelude = "\n\nglobals:\n  - type: note\n    content: A message.\n\n";
    let masterlist = "\ncommon:\n  key: value\nprelude:\n  a: b\nplugins:\n  - name: a.esp\n";
    let expected = "\ncommon:\n  key: value\nprelude:\n\n\n  globals:\n    - type: note\n      content: A message.\n\n\nplugins:\n  - name: a.esp\n";
    assert_eq!(replace_prelude(masterlist, prelude), expected);
}

#[test]
fn a_flow_style_prelude_replaces_a_block_style_one() {
    let masterlist = "prelude:\n  a: b\n\nplugins:\n  - name: a.esp\n";
    let expected =
        "prelude:\n  globals: [{type: note, content: A message.}]\nplugins:\n  - name: a.esp\n";
    assert_eq!(
        replace_prelude(masterlist, "globals: [{type: note, content: A message.}]"),
        expected
    );
}

#[test]
fn comments_and_blank_lines_do_not_end_the_prelude() {
    for masterlist in [
        "prelude:\n  a: b\n# Comment line\n  c: d\n\nplugins:\n  - name: a.esp\n",
        "prelude:\n  a: b\n\n\nplugins:\n  - name: a.esp\n",
    ] {
        assert_eq!(replace_prelude(masterlist, NOTE_PRELUDE), REPLACED);
    }
}
