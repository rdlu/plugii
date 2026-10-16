use std::path::PathBuf;

use clap::{CommandFactory, Parser};
use plugii::cli::{Action, Cli};

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
fn watch_subcommand_and_flag() {
    assert_eq!(action(&["watch"]), Action::Watch { record: None });
    assert_eq!(action(&["-w"]), Action::Watch { record: None });
    assert_eq!(action(&["--watch"]), Action::Watch { record: None });
    assert_eq!(
        action(&["watch", "--record", "f.jsonl"]),
        Action::Watch {
            record: Some(PathBuf::from("f.jsonl"))
        }
    );
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
            file: "f.jsonl".into()
        }
    );
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
