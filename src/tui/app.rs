//! The TUI's state, with no terminal: snapshots, events and samples come in
//! through [`App::on_update`], keys through [`App::on_key`], and the
//! renderer reads the rest.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::detail::Target;
use super::ghosts::{self, Ghost};
use super::keys::{Action, Dir, Pending};
use super::ports::{self, RowKind, TreeOpts};
use super::stats::Stats;
use crate::collect;
use crate::diff::Event;
use crate::health;
use crate::history::{History, Sample};
use crate::labels::{self, Key, Labels};
use crate::model::{Finding, Snapshot};
use crate::watch::{self, SharedLabels};

/// Samples kept for the charts: 10 minutes at the watcher's 1 s tick.
pub const HISTORY_LEN: usize = 600;
/// Events kept for the Events tab; older ones are dropped.
pub const EVENTS_MAX: usize = 1000;
/// How long a socket whose state just changed stays highlighted.
pub const FLASH_MS: u64 = 2000;
/// Below this many columns the side panels go below the sockets.
pub const NARROW_COLS: u16 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Ports,
    Charts,
    Events,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Ports, Tab::Charts, Tab::Events];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Ports => "Ports",
            Tab::Charts => "Charts",
            Tab::Events => "Events",
        }
    }
}

/// A focusable region of a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sockets,
    UsbC,
    Power,
    Health,
    PowerChart,
    CpuChart,
    Links,
    Urbs,
    Chargers,
    Events,
}

const PANES: usize = 10;

/// A pane's position. In the Sockets and Events panes `sel` is the selected
/// row and `offset` the first row shown (the renderer keeps `sel` in view);
/// the side panels only scroll, by `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneState {
    pub sel: usize,
    pub offset: usize,
    /// Rows the pane showed at the last draw, for half-page moves.
    pub height: usize,
}

impl Default for PaneState {
    fn default() -> Self {
        Self {
            sel: 0,
            offset: 0,
            height: 20,
        }
    }
}

/// A popup over the tab; it takes the keys until closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Detail(Target),
    Help,
}

/// The labels file the label wizard writes, and the labels it holds,
/// shared with the watcher.
#[derive(Debug, Clone)]
pub struct LabelsFile {
    /// `~/.config/plugii/labels` in the TUI; a temporary file in tests.
    pub path: PathBuf,
    /// The path as the status line shows it, `~` for the home directory.
    pub shown: String,
    pub labels: SharedLabels,
}

impl LabelsFile {
    pub fn new(path: PathBuf, shown: String, labels: SharedLabels) -> Self {
        Self {
            path,
            shown,
            labels,
        }
    }
}

/// The `L` prompt: what it names, and the text so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelPrompt {
    pub key: Key,
    /// `socket 0000:06:00.3-1`, `device 0b95:1790`.
    pub what: String,
    /// What the prompt line starts with: `label device 0b95:1790 (AX88179B): `.
    pub title: String,
    pub text: String,
}

/// A one-off message in the status line: the label wizard's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

/// Updates held back while paused, applied on resume so the event list and
/// the chart history have no gap.
#[derive(Debug, Default)]
struct Held {
    snapshot: Option<Snapshot>,
    events: Vec<Event>,
    samples: Vec<Sample>,
    /// The Charts tab's stats, carried on from the shown ones.
    stats: Option<Stats>,
}

