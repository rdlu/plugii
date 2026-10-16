use clap::Parser;
use color_eyre::eyre::{Result, WrapErr, bail};
use plugii::cli::{Action, Cli};
use plugii::labels;
use plugii::sysfs::Root;

fn main() -> Result<()> {
    // User-facing errors: no source location or backtrace hints.
    color_eyre::config::HookBuilder::default()
        .display_location_section(false)
        .display_env_section(false)
        .install()?;
    match Cli::parse().action() {
        Action::Report {
            health_only: false,
            json: false,
            firmware: false,
        } => print_labels(),
        Action::Report {
            health_only: true, ..
        } => not_yet("plugii -H"),
        Action::Report { json: true, .. } => not_yet("plugii --json"),
        Action::Report { firmware: true, .. } => not_yet("plugii --firmware"),
        Action::Watch { record: None } => not_yet("plugii watch"),
        Action::Watch { record: Some(_) } => not_yet("plugii watch --record"),
        Action::Capture { dir } => capture(&dir),
        Action::Replay { .. } => not_yet("plugii replay"),
    }
}

fn capture(dir: &std::path::Path) -> Result<()> {
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
    Ok(())
}

fn not_yet(what: &str) -> Result<()> {
    bail!("`{what}` is not implemented yet")
}

/// Stands in for the report until the sysfs readers exist.
fn print_labels() -> Result<()> {
    let Some(dir) = labels::config_dir() else {
        bail!("can't find the config directory: neither XDG_CONFIG_HOME nor HOME is set");
    };
    let Some(path) = labels::find(&dir) else {
        println!(
            "no labels file ({} or {})",
            dir.join("plugii/labels").display(),
            dir.join("usb-ports/labels").display()
        );
        return Ok(());
    };
    let labels = labels::read(&path).wrap_err_with(|| format!("reading {}", path.display()))?;
    println!("labels from {}", path.display());
    print!("{}", labels.summary());
    Ok(())
}
