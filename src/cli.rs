//! Command line: `plugii [-H] [--json] [--firmware]`, `plugii watch` / `-w`,
//! `plugii watch --record <file> [--redact] [--force]`, `plugii capture <dir>`,
//! `plugii replay <file> [--speed N]`, `plugii labels`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::tui::replay::SPEEDS;

#[derive(Debug, Parser)]
#[command(version, about, args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Health verdict only.
    #[arg(short = 'H', long)]
    pub health: bool,

    /// Print the snapshot as JSON.
    #[arg(long)]
    pub json: bool,

    /// Also check firmware versions (DMI, fwupd, vendor catalogs; uses the network).
    #[arg(long)]
    pub firmware: bool,

    /// Same as `plugii watch`.
    #[arg(short = 'w', long = "watch", conflicts_with_all = ["health", "json", "firmware"])]
    pub watch: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Live view: updates as devices, chargers and displays come and go.
    Watch {
        /// Also record every snapshot to FILE as JSON lines (gzip-compressed
        /// when FILE ends in .gz), for `plugii replay`.
        #[arg(long, value_name = "FILE")]
        record: Option<PathBuf>,
        /// Replace device serial numbers and Thunderbolt unique IDs in the
        /// recording with REDACTED.
        #[arg(long, requires = "record")]
        redact: bool,
        /// Overwrite FILE if it exists.
        #[arg(long, requires = "record")]
        force: bool,
    },
    /// Copy the sysfs files plugii reads into DIR, as a test fixture.
    Capture { dir: PathBuf },
    /// Play back a file written by `watch --record`.
    Replay {
        file: PathBuf,
        /// Playback speed: 1, 2, 4, 10, 30 or 60 (`+` / `-` change it).
        #[arg(long, default_value_t = 1, value_parser = parse_speed)]
        speed: u32,
    },
    /// List the labels file's sockets, peers, USB-C ports and devices.
    Labels,
}

/// What to do, with flag aliases resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Report {
        health_only: bool,
        json: bool,
        firmware: bool,
    },
    Watch {
        record: Option<Record>,
    },
    Capture {
        dir: PathBuf,
    },
    Replay {
        file: PathBuf,
        speed: u32,
    },
    Labels,
}

/// `watch --record FILE [--redact] [--force]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub file: PathBuf,
    pub redact: bool,
    pub force: bool,
}

/// One of the speeds replay steps through.
fn parse_speed(s: &str) -> Result<u32, String> {
    s.parse()
        .ok()
        .filter(|n| SPEEDS.contains(n))
        .ok_or_else(|| "must be one of 1, 2, 4, 10, 30, 60".to_owned())
}

impl Cli {
    pub fn action(self) -> Action {
        match self.command {
            Some(Command::Watch {
                record,
                redact,
                force,
            }) => Action::Watch {
                record: record.map(|file| Record {
                    file,
                    redact,
                    force,
                }),
            },
            Some(Command::Capture { dir }) => Action::Capture { dir },
            Some(Command::Replay { file, speed }) => Action::Replay { file, speed },
            Some(Command::Labels) => Action::Labels,
            None if self.watch => Action::Watch { record: None },
            None => Action::Report {
                health_only: self.health,
                json: self.json,
                firmware: self.firmware,
            },
        }
    }
}
