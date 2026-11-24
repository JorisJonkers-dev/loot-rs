# Feature coverage

Generated on main by tally from `catalogue/` (docs/TESTING.md, "Feature catalogue"). Do not edit.

| | Implemented | Tagged | Passing | Covered | Missing | Gated | n/a, deferred |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Total** | 76/78 | 76 | 0 | 0 | 2 | 0 | 6 |
| **Condition language** | 17/17 | 17 | 0 | 0 | 0 | 0 | 0 |
| Condition language (`conditions.language`) | 17/17 | 17 | 0 | 0 | 0 | 0 | 0 |
| **Groups** | 8/8 | 8 | 0 | 0 | 0 | 0 | 0 |
| Groups (`groups.behaviour`) | 8/8 | 8 | 0 | 0 | 0 | 0 | 0 |
| **Load order input** | 5/7 | 5 | 0 | 0 | 2 | 0 | 0 |
| Load order input (`load_order.input`) | 5/7 | 5 | 0 | 0 | 2 | 0 | 0 |
| **Metadata file** | 22/22 | 22 | 0 | 0 | 0 | 0 | 0 |
| Metadata file (`metadata.file`) | 10/10 | 10 | 0 | 0 | 0 | 0 | 0 |
| Plugin entry (`metadata.plugin`) | 12/12 | 12 | 0 | 0 | 0 | 0 | 0 |
| **Messages and output** | 3/3 | 3 | 0 | 0 | 0 | 0 | 1 |
| Messages and output (`output.messages`) | 3/3 | 3 | 0 | 0 | 0 | 0 | 1 |
| **Sorting** | 15/15 | 15 | 0 | 0 | 0 | 0 | 5 |
| Messages of the LOOT application that are not ported (`sorting.not_ported`) | 1/1 | 1 | 0 | 0 | 0 | 0 | 4 |
| Sorting (`sorting.stages`) | 14/14 | 14 | 0 | 0 | 0 | 0 | 1 |
| **Additions of this project** | 6/6 | 6 | 0 | 0 | 0 | 0 | 0 |
| Additions of this project (`tool.cli`) | 6/6 | 6 | 0 | 0 | 0 | 0 | 0 |

### Condition language (`conditions.language`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| `and`, `or`, `not`, parentheses, C-style precedence | `conditions.language.operators` | tagged | `src/condition_tests.rs::precedence_is_not_then_and_then_or` |
| Whitespace and line breaks around operators and arguments | `conditions.language.whitespace` | tagged | `src/condition_tests.rs::whitespace_and_line_breaks_are_allowed` |
| Regex detection (`:\*?\|` in the argument) | `conditions.language.regex_detection` | tagged | `src/env_tests.rs::count_files_handles_plain_paths_and_regexes`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `file(path)`, `file(regex)` | `conditions.language.file` | tagged | `src/condition_tests.rs::file_and_active`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `active(path)`, `active(regex)` | `conditions.language.active` | tagged | `src/condition_tests.rs::file_and_active`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `many(regex)`, `many_active(regex)` | `conditions.language.many` | tagged | `src/condition_tests.rs::many_counts_more_than_one`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `file_size(path, size)` | `conditions.language.file_size` | tagged | `src/condition_tests.rs::size_checksum_master_and_readable`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `readable(path)` | `conditions.language.readable` | tagged | `src/condition_tests.rs::size_checksum_master_and_readable`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `is_master(path)` | `conditions.language.is_master` | tagged | `src/condition_tests.rs::size_checksum_master_and_readable`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `checksum(path, crc)` | `conditions.language.checksum` | tagged | `src/condition_tests.rs::size_checksum_master_and_readable`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `version(path, op, version)` and the older argument order | `conditions.language.version` | tagged | `src/env_tests.rs::the_version_of_a_plugin_comes_from_its_description`, `src/exe_version_tests.rs::the_fixed_file_version_is_compared_in_both_argument_orders` |
| `product_version(path, op, version)` | `conditions.language.product_version` | tagged | `src/exe_version_tests.rs::the_product_version_string_is_compared_with_loots_grammar` |
| `filename_version(regex, op, version)` and the other argument order | `conditions.language.filename_version` | tagged | `src/env_tests.rs::filename_version_compares_the_captured_version_of_a_matching_file` |
| `description_contains(path, regex)` | `conditions.language.description_contains` | tagged | `src/env_tests.rs::description_contains_matches_the_plugin_description_case_insensitively` |
| `is_executable(path)` | `conditions.language.is_executable` | tagged | `src/env_tests.rs::is_executable_is_true_only_for_a_file_that_reads_as_a_pe_image` |
| Version comparison grammar | `conditions.language.version_grammar` | tagged | `src/version_tests.rs::versions_compare_number_by_number_with_missing_ids_as_zero` |
| Evaluation caching | `conditions.language.caching` | tagged | `src/env_tests.rs::directory_listings_are_cached` |

