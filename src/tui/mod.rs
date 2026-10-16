//! `plugii watch`: the live TUI. [`app`] holds the state and takes keys and
//! updates without a terminal; [`ui`] draws it; [`run`] wires both to the
//! terminal and the watcher.

pub mod activity;
pub mod app;
pub mod charts;
pub mod detail;
pub mod ghosts;
pub mod keys;
pub mod ports;
pub mod stats;
pub mod ui;

use std::env;
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use color_eyre::eyre::Result;
use crossterm::event::{self, Event as TermEvent, KeyEventKind};

use crate::diff;
use crate::history::Sample;
use crate::labels::{self, Labels};
use crate::model::Snapshot;
use crate::sysfs::Root;
use crate::watch::{self, SharedLabels, Update};

pub use app::App;
use app::LabelsFile;

/// How long to wait for a key before checking the watcher again.
const POLL: Duration = Duration::from_millis(100);
/// Redraw at least this often, for the clock and expiring flashes.
const REDRAW: Duration = Duration::from_secs(1);

/// Runs the TUI until the user quits. The terminal is restored on exit and
/// on panic. `labels_path` is where the label wizard writes (`None`: it
/// can't).
pub fn run(root: Root, labels: Labels, labels_path: Option<PathBuf>) -> Result<()> {
    // Read the local offset before any thread starts: the `time` crate
    // refuses to once the process is multi-threaded.
    let offset = time::UtcOffset::current_local_offset()
        .map(|o| o.whole_seconds())
        .unwrap_or(0);
    let mut app = App::new(offset);
    let shared: SharedLabels = Arc::new(RwLock::new(labels));
    let home = env::var_os("HOME").map(PathBuf::from);
    app.labels_file = labels_path.map(|path| {
        let shown = labels::display_path(&path, home.as_deref());
        LabelsFile::new(path, shown, Arc::clone(&shared))
    });
    let updates = watch::spawn(root, shared);
    let mut terminal = ratatui::try_init()?;
    let result = run_loop(&mut terminal, &mut app, &updates);
    ratatui::restore();
    result
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    updates: &std::sync::mpsc::Receiver<Update>,
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
