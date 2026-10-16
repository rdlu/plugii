use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use color_eyre::eyre::{Result, WrapErr, bail};
use plugii::cli::{self, Action, Cli};
use plugii::collect::{self, CollectOpts};
use plugii::labels::{self, Labels};
use plugii::record::{self, Output, Recorder};
use plugii::sysfs::Root;
use plugii::tui::RecordTo;
use plugii::{health, report};

fn main() -> Result<ExitCode> {
    // User-facing errors: no source location or backtrace hints.
    color_eyre::config::HookBuilder::default()
        .display_location_section(false)
        .display_env_section(false)
        .install()?;
    match Cli::parse().action() {
        Action::Report { firmware: true, .. } => not_yet("plugii --firmware"),
        Action::Report {
            health_only, json, ..
        } => run_report(health_only, json),
        Action::Watch { record } => watch(record),
        Action::Capture { dir } => capture(&dir),
        Action::Replay { file, speed } => replay(&file, speed),
        Action::Labels => print_labels(),
    }
}

/// The report, `-H` or `--json`; exit status 1 when any finding is a
/// warning or a failure, as the script does.
fn run_report(health_only: bool, json: bool) -> Result<ExitCode> {
    let labels = load_labels()?;
    let snapshot = collect::snapshot(&Root::live(), &labels, &CollectOpts::live());
    let findings = health::check(&snapshot);
    let mut stdout = std::io::stdout().lock();
    if json {
        let text = serde_json::to_string_pretty(&report::json(&snapshot, &findings))?;
        writeln!(stdout, "{text}")?;
    } else {
        let color = std::io::stdout().is_terminal();
        let text = if health_only {
            report::render_health(&findings, color)
        } else {
            report::render(&snapshot, &findings, color)
        };
        write!(stdout, "{text}")?;
    }
    stdout.flush()?;
    Ok(if health::is_bad(&findings) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// The labels file, if there is one; no config directory means no labels.
fn load_labels() -> Result<Labels> {
    let Some(dir) = labels::config_dir() else {
        return Ok(Labels::default());
    };
    match labels::find(&dir) {
        Some(path) => labels::read(&path).wrap_err_with(|| format!("reading {}", path.display())),
        None => Ok(Labels::default()),
    }
}

fn print_labels() -> Result<ExitCode> {
    let Some(dir) = labels::config_dir() else {
        bail!("can't find the config directory: neither XDG_CONFIG_HOME nor HOME is set");
    };
    let Some(path) = labels::find(&dir) else {
        println!("no labels file ({})", labels::path(&dir).display());
        return Ok(ExitCode::SUCCESS);
    };
    let labels = labels::read(&path).wrap_err_with(|| format!("reading {}", path.display()))?;
    println!("labels from {}", path.display());
    print!("{}", labels.summary());
    Ok(ExitCode::SUCCESS)
}

/// The live TUI, recording with `--record`. The file is created before the
/// TUI starts, so an existing one stops it with a plain error.
fn watch(record: Option<cli::Record>) -> Result<ExitCode> {
    let labels = load_labels()?;
    let path = labels::config_dir().map(|dir| labels::path(&dir));
    let record = match record {
        Some(r) => {
            let out = Output::create(&r.file, r.force)
                .wrap_err_with(|| format!("can't record to {}", r.file.display()))?;
            Some(RecordTo {
                recorder: Recorder::new(out, r.redact),
                shown: shown(&r.file),
            })
        }
        None => None,
    };
    plugii::tui::run(Root::live(), labels, path, record)?;
    Ok(ExitCode::SUCCESS)
}

fn replay(file: &Path, speed: u32) -> Result<ExitCode> {
    let recording =
        record::read(file).wrap_err_with(|| format!("can't replay {}", file.display()))?;
    plugii::tui::replay(recording, shown(file), speed)?;
    Ok(ExitCode::SUCCESS)
}

/// A path as the status line shows it, `~` for the home directory.
fn shown(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    labels::display_path(path, home.as_deref())
}

fn capture(dir: &std::path::Path) -> Result<ExitCode> {
    let summary = plugii::capture::capture(&Root::live(), dir)
        .wrap_err_with(|| format!("capturing into {}", dir.display()))?;
    println!(
        "captured {} files ({} bytes) and {} symlinks into {}; {} redacted, {} skipped",
        summary.files,
        summary.bytes,
        summary.links,
        dir.display(),
        summary.redacted,
        summary.skipped
    );
    Ok(ExitCode::SUCCESS)
}

fn not_yet(what: &str) -> Result<ExitCode> {
    bail!("`{what}` is not implemented yet")
}