#[derive(Debug)]
pub struct App {
    pub snapshot: Option<Snapshot>,
    pub findings: Vec<Finding>,
    pub history: History<Sample>,
    /// URB rates, link speeds and chargers for the Charts tab.
    pub stats: Stats,
    pub events: VecDeque<Event>,
    pub tab: Tab,
    /// The focused pane of each tab.
    focus: [Pane; 3],
    panes: [PaneState; PANES],
    /// Hubs collapsed in the Ports tree, by sysfs name.
    pub collapsed: HashSet<String>,
    keys: Pending,
    pub paused: bool,
    paused_at_ms: u64,
    held: Held,
    /// The Events pane sticks to the newest event until the user moves up.
    pub events_follow: bool,
    pub warning: Option<String>,
    pub should_quit: bool,
    /// Unix ms; the run loop sets it, tests fix it.
    pub now_ms: u64,
    /// Local time's offset from UTC, for clock times.
    pub utc_offset_s: i32,
    /// Terminal columns and rows.
    pub size: (u16, u16),
    pub overlay: Option<Overlay>,
    /// The popup's first line shown, and its height at the last draw.
    pub overlay_scroll: usize,
    pub overlay_height: usize,
    /// The `/` prompt is open: keys edit the current tab's filter.
    pub prompt: bool,
    /// Device filter on the Ports tab; empty is none.
    pub sockets_filter: String,
    /// Event-line filter on the Events tab.
    pub events_filter: String,
    /// Recently unplugged subtrees, shown until they expire.
    pub ghosts: Vec<Ghost>,
    /// Where `L` writes labels; without one (no config directory) it can't.
    /// Its labels are put on every snapshot that comes in.
    pub labels_file: Option<LabelsFile>,
    /// The `L` prompt is open: keys edit its text.
    pub label_prompt: Option<LabelPrompt>,
    pub notice: Option<Notice>,
    /// `watch --record`: the file being written, as the status line shows
    /// it; `None` when not recording or after a write failed.
    pub recording: Option<String>,
    /// Set in `plugii replay`: the player's state for the status line, and
    /// the replay key last pressed.
    pub replay: Option<ReplayView>,
}

/// What a replay shows in the status line; [`super::replay::Replay`] keeps
/// it current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayView {
    /// The recording's name, as the status line shows it.
    pub file: String,
    /// The recording header's host.
    pub host: String,
    /// The first and last snapshots' times, and where playback is.
    pub start_ms: u64,
    pub end_ms: u64,
    pub at_ms: u64,
    pub speed: u32,
    pub paused: bool,
    /// Played to the end (and paused there).
    pub ended: bool,
    /// The replay key pressed, for the replay to act on.
    pub request: Option<ReplayRequest>,
}

/// A replay key's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayRequest {
    TogglePause,
    Faster,
    Slower,
    NextPlug,
    PrevPlug,
}

impl App {
    pub fn new(utc_offset_s: i32) -> Self {
        Self {
            snapshot: None,
            findings: Vec::new(),
            history: History::new(HISTORY_LEN),
            stats: Stats::default(),
            events: VecDeque::new(),
            tab: Tab::Ports,
            focus: [Pane::Sockets, Pane::PowerChart, Pane::Events],
            panes: [PaneState::default(); PANES],
            collapsed: HashSet::new(),
            keys: Pending::default(),
            paused: false,
            paused_at_ms: 0,
            held: Held::default(),
            events_follow: true,
            warning: None,
            should_quit: false,
            now_ms: 0,
            utc_offset_s,
            size: (120, 40),
            overlay: None,
            overlay_scroll: 0,
            overlay_height: 20,
            prompt: false,
            sockets_filter: String::new(),
            events_filter: String::new(),
            ghosts: Vec::new(),
            labels_file: None,
            label_prompt: None,
            notice: None,
            recording: None,
            replay: None,
        }
    }

    /// Forgets every snapshot, event and sample, keeping what the user set
    /// (tab, focus, filters, collapsed hubs): a replay seeking back feeds
    /// the recording again from the start.
    pub fn clear_data(&mut self) {
        self.snapshot = None;
        self.findings.clear();
        self.history = History::new(HISTORY_LEN);
        self.stats = Stats::default();
        self.events.clear();
        self.events_follow = true;
        self.ghosts.clear();
        self.held = Held::default();
    }

    /// A new scan, the events since the previous one, and its chart sample.
    /// While paused these are held and applied on resume.
    ///
    /// The labels file's labels go on the snapshot: a scan the watcher
    /// started before `L` wrote new ones would otherwise show the old
    /// names for a moment.
    pub fn on_update(&mut self, mut snapshot: Snapshot, events: Vec<Event>, sample: Sample) {
        if let Some(file) = &self.labels_file {
            collect::apply_labels(&mut snapshot, &watch::current(&file.labels));
        }
        if self.paused {
            let stats = self.held.stats.get_or_insert_with(|| self.stats.clone());
            stats.observe(&snapshot);
            self.held.snapshot = Some(snapshot);
            self.held.events.extend(events);
            let extra = self.held.events.len().saturating_sub(EVENTS_MAX);
            self.held.events.drain(..extra);
            self.held.samples.push(sample);
            let extra = self.held.samples.len().saturating_sub(HISTORY_LEN);
            self.held.samples.drain(..extra);
            return;
        }
        self.stats.observe(&snapshot);
        self.apply_snapshot(snapshot, &events);
        self.push_events(events);
        self.history.push(sample);
    }

