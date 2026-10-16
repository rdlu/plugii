//! `plugii watch` and `plugii replay`: the TUI. [`app`] holds the state and
//! takes keys and updates without a terminal; [`ui`] draws it; [`run`] wires
//! both to the terminal and the watcher, [`replay()`] to a recording.

pub mod activity;
pub mod app;
pub mod charts;
pub mod detail;
pub mod ghosts;
pub mod keys;
pub mod ports;
pub mod replay;
pub mod stats;
pub mod ui;

use std::env;
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use color_eyre::eyre::{Result, WrapErr, bail};
use crossterm::event::{self, Event as TermEvent, KeyEventKind};

use crate::diff;
use crate::history::Sample;
use crate::labels::{self, Labels};
use crate::model::Snapshot;
use crate::record::{Output, Recorder, Recording};
use crate::sysfs::Root;
use crate::watch::{self, SharedLabels, Update};

pub use app::App;
use app::{LabelsFile, Notice};
use replay::Replay;

/// How long to wait for a key before checking the watcher again.
const POLL: Duration = Duration::from_millis(100);
/// Redraw at least this often, for the clock and expiring flashes.
const REDRAW: Duration = Duration::from_secs(1);

/// `watch --record`: where every snapshot goes, and the file as the status
/// line shows it.
#[derive(Debug)]
pub struct RecordTo {
    pub recorder: Recorder<Output>,
    pub shown: String,
}

/// Runs the TUI until the user quits. The terminal is restored on exit and
/// on panic. `labels_path` is where the label wizard writes (`None`: it
/// can't). With `record`, every snapshot the watcher sends is written there,
/// paused or not; a write error stops the recording, not the TUI.
pub fn run(
    root: Root,
    labels: Labels,
    labels_path: Option<PathBuf>,
    mut record: Option<RecordTo>,
) -> Result<()> {
    let mut app = App::new(local_offset());
    let shared: SharedLabels = Arc::new(RwLock::new(labels));
    let home = env::var_os("HOME").map(PathBuf::from);
    app.labels_file = labels_path.map(|path| {
        let shown = labels::display_path(&path, home.as_deref());
        LabelsFile::new(path, shown, Arc::clone(&shared))
    });
    app.recording = record.as_ref().map(|r| r.shown.clone());
    let updates = watch::spawn(root, shared);
    let mut terminal = ratatui::try_init()?;
    let result = run_loop(&mut terminal, &mut app, &updates, &mut record);
    ratatui::restore();
    // Ctrl-c is a key in raw mode, so quitting always gets here and the
    // gzip trailer is written.
    if let Some(r) = record {
        r.recorder
            .finish()
            .wrap_err_with(|| format!("finishing the recording {}", r.shown))?;
    }
    result
}

/// Local time's offset from UTC. Read before any thread starts: the `time`
/// crate refuses to once the process is multi-threaded.
fn local_offset() -> i32 {
    time::UtcOffset::current_local_offset()
        .map(|o| o.whole_seconds())
        .unwrap_or(0)
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    updates: &std::sync::mpsc::Receiver<Update>,
    record: &mut Option<RecordTo>,
) -> Result<()> {
    let mut prev: Option<Snapshot> = None;
    let mut watcher_alive = true;
    let mut dirty = true;
    let mut last_draw = Instant::now();
    while !app.should_quit {
        app.now_ms = now_ms();
        if dirty || last_draw.elapsed() >= REDRAW {
            terminal.draw(|f| ui::draw(f, app))?;
            last_draw = Instant::now();
            dirty = false;
        }
        if event::poll(POLL)? {
            match event::read()? {
                TermEvent::Key(key) if key.kind != KeyEventKind::Release => {
                    app.on_key(key);
                    dirty = true;
                }
                TermEvent::Resize(w, h) => {
                    app.size = (w, h);
                    dirty = true;
                }
                _ => {}
            }
        }
        while watcher_alive {
            match updates.try_recv() {
                Ok(Update::Snapshot(snapshot)) => {
                    record_snapshot(app, record, &snapshot);
                    let events = prev
                        .as_ref()
                        .map(|p| diff::diff(p, &snapshot))
                        .unwrap_or_default();
                    let sample = Sample::from_snapshot(&snapshot);
                    prev = Some((*snapshot).clone());
                    app.on_update(*snapshot, events, sample);
                    dirty = true;
                }
                Ok(Update::Warning(w)) => {
                    app.on_warning(w);
                    dirty = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    app.on_warning("the watcher stopped: no more updates".into());
                    watcher_alive = false;
                    dirty = true;
                }
            }
        }
    }
    Ok(())
}

/// Writes `snapshot` to the recording; the first error stops it and says so
/// once in the status line.
fn record_snapshot(app: &mut App, record: &mut Option<RecordTo>, snapshot: &Snapshot) {
    let Some(r) = record else {
        return;
    };
    if let Err(e) = r.recorder.record(snapshot) {
        app.notice = Some(Notice {
            text: format!("recording stopped: can't write {}: {e}", r.shown),
            error: true,
        });
        app.recording = None;
        if let Some(r) = record.take() {
            // Best effort: the file already failed a write.
            let _ = r.recorder.finish();
        }
    }
}

/// Plays `recording` back until the user quits. `file` is its name for the
/// status line; `speed` one of [`replay::SPEEDS`] (others snap down).
pub fn replay(recording: Recording, file: String, speed: u32) -> Result<()> {
    let warnings = recording.warnings.clone();
    let app = App::new(local_offset());
    let Some(mut replay) = Replay::new(app, recording, file.clone(), speed) else {
        bail!("{file} has no snapshots to replay");
    };
    if !warnings.is_empty() {
        replay.app.warning = Some(warnings.join("; "));
    }
    let mut terminal = ratatui::try_init()?;
    let result = replay_loop(&mut terminal, &mut replay);
    ratatui::restore();
    result
}

fn replay_loop(terminal: &mut ratatui::DefaultTerminal, replay: &mut Replay) -> Result<()> {
    let mut dirty = true;
    let mut last_draw = Instant::now();
    let mut last_tick = Instant::now();
    while !replay.app.should_quit {
        if dirty || last_draw.elapsed() >= REDRAW {
            terminal.draw(|f| ui::draw(f, &mut replay.app))?;
            last_draw = Instant::now();
            dirty = false;
        }
        let wait = replay.player.until_next().map_or(POLL, |d| d.min(POLL));
        if event::poll(wait)? {
            match event::read()? {
                TermEvent::Key(key) if key.kind != KeyEventKind::Release => {
                    replay.on_key(key);
                    dirty = true;
                }
                TermEvent::Resize(w, h) => {
                    replay.app.size = (w, h);
                    dirty = true;
                }
                _ => {}
            }
        }
        let before = replay.player.current();
        let now = Instant::now();
        replay.tick(now - last_tick);
        last_tick = now;
        dirty |= replay.player.current() != before;
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
