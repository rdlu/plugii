use std::path::PathBuf;

use clap::{CommandFactory, Parser};
use plugii::cli::{Action, Cli, Record};

fn action(args: &[&str]) -> Action {
    let argv = std::iter::once("plugii").chain(args.iter().copied());
    Cli::try_parse_from(argv).unwrap().action()
}

fn rejects(args: &[&str]) -> bool {
    let argv = std::iter::once("plugii").chain(args.iter().copied());
    Cli::try_parse_from(argv).is_err()
}

fn report(health_only: bool, json: bool, firmware: bool) -> Action {
    Action::Report {
        health_only,
        json,
        firmware,
        refresh: false,
    }
}

#[test]
fn definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn default_is_the_full_report() {
    assert_eq!(action(&[]), report(false, false, false));
}

#[test]
fn report_flags() {
    assert_eq!(action(&["-H"]), report(true, false, false));
    assert_eq!(action(&["--health"]), report(true, false, false));
    assert_eq!(action(&["--json"]), report(false, true, false));
    assert_eq!(action(&["--firmware"]), report(false, false, true));
    assert_eq!(
        action(&["-H", "--json", "--firmware"]),
        report(true, true, true)
    );
}

#[test]
fn refresh_needs_firmware() {
    assert_eq!(
        action(&["--firmware", "--refresh"]),
        Action::Report {
            health_only: false,
            json: false,
            firmware: true,
            refresh: true,
        }
    );
    assert!(rejects(&["--refresh"]));
    assert!(rejects(&["-w", "--refresh"]));
}

#[test]
fn watch_subcommand_and_flag() {
    assert_eq!(action(&["watch"]), Action::Watch { record: None });
    assert_eq!(action(&["-w"]), Action::Watch { record: None });
    assert_eq!(action(&["--watch"]), Action::Watch { record: None });
    let record = |file: &str, redact, force| Action::Watch {
        record: Some(Record {
            file: PathBuf::from(file),
            redact,
            force,
        }),
    };
    assert_eq!(
        action(&["watch", "--record", "f.jsonl"]),
        record("f.jsonl", false, false)
    );
    assert_eq!(
        action(&["watch", "--record", "f.jsonl.gz", "--redact", "--force"]),
        record("f.jsonl.gz", true, true)
    );
    // --redact and --force only go with --record.
    assert!(rejects(&["watch", "--redact"]));
    assert!(rejects(&["watch", "--force"]));
}

#[test]
fn capture_and_replay() {
    assert_eq!(
        action(&["capture", "fx/xps"]),
        Action::Capture {
            dir: "fx/xps".into()
        }
    );
    assert_eq!(
        action(&["replay", "f.jsonl"]),
        Action::Replay {
            file: "f.jsonl".into(),
            speed: 1
        }
    );
    assert_eq!(
        action(&["replay", "f.jsonl", "--speed", "30"]),
        Action::Replay {
            file: "f.jsonl".into(),
            speed: 30
        }
    );
    assert!(rejects(&["replay", "f.jsonl", "--speed", "3"]));
    assert!(rejects(&["replay", "f.jsonl", "--speed", "0"]));
}

#[test]
fn labels_subcommand() {
    assert_eq!(action(&["labels"]), Action::Labels);
    assert!(rejects(&["labels", "x"]));
    assert!(rejects(&["--json", "labels"]));
}

#[test]
fn invalid_combinations() {
    assert!(rejects(&["capture"]));
    assert!(rejects(&["replay"]));
    assert!(rejects(&["-w", "--json"]));
    assert!(rejects(&["-w", "-H"]));
    assert!(rejects(&["--json", "watch"]));
    assert!(rejects(&["-w", "capture", "x"]));
    assert!(rejects(&["bogus"]));
}

/// `plugii [command] --help` as the user sees it. The docs site mounts these
/// snapshots for its CLI reference, so the page can't drift from clap.
fn help(args: &[&str]) -> String {
    let argv = std::iter::once("plugii")
        .chain(args.iter().copied())
        .chain(["--help"]);
    let err = Cli::try_parse_from(argv).unwrap_err();
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    err.render().to_string()
}

#[test]
fn help_plugii() {
    insta::assert_snapshot!(help(&[]));
}

#[test]
fn help_watch() {
    insta::assert_snapshot!(help(&["watch"]));
}

#[test]
fn help_replay() {
    insta::assert_snapshot!(help(&["replay"]));
}

#[test]
fn help_capture() {
    insta::assert_snapshot!(help(&["capture"]));
}

#[test]
fn help_labels() {
    insta::assert_snapshot!(help(&["labels"]));
}