    /// The watcher's warning, shown in the status line.
    pub fn on_warning(&mut self, warning: String) {
        self.warning = Some(warning);
    }

    /// A key: to the prompt while it is open, else through the keymap to
    /// the popup or the tab.
    pub fn on_key(&mut self, key: KeyEvent) {
        if self.label_prompt.is_some() {
            self.label_prompt_key(key);
            return;
        }
        if self.prompt {
            self.prompt_key(key);
            return;
        }
        let action = self.keys.feed(key);
        if self.overlay.is_some() {
            self.overlay_action(action);
        } else {
            self.apply(action);
        }
    }

    /// The filter of the current tab, if it has one.
    pub fn filter(&self) -> Option<&str> {
        match self.tab {
            Tab::Ports => Some(&self.sockets_filter),
            Tab::Events => Some(&self.events_filter),
            Tab::Charts => None,
        }
    }

    /// Changes the current tab's filter, keeping the selected row if it
    /// stays.
    fn set_filter(&mut self, change: impl FnOnce(&mut String)) {
        match self.tab {
            Tab::Ports => {
                let selected = self.selected_row().map(|r| r.id);
                change(&mut self.sockets_filter);
                self.reselect(selected);
            }
            Tab::Events => {
                let selected = self
                    .visible_events()
                    .get(self.pane(Pane::Events).sel)
                    .map(|e| (*e).clone());
                change(&mut self.events_filter);
                let events = self.visible_events();
                let i = selected
                    .and_then(|s| events.iter().position(|e| **e == s))
                    .unwrap_or(usize::MAX);
                let len = events.len();
                self.pane_mut(Pane::Events).sel = i.min(len.saturating_sub(1));
            }
            Tab::Charts => {}
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.cancel_prompt(),
            KeyCode::Char('c') if ctrl => self.cancel_prompt(),
            KeyCode::Char('u') if ctrl => self.set_filter(String::clear),
            KeyCode::Enter => {
                self.prompt = false;
                if self.filter().is_some_and(str::is_empty) {
                    return;
                }
                if self.tab == Tab::Ports && !self.selected_row().is_some_and(|r| r.is_match()) {
                    self.jump_match(true);
                }
            }
            KeyCode::Backspace => {
                if self.filter().is_some_and(str::is_empty) {
                    self.prompt = false;
                } else {
                    self.set_filter(|f| {
                        f.pop();
                    });
                }
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.set_filter(|f| f.push(c));
            }
            _ => {}
        }
    }

    fn cancel_prompt(&mut self) {
        self.prompt = false;
        self.set_filter(String::clear);
    }