### Groups (`groups.behaviour`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| Predefined `default` group | `groups.behaviour.default_group` | tagged | `src/check_tests.rs::plugins_without_entries_are_in_the_default_group_and_listed_when_installed_only` |
| Transitive "loads after" | `groups.behaviour.transitive_after` | tagged | `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| Cyclic group relations | `groups.behaviour.cyclic` | tagged | `src/sort_tests.rs::groups_that_load_after_each_other_are_an_error`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| Edge dropped when it clashes with master, `after` or `req` edges | `groups.behaviour.edge_dropped` | tagged | `src/api_tests.rs::a_group_rule_that_a_requirement_contradicts_is_dropped_and_soft_unless_strict`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| Group named but undefined | `groups.behaviour.undefined_group` | tagged | `src/check_tests.rs::an_undefined_group_is_noted_and_hard_only_when_strict`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| Group order violation | `groups.behaviour.order_violation` | tagged | `src/check_tests.rs::a_group_that_loads_before_one_it_must_follow_is_a_soft_finding_by_default`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| Sorting | `groups.behaviour.sorting` | tagged | `src/sort_tests.rs::groups_decide_the_relative_positions` |
| Minimal-change sort (`SortMode::MinimalChange`) | `groups.behaviour.minimal_change` | tagged | `src/sort/minimal_tests.rs::one_plugin_that_loads_too_late_is_moved_alone_to_the_place_the_rule_needs` |

### Load order input (`load_order.input`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| `plugins.txt`, active-only style | `load_order.input.plugins_txt_active` | tagged | `src/env_tests.rs::plugins_txt_active_only_style` |
| `plugins.txt`, `*` style | `load_order.input.plugins_txt_star` | tagged | `src/env_tests.rs::plugins_txt_starred_style` |
| `loadorder.txt` | `load_order.input.loadorder_txt` | missing |  |
| Ghosted plugins (`.ghost`) | `load_order.input.ghosted` | tagged | `src/env_tests.rs::a_ghosted_plugin_counts_as_the_plugin` |
| Game directory, additional data directories | `load_order.input.data_dirs` | tagged | `src/env_tests.rs::additional_directories_are_searched_before_data_in_the_order_given` |
| Names differing only in case | `load_order.input.case_names` | tagged | `src/env_tests.rs::names_that_differ_only_in_case_are_a_warning` |
| Master flag / master list sort rules | `load_order.input.master_flag_rules` | missing |  |

### Metadata file (`metadata.file`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| `plugins` list | `metadata.file.plugins_list` | tagged | `src/metadata_tests.rs::plugin_entry_fields_are_read` |
| `globals` | `metadata.file.globals` | tagged | `src/metadata_tests.rs::aliases_in_globals_are_resolved` |
| `groups` | `metadata.file.groups` | tagged | `src/metadata_tests.rs::groups_merge_by_name_and_default_exists` |
| `bash_tags` | `metadata.file.bash_tags` | tagged | `src/metadata_tests.rs::bash_tags_and_unknown_keys` |
| `prelude` | `metadata.file.prelude` | tagged | `src/metadata_tests.rs::a_prelude_at_the_start_of_the_masterlist_is_replaced` |
| YAML anchors, aliases | `metadata.file.anchors_aliases` | tagged | `src/metadata_tests.rs::aliases_in_globals_are_resolved`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| YAML merge keys `<<` | `metadata.file.merge_keys` | tagged | `src/metadata_tests.rs::merge_keys_are_expanded_and_explicit_keys_win`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| Userlist, metadata merging of several files | `metadata.file.userlist` | tagged | `tests/userlist.rs::user_metadata_comes_before_the_masterlist_s_and_its_group_wins` |
| Unknown top-level keys | `metadata.file.unknown_keys` | tagged | `src/metadata_tests.rs::bash_tags_and_unknown_keys` |
| Masterlist identity (`blob_sha1`, `update_timestamp`) | `metadata.file.identity` | tagged | `tests/masterlist_identity.rs::the_digest_of_a_masterlist_file_is_what_git_hash_object_prints` |

### Plugin entry (`metadata.plugin`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| `name`, exact | `metadata.plugin.name_exact` | tagged | `src/metadata_tests.rs::name_patterns_expose_their_text_and_match_exact_names_case_insensitively` |
| `name`, regex | `metadata.plugin.name_regex` | tagged | `src/metadata_tests.rs::regex_names_match_whole_names_case_insensitively`, `tests/golden.rs::every_case_prints_what_its_golden_file_says`, `tests/regex_semantics.rs::plugin_name_expressions_match_as_libloot_matches_them` |
| `group` | `metadata.plugin.group` | tagged | `src/check_tests.rs::every_matching_entry_applies_in_file_order_and_the_first_group_wins`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `after` | `metadata.plugin.after` | tagged | `src/check_tests.rs::after_is_violated_only_when_the_other_plugin_is_installed_and_later`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `req` | `metadata.plugin.req` | tagged | `src/check_tests.rs::a_missing_requirement_is_hard_and_names_the_display_text_and_detail`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `inc` | `metadata.plugin.inc` | tagged | `src/check_tests.rs::an_incompatible_plugin_that_is_present_is_hard`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `msg` | `metadata.plugin.msg` | tagged | `src/check_tests.rs::messages_apply_when_their_condition_holds`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `tag` | `metadata.plugin.tag` | tagged | `src/check_tests.rs::tags_are_added_removed_cancelled_and_deduplicated`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `dirty`, `clean` | `metadata.plugin.dirty_clean` | tagged | `src/api_tests.rs::cleaning_data_is_matched_by_the_checksum_of_the_installed_file`, `tests/golden.rs::every_case_prints_what_its_golden_file_says`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| `url` | `metadata.plugin.url` | tagged | `src/metadata_tests.rs::url_entries_are_read_as_links_with_optional_names` |
| `display`, `detail` on file entries | `metadata.plugin.display_detail` | tagged | `src/metadata_tests.rs::file_labels_prefer_the_display_text` |
| `constraint` on file entries | `metadata.plugin.constraint` | tagged | `src/check_tests.rs::a_constraint_decides_whether_the_required_file_counts_as_present`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |

### Messages and output (`output.messages`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| Message types `say`, `warn`, `error` | `output.messages.types` | tagged | `src/metadata_tests.rs::message_kinds_have_their_keywords` |
| Localised content | `output.messages.localised` | tagged | `src/metadata_tests.rs::a_text_in_exactly_the_language_wanted_is_chosen`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `subs` substitution | `output.messages.subs` | tagged | `src/metadata_tests.rs::substitutions_replace_numbered_placeholders_and_ignore_missing_ones`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| CommonMark rendering | `output.messages.commonmark` | n/a |  |

### Messages of the LOOT application that are not ported (`sorting.not_ported`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| "You have not sorted your load order this session." (General Information) | `sorting.not_ported.not_sorted_session` | n/a |  |
| Text for a sorting cycle: `Cyclic interaction detected between "A.esp" and "B.esp": A.esp [Masterlist Load After] B.esp [Masterlist Load After] A.esp` | `sorting.not_ported.cycle_text` | n/a |  |
| A plugin that is its own master | `sorting.not_ported.self_master` | tagged | `src/validate_tests.rs::a_plugin_that_is_its_own_master_and_a_low_header_version_are_reported`, `tests/golden_app.rs::every_message_is_what_the_loot_application_showed` |
| Case-sensitive path warnings ("... is installed in a case-sensitive location ...", and the same for local application data) | `sorting.not_ported.case_sensitive_paths` | n/a |  |
| Checks of other games (light, medium, update and blueprint plugins) | `sorting.not_ported.other_games` | n/a |  |

### Sorting (`sorting.stages`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| Vertices in lexicographical (byte) order of the names | `sorting.stages.byte_order_vertices` | tagged | `src/sort/plugins_tests.rs::should_not_depend_on_plugin_graph_node_order` |
| Separate graphs for masters (master flag) and the others, joined masters first | `sorting.stages.separate_graphs` | tagged | `src/sort_tests.rs::masters_load_before_the_others_and_each_group_keeps_its_order`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| Blueprint masters (Starfield) | `sorting.stages.blueprint` | n/a |  |
| Edges from header masters, `req` and `after` (masterlist) | `sorting.stages.edges_rules` | tagged | `src/sort_tests.rs::load_after_and_requirements_decide_the_relative_positions`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| Edges from the userlist (`req`, `after`, `group`) | `sorting.stages.user_edges` | tagged | `src/sort/plugins_tests.rs::should_add_user_group_edge_if_source_plugin_is_in_group_due_to_user_metadata`, `tests/userlist.rs::a_user_after_entry_moves_a_plugin_and_is_listed_with_a_user_source` |
| Hard-coded early loader | `sorting.stages.early_loader` | tagged | `src/sort/plugins_tests.rs::should_add_edges_between_consecutive_early_loaders_skipping_missing_plugins`, `src/sort_tests.rs::an_early_loader_loads_before_everything_else_of_its_kind` |
| Cycle among these edges is an error naming the cycle | `sorting.stages.cycle_error` | tagged | `src/sort_tests.rs::a_cycle_among_the_metadata_names_the_plugins_and_edges`, `tests/golden_app.rs::every_message_is_what_the_loot_application_showed` |
| Group edges: depth-first searches of the group graph, roots first by longest path, skipped when they would close a cycle, the `default` group last | `sorting.stages.group_edges` | tagged | `src/sort/plugins_tests.rs::should_skip_an_edge_that_would_cause_a_cycle`, `src/sort_tests.rs::groups_decide_the_relative_positions`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| Cycle among groups | `sorting.stages.group_cycle` | tagged | `src/sort_tests.rs::groups_that_load_after_each_other_are_an_error` |
| Record overlap edges | `sorting.stages.record_overlap` | tagged | `src/api_tests.rs::the_plugin_that_changes_more_of_the_same_records_loads_first`, `src/sort/plugins_tests.rs::should_choose_record_overlap_over_asset_overlap`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| Asset (archive) overlap edges | `sorting.stages.asset_overlap` | tagged | `src/api_tests.rs::the_plugin_that_loads_more_of_the_same_archive_files_goes_first`, `src/sort/plugins_tests.rs::should_choose_record_overlap_over_asset_overlap`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| Tie-break edges and pinning | `sorting.stages.tie_break` | tagged | `src/sort/plugins_tests.rs::tie_break_gives_the_creation_order_when_there_are_no_other_edges` |
| Topological sort | `sorting.stages.topological` | tagged | `src/sort_tests.rs::load_after_and_requirements_decide_the_relative_positions` |
| Messages LOOT's application generates when it checks the install (`generated_messages`, `generated_general_messages`) | `sorting.stages.generated_messages` | tagged | `src/validate_tests.rs::a_missing_or_inactive_master_of_an_active_plugin_is_an_error_and_of_an_inactive_one_nothing`, `tests/golden_app.rs::every_message_is_what_the_loot_application_showed`, `tests/golden_sort.rs::every_case_prints_what_its_golden_files_say` |
| The Bash Tags of the plugin's description (`bash_tags`) | `sorting.stages.bash_tags_description` | tagged | `src/validate_tests.rs::the_tags_of_a_description_are_those_between_the_braces` |

### Additions of this project (`tool.cli`)

| Item | Id | State | Tests |
| --- | --- | --- | --- |
| `loot-rs check`: read-only check of a load order against the masterlist rules | `tool.cli.check_command` | tagged | `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| `--strict-groups`: group order violations are hard | `tool.cli.strict_groups` | tagged | `src/check_tests.rs::strict_groups_make_group_violations_hard`, `tests/golden.rs::every_case_prints_what_its_golden_file_says` |
| JSON output of `sort` with an integer schema version | `tool.cli.json_output` | tagged | `src/api_tests.rs::the_sort_json_starts_with_the_integer_schema_version` |
| Parallel reading of plugins with a shared memory budget, the same result for any thread count | `tool.cli.parallel_sort` | tagged | `src/parallel_tests.rs::map_returns_the_results_in_the_order_of_the_items_for_any_thread_count` |
| Every size that comes from the input is bounded | `tool.cli.resource_limits` | tagged | `tests/limits.rs::hostile_input_is_refused_quickly_and_with_little_memory` |
| Copyable text writes the home directory as `~` | `tool.cli.home_tilde` | tagged | `src/api_tests.rs::every_home_directory_of_the_text_is_written_as_a_tilde` |
