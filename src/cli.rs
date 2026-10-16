//! Command line: `plugii [-H] [--json] [--firmware]`, `plugii watch` / `-w`,
//! `plugii capture <dir>`, `plugii replay <file>`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
        /// Also append every snapshot to FILE as JSON lines.
        #[arg(long, value_name = "FILE")]
        record: Option<PathBuf>,
    },
    /// Copy the sysfs files plugii reads into DIR, as a test fixture.
    Capture { dir: PathBuf },
    /// Play back a file written by `watch --record`.
    Replay { file: PathBuf },
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
        record: Option<PathBuf>,
    },
    Capture {
        dir: PathBuf,
    },
    Replay {
        file: PathBuf,
    },
}

impl Cli {
    pub fn action(self) -> Action {
        match self.command {
            Some(Command::Watch { record }) => Action::Watch { record },
            Some(Command::Capture { dir }) => Action::Capture { dir },
            Some(Command::Replay { file }) => Action::Replay { file },
            None if self.watch => Action::Watch { record: None },
            None => Action::Report {
                health_only: self.health,
                json: self.json,
                firmware: self.firmware,
            },
        }
    }
}