    /// `L` on the Ports tree: a prompt to label the selected device's
    /// vid:pid, or else the socket the row belongs to, filled with its
    /// current label.
    fn open_label_prompt(&mut self) {
        if self.tab != Tab::Ports || self.focus() != Pane::Sockets {
            return;
        }
        let Some(row) = self.selected_row() else {
            return;
        };
        if self.labels_file.is_none() {
            self.notice = Some(Notice {
                text: "can't label: no config directory (HOME and XDG_CONFIG_HOME unset)".into(),
                error: true,
            });
            return;
        }
        let prompt = match &row.kind {
            RowKind::Device { device, .. } => {
                let what = format!("device {:04x}:{:04x}", device.vendor_id, device.product_id);
                let title = match &device.product {
                    Some(p) => format!("label {what} ({p}): "),
                    None => format!("label {what}: "),
                };
                LabelPrompt {
                    key: Key::Device {
                        vendor_id: device.vendor_id,
                        product_id: device.product_id,
                    },
                    title,
                    what,
                    text: device.label.clone().unwrap_or_default(),
                }
            }
            _ => {
                let label = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.sockets.iter().find(|x| x.key == row.socket))
                    .and_then(|s| s.label.clone());
                let what = format!("socket {}", row.socket);
                LabelPrompt {
                    key: Key::Socket(row.socket.clone()),
                    title: format!("label {what}: "),
                    what,
                    text: label.unwrap_or_default(),
                }
            }
        };
        self.notice = None;
        self.label_prompt = Some(prompt);
    }

    fn label_prompt_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(prompt) = &mut self.label_prompt else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.label_prompt = None,
            KeyCode::Char('c') if ctrl => self.label_prompt = None,
            KeyCode::Char('u') if ctrl => prompt.text.clear(),
            KeyCode::Enter => self.save_label(),
            KeyCode::Backspace => {
                prompt.text.pop();
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                prompt.text.push(c);
            }
            _ => {}
        }
    }

    /// Writes the prompt's label to the labels file (an empty one removes
    /// the key's lines), then shows the file's labels at once and hands
    /// them to the watcher.
    ///
    /// A socket's UCSI port is found by carrying the same label, so a
    /// `typec:` line with the socket's old label gets the new one too.
    fn save_label(&mut self) {
        let Some(prompt) = self.label_prompt.take() else {
            return;
        };
        let Some(file) = &self.labels_file else {
            return;
        };
        let text = prompt.text.trim().to_owned();
        let mut edits = vec![(prompt.key.clone(), text.clone())];
        if let Key::Socket(socket) = &prompt.key
            && !text.is_empty()
        {
            let before = watch::current(&file.labels);
            if let Some(old) = before.socket_label(socket) {
                edits.extend(
                    before
                        .typec
                        .iter()
                        .filter(|(_, l)| *l == old)
                        .map(|(port, _)| (Key::Typec(port.clone()), text.clone())),
                );
            }
        }
        match labels::write(&file.path, &edits) {
            Ok(new) => {
                let shown = file.shown.clone();
                watch::replace(&file.labels, new.clone());
                self.relabel(&new);
                let text = if text.is_empty() {
                    format!("unlabelled {} in {shown}", prompt.what)
                } else {
                    format!("labelled {} \"{text}\" in {shown}", prompt.what)
                };
                self.notice = Some(Notice { text, error: false });
            }
            Err(e) => {
                self.notice = Some(Notice {
                    text: format!("can't write {}: {e}", file.shown),
                    error: true,
                });
            }
        }
    }

    /// Puts `labels` on what is shown (and held while paused), and checks
    /// health again: findings name sockets by label.
    fn relabel(&mut self, labels: &Labels) {
        let selected = self.selected_row().map(|r| r.id);
        if let Some(s) = &mut self.snapshot {
            collect::apply_labels(s, labels);
            self.findings = health::check(s);
        }
        if let Some(s) = &mut self.held.snapshot {
            collect::apply_labels(s, labels);
        }
        for g in &mut self.ghosts {
            collect::apply_device_labels(&mut g.device, labels);
        }
        self.reselect(selected);
    }

    fn overlay_action(&mut self, action: Action) {
        let half = (self.overlay_height / 2).max(1);
        let scroll = &mut self.overlay_scroll;
        match action {
            // The renderer clamps the scroll to the content.
            Action::Move(n) if n < 0 => *scroll = scroll.saturating_sub(n.unsigned_abs() as usize),
            Action::Move(n) => *scroll = scroll.saturating_add(n as usize),
            Action::HalfDown => *scroll = scroll.saturating_add(half),
            Action::HalfUp => *scroll = scroll.saturating_sub(half),
            Action::Top => *scroll = 0,
            Action::Bottom => *scroll = usize::MAX,
            Action::Back | Action::Close => self.overlay = None,
            Action::Help if self.overlay == Some(Overlay::Help) => self.overlay = None,
            Action::Help => self.open(Overlay::Help),
            Action::Quit => self.should_quit = true,
            _ => {}
        }
    }

    fn open(&mut self, overlay: Overlay) {
        self.overlay = Some(overlay);
        self.overlay_scroll = 0;
    }

    /// The selected row of the Ports tree.
    pub fn selected_row(&self) -> Option<ports::Row> {
        self.socket_rows()
            .into_iter()
            .nth(self.pane(Pane::Sockets).sel)
    }

    /// Details of the selected device, or of the socket a row belongs to.
    fn open_detail(&mut self) {
        if self.tab != Tab::Ports || self.focus() != Pane::Sockets {
            return;
        }
        let Some(row) = self.selected_row() else {
            return;
        };
        let target = match &row.kind {
            ports::RowKind::Device { device, .. } => Target::Device(device.sysfs_name.clone()),
            _ => Target::Socket(row.socket),
        };
        self.open(Overlay::Detail(target));
    }

    /// `n` (`forward`) / `N`: the next or previous match, wrapping.
    fn jump_match(&mut self, forward: bool) {
        match self.tab {
            Tab::Ports if !self.sockets_filter.is_empty() => {
                self.focus[Tab::Ports.index()] = Pane::Sockets;
                let matches: Vec<usize> = self
                    .socket_rows()
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.is_match())
                    .map(|(i, _)| i)
                    .collect();
                let sel = self.pane(Pane::Sockets).sel;
                let to = if forward {
                    matches.iter().find(|&&i| i > sel).or(matches.first())
                } else {
                    matches.iter().rev().find(|&&i| i < sel).or(matches.last())
                };
                if let Some(&i) = to {
                    self.pane_mut(Pane::Sockets).sel = i;
                }
            }
            Tab::Events if !self.events_filter.is_empty() => {
                // Every line shown matches: step one, wrapping.
                let len = self.visible_events().len();
                if len == 0 {
                    return;
                }
                let sel = self.pane(Pane::Events).sel;
                let to = if forward {
                    (sel + 1) % len
                } else {
                    (sel + len - 1) % len
                };
                self.pane_mut(Pane::Events).sel = to;
                self.events_follow = to + 1 >= len;
            }
            _ => {}
        }
    }

    /// The events the Events tab lists: those whose line contains the
    /// filter, ignoring case.
    pub fn visible_events(&self) -> Vec<&Event> {
        let needle = self.events_filter.to_lowercase();
        self.events
            .iter()
            .filter(|e| needle.is_empty() || e.line().to_lowercase().contains(&needle))
            .collect()
    }

    /// Ghosts still shown at the display's time.
    pub fn live_ghosts(&self) -> Vec<&Ghost> {
        let now = self.view_now_ms();
        self.ghosts.iter().filter(|g| g.live(now)).collect()
    }

    /// The keys of an unfinished sequence (`5`, `g`, `^W`, `:q`).
    pub fn pending_keys(&self) -> String {
        self.keys.text()
    }

    /// The time the display shows: frozen while paused.
    pub fn view_now_ms(&self) -> u64 {
        if self.paused {
            self.paused_at_ms
        } else {
            self.now_ms
        }
    }

    pub fn narrow(&self) -> bool {
        self.size.0 < NARROW_COLS
    }

    pub fn focus(&self) -> Pane {
        self.focus[self.tab.index()]
    }

    pub fn pane(&self, pane: Pane) -> PaneState {
        self.panes[pane as usize]
    }

    pub fn pane_mut(&mut self, pane: Pane) -> &mut PaneState {
        &mut self.panes[pane as usize]
    }

    /// The Ports tree as currently expanded, filtered, with live ghosts.
    pub fn socket_rows(&self) -> Vec<ports::Row> {
        match &self.snapshot {
            Some(s) => ports::rows(
                s,
                TreeOpts {
                    collapsed: &self.collapsed,
                    ghosts: &self.live_ghosts(),
                    filter: &self.sockets_filter,
                },
            ),
            None => Vec::new(),
        }
    }

    /// Puts the Sockets selection back on the row `id` if it is still
    /// there, else keeps it in range.
    fn reselect(&mut self, id: Option<String>) {
        let rows = self.socket_rows();
        let state = self.pane_mut(Pane::Sockets);
        if let Some(i) = id.and_then(|id| rows.iter().position(|r| r.id == id)) {
            state.sel = i;
        }
        state.sel = state.sel.min(rows.len().saturating_sub(1));
    }

    /// The new snapshot; subtrees it lost on a socket a Removed event names
    /// become ghosts, and ghosts whose device is back go.
    fn apply_snapshot(&mut self, snapshot: Snapshot, events: &[Event]) {
        let selected = self.selected_row().map(|r| r.id);
        if let Some(old) = &self.snapshot {
            let new = ghosts::removed(old, &snapshot, events);
            self.ghosts.extend(new);
        }
        let now = snapshot.taken_at_ms;
        self.ghosts
            .retain(|g| g.live(now) && !ghosts::returned(g, &snapshot));
        self.findings = health::check(&snapshot);
        self.snapshot = Some(snapshot);
        self.reselect(selected);
    }

    fn push_events(&mut self, events: Vec<Event>) {
        self.events.extend(events);
        let extra = self.events.len().saturating_sub(EVENTS_MAX);
        self.events.drain(..extra);
        let len = self.visible_events().len();
        let state = self.pane_mut(Pane::Events);
        state.sel = state.sel.saturating_sub(extra);
        if self.events_follow {
            self.pane_mut(Pane::Events).sel = len.saturating_sub(1);
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Move(n) => self.move_by(n),
            Action::Top => self.move_to(0),
            Action::Bottom => self.move_to(usize::MAX),
            Action::HalfDown => self.move_by(self.half_page()),
            Action::HalfUp => self.move_by(-self.half_page()),
            Action::Left => {
                if !self.set_collapsed(true) {
                    self.focus_dir(Dir::Left);
                }
            }
            Action::Right => {
                if !self.set_collapsed(false) {
                    self.focus_dir(Dir::Right);
                }
            }
            Action::Focus(dir) => self.focus_dir(dir),
            Action::NextTab => self.tab = Tab::ALL[(self.tab.index() + 1) % 3],
            Action::PrevTab => self.tab = Tab::ALL[(self.tab.index() + 2) % 3],
            Action::GoTab(i) => self.tab = Tab::ALL[i.min(2)],
            // A replay pauses playback, not the view: see `replay`.
            Action::Pause if self.replay.is_some() => {
                self.replay_request(ReplayRequest::TogglePause)
            }
            Action::Pause => self.toggle_pause(),
            Action::Faster => self.replay_request(ReplayRequest::Faster),
            Action::Slower => self.replay_request(ReplayRequest::Slower),
            Action::NextPlug => self.replay_request(ReplayRequest::NextPlug),
            Action::PrevPlug => self.replay_request(ReplayRequest::PrevPlug),
            Action::ClearEvents => {
                self.events.clear();
                *self.pane_mut(Pane::Events) = PaneState {
                    height: self.pane(Pane::Events).height,
                    ..PaneState::default()
                };
                self.events_follow = true;
            }
            Action::Open => self.open_detail(),
            Action::Search => {
                if self.filter().is_some() {
                    if self.tab == Tab::Ports {
                        self.focus[Tab::Ports.index()] = Pane::Sockets;
                    }
                    self.set_filter(String::clear);
                    self.prompt = true;
                }
            }
            Action::NextMatch => self.jump_match(true),
            Action::PrevMatch => self.jump_match(false),
            Action::Help => self.open(Overlay::Help),
            Action::Back => {
                if self.filter().is_some_and(|f| !f.is_empty()) {
                    self.set_filter(String::clear);
                }
            }
            Action::Close | Action::Quit => self.should_quit = true,
            Action::Label if self.replay.is_some() => {
                self.notice = Some(Notice {
                    text: "labels can't be edited in a replay".into(),
                    error: true,
                });
            }
            Action::Label => self.open_label_prompt(),
            Action::None => {}
        }
    }

    /// Hands a replay key to the replay; live, replay keys do nothing.
    fn replay_request(&mut self, request: ReplayRequest) {
        if let Some(r) = &mut self.replay {
            r.request = Some(request);
        }
    }

    fn toggle_pause(&mut self) {
        if !self.paused {
            self.paused = true;
            self.paused_at_ms = self.now_ms;
            return;
        }
        self.paused = false;
        let held = std::mem::take(&mut self.held);
        if let Some(stats) = held.stats {
            self.stats = stats;
        }
        if let Some(snapshot) = held.snapshot {
            self.apply_snapshot(snapshot, &held.events);
        }
        self.push_events(held.events);
        for sample in held.samples {
            self.history.push(sample);
        }
    }

    fn half_page(&self) -> i64 {
        (self.pane(self.focus()).height / 2).max(1) as i64
    }

    /// Rows in a pane with a selection; `None` for panes that only scroll.
    fn selectable_len(&self, pane: Pane) -> Option<usize> {
        match pane {
            Pane::Sockets => Some(self.socket_rows().len()),
            Pane::Events => Some(self.visible_events().len()),
            _ => None,
        }
    }

    fn move_by(&mut self, n: i64) {
        let pane = self.focus();
        let state = self.pane(pane);
        let from = match self.selectable_len(pane) {
            Some(_) => state.sel,
            None => state.offset,
        };
        let to = if n < 0 {
            from.saturating_sub(n.unsigned_abs() as usize)
        } else {
            from.saturating_add(n as usize)
        };
        self.move_to(to);
    }

    fn move_to(&mut self, to: usize) {
        let pane = self.focus();
        match pane {
            Pane::PowerChart | Pane::CpuChart => {}
            Pane::UsbC | Pane::Power | Pane::Health | Pane::Links | Pane::Urbs | Pane::Chargers => {
                // The renderer clamps it to the content.
                self.pane_mut(pane).offset = to;
            }
            Pane::Sockets | Pane::Events => {
                let len = self.selectable_len(pane).unwrap_or(0);
                let sel = to.min(len.saturating_sub(1));
                self.pane_mut(pane).sel = sel;
                if pane == Pane::Events {
                    self.events_follow = sel + 1 >= len;
                }
            }
        }
    }

    /// Collapses (or expands) the selected hub in the Ports tree. `false`
    /// when the selection isn't a hub with devices below it.
    fn set_collapsed(&mut self, collapse: bool) -> bool {
        if self.focus() != Pane::Sockets {
            return false;
        }
        let rows = self.socket_rows();
        let Some(name) = rows
            .get(self.pane(Pane::Sockets).sel)
            .and_then(|r| r.hub_name())
        else {
            return false;
        };
        if collapse {
            self.collapsed.insert(name.to_owned());
        } else {
            self.collapsed.remove(name);
        }
        true
    }

    /// The tab's panes as columns, left to right, each top to bottom.
    fn columns(&self) -> Vec<Vec<Pane>> {
        match self.tab {
            Tab::Ports if self.narrow() => {
                vec![vec![Pane::Sockets, Pane::UsbC, Pane::Power, Pane::Health]]
            }
            Tab::Ports => vec![
                vec![Pane::Sockets],
                vec![Pane::UsbC, Pane::Power, Pane::Health],
            ],
            Tab::Charts => vec![vec![
                Pane::PowerChart,
                Pane::CpuChart,
                Pane::Links,
                Pane::Urbs,
                Pane::Chargers,
            ]],
            Tab::Events => vec![vec![Pane::Events]],
        }
    }

    fn focus_dir(&mut self, dir: Dir) {
        let columns = self.columns();
        let current = self.focus();
        let Some((col, row)) = columns
            .iter()
            .enumerate()
            .find_map(|(c, panes)| Some((c, panes.iter().position(|&p| p == current)?)))
        else {
            return;
        };
        let target = match dir {
            Dir::Up => row.checked_sub(1).map(|r| columns[col][r]),
            Dir::Down => columns[col].get(row + 1).copied(),
            Dir::Left => col.checked_sub(1).map(|c| {
                let panes = &columns[c];
                panes[row.min(panes.len() - 1)]
            }),
            Dir::Right => columns.get(col + 1).map(|panes| panes[0]),
        };
        if let Some(pane) = target {
            self.focus[self.tab.index()] = pane;
        }
    }
}

/// `HH:MM:SS` for a Unix time in ms, shifted by `utc_offset_s`.
pub fn clock(t_ms: u64, utc_offset_s: i32) -> String {
    let secs = (t_ms / 1000) as i64 + i64::from(utc_offset_s);
    let day = secs.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", day / 3600, day % 3600 / 60, day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_times() {
        assert_eq!(clock(0, 0), "00:00:00");
        assert_eq!(clock(3_723_999, 0), "01:02:03");
        assert_eq!(clock(0, -3600), "23:00:00");
        assert_eq!(clock(0, 2 * 3600), "02:00:00");
    }
}
