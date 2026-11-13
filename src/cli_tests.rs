//! Unit tests of `cli`.

use super::*;
use crate::support_tests::TempDir;

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(ToString::to_string).collect()
}

fn full() -> Vec<String> {
    strings(&[
        "check",
        "--masterlist",
        "m.yaml",
        "--plugins",
        "p.txt",
        "--data",
        "Data",
    ])
}

#[test]
fn all_flags_are_read() {
    let args = parse_args(&full()).expect("parses");
    assert_eq!(args.masterlist, PathBuf::from("m.yaml"));
    assert_eq!(args.plugins, PathBuf::from("p.txt"));
    assert_eq!(args.data, PathBuf::from("Data"));
    assert!(!args.options.strict_groups);
}

#[test]
fn strict_groups_and_any_flag_order() {
    let args = parse_args(&strings(&[
        "check",
        "--strict-groups",
        "--data",
        "D",
        "--plugins",
        "p",
        "--masterlist",
        "m",
    ]))
    .expect("parses");
    assert!(args.options.strict_groups);
    assert_eq!(args.data, PathBuf::from("D"));
}

#[test]
fn a_later_flag_replaces_an_earlier_one() {
    let mut args = full();
    args.extend(strings(&["--data", "Other"]));
    assert_eq!(
        parse_args(&args).expect("parses").data,
        PathBuf::from("Other")
    );
}

#[test]
fn problems_with_the_command_and_flags_are_named() {
    let error = |args: &[&str]| parse_args(&strings(args)).expect_err("fails");
    assert_eq!(error(&[]), "missing command");
    assert_eq!(error(&["frobnicate"]), "unknown command \"frobnicate\"");
    assert_eq!(error(&["sort"]), "--masterlist is required");
    assert_eq!(error(&["check"]), "--masterlist is required");
    assert_eq!(
        error(&["check", "--masterlist", "m"]),
        "--plugins is required"
    );
    assert_eq!(
        error(&["check", "--masterlist", "m", "--plugins", "p"]),
        "--data is required"
    );
    assert_eq!(
        error(&["check", "--masterlist"]),
        "--masterlist needs a value"
    );
    assert_eq!(error(&["check", "--plugins"]), "--plugins needs a value");
    assert_eq!(error(&["check", "--data"]), "--data needs a value");
    assert_eq!(
        error(&["check", "--verbose"]),
        "unknown argument \"--verbose\""
    );
}

fn files(dir: &TempDir, masterlist: &str, plugins: &str) -> Vec<String> {
    std::fs::create_dir_all(dir.path().join("Data")).expect("data");
    let m = dir.file("masterlist.yaml", masterlist.as_bytes());
    let p = dir.file("plugins.txt", plugins.as_bytes());
    strings(&[
        "check",
        "--masterlist",
        m.to_str().expect("utf8"),
        "--plugins",
        p.to_str().expect("utf8"),
        "--data",
        dir.path().join("Data").to_str().expect("utf8"),
    ])
}

fn run_main(args: &[String]) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).expect("utf8"),
        String::from_utf8(err).expect("utf8"),
    )
}

#[test]
fn a_clean_run_exits_0_and_prints_the_report() {
    let dir = TempDir::new();
    let args = files(&dir, "plugins: []\n", "");
    let (code, out, err) = run_main(&args);
    assert_eq!((code, err.as_str()), (EXIT_OK, ""));
    assert!(out.ends_with("Hard violations: 0.\n"), "{out}");
}

#[test]
fn a_hard_violation_exits_1() {
    let dir = TempDir::new();
    dir.plugin("Data/A.esp", &[]);
    let args = files(
        &dir,
        "plugins:\n  - name: A.esp\n    req: ['Gone.esp']\n",
        "A.esp\n",
    );
    let (code, out, _) = run_main(&args);
    assert_eq!(code, EXIT_VIOLATION);
    assert!(out.contains("Hard violations: 1."), "{out}");
}

#[test]
fn unreadable_inputs_exit_2_and_name_the_file() {
    let dir = TempDir::new();
    let mut args = files(&dir, "plugins: []\n", "");
    args[2] = dir
        .path()
        .join("absent.yaml")
        .to_string_lossy()
        .into_owned();
    let (code, out, err) = run_main(&args);
    assert_eq!(code, EXIT_ERROR);
    assert_eq!(out, "");
    assert!(err.starts_with("loot-rs: "), "{err}");
    assert!(err.contains("absent.yaml"), "{err}");

    let mut args = files(&dir, "plugins: []\n", "");
    args[4] = dir.path().join("absent.txt").to_string_lossy().into_owned();
    assert!(run_main(&args).2.contains("absent.txt"));

    let mut args = files(&dir, "plugins: []\n", "");
    args[6] = dir
        .path()
        .join("masterlist.yaml")
        .to_string_lossy()
        .into_owned();
    assert!(run_main(&args).2.contains("not a directory"));
}

#[test]
fn an_invalid_masterlist_exits_2() {
    let dir = TempDir::new();
    let args = files(&dir, "a: [unclosed", "");
    let (code, _, err) = run_main(&args);
    assert_eq!(code, EXIT_ERROR);
    assert!(err.contains("masterlist: YAML"), "{err}");
}

#[test]
fn argument_errors_print_the_usage() {
    let (code, out, err) = run_main(&strings(&["check"]));
    assert_eq!(code, EXIT_ERROR);
    assert_eq!(out, "");
    assert!(err.contains("--masterlist is required"), "{err}");
    assert!(err.contains(USAGE), "{err}");
}

#[test]
fn a_report_that_cannot_be_written_is_an_error() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = TempDir::new();
    let args = parse_args(&files(&dir, "plugins: []\n", "")).expect("parses");
    let error = execute(&args, &mut Closed).expect_err("fails");
    assert!(error.contains("cannot write the report"), "{error}");
}

#[test]
fn the_exit_codes_are_the_documented_ones() {
    assert_eq!((EXIT_OK, EXIT_VIOLATION, EXIT_ERROR), (0, 1, 2));
}

#[test]
fn json_is_a_flag_of_check() {
    let mut args = full();
    args.push("--json".into());
    assert!(parse_args(&args).expect("parses").json);
    assert!(!parse_args(&full()).expect("parses").json);
}

#[test]
fn json_output_is_one_line_and_keeps_the_exit_code() {
    let dir = TempDir::new();
    dir.plugin("Data/A.esp", &[]);
    let mut args = files(
        &dir,
        "plugins:\n  - name: A.esp\n    req: ['Gone.esp']\n",
        "A.esp\n",
    );
    args.push("--json".into());
    let (code, out, err) = run_main(&args);
    assert_eq!((code, err.as_str()), (EXIT_VIOLATION, ""));
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.starts_with("{\"general_messages\":[]"), "{out}");
    assert!(out.contains("\"hard_violations\":1}"), "{out}");
}

#[test]
fn the_command_is_check_or_sort() {
    let mut sort = full();
    sort[0] = "sort".into();
    assert_eq!(parse_args(&full()).expect("parses").command, Command::Check);
    assert_eq!(parse_args(&sort).expect("parses").command, Command::Sort);
}
