//! The TUI's state, with no terminal: snapshots, events and samples come in
//! through [`App::on_update`], keys through [`App::on_key`], and the
//! renderer reads the rest.

use std::collections::{HashSet, VecDeque};

use crossterm::event::KeyEvent;

use super::keys::{Action, Dir, Pending};
use super::ports;
use crate::diff::Event;
use crate::health;
use crate::history::{History, Sample};
use crate::model::{Finding, Snapshot};

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
    Events,
}

const PANES: usize = 7;

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

/// Updates held back while paused, applied on resume so the event list and
/// the chart history have no gap.
#[derive(Debug, Default)]
struct Held {
    snapshot: Option<Snapshot>,
    events: Vec<Event>,
    samples: Vec<Sample>,
}

#[derive(Debug)]
pub struct App {
    pub snapshot: Option<Snapshot>,
    pub findings: Vec<Finding>,
    pub history: History<Sample>,
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
}

impl App {
    pub fn new(utc_offset_s: i32) -> Self {
        Self {
            snapshot: None,
            findings: Vec::new(),
            history: History::new(HISTORY_LEN),
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
        }
    }

    /// A new scan, the events since the previous one, and its chart sample.
    /// While paused these are held and applied on resume.
    pub fn on_update(&mut self, snapshot: Snapshot, events: Vec<Event>, sample: Sample) {
        if self.paused {
            self.held.snapshot = Some(snapshot);
            self.held.events.extend(events);
            let extra = self.held.events.len().saturating_sub(EVENTS_MAX);
            self.held.events.drain(..extra);
            self.held.samples.push(sample);
            let extra = self.held.samples.len().saturating_sub(HISTORY_LEN);
            self.held.samples.drain(..extra);
            return;
        }
        self.apply_snapshot(snapshot);
        self.push_events(events);
        self.history.push(sample);
    }

    /// The watcher's warning, shown in the status line.
    pub fn on_warning(&mut self, warning: String) {
        self.warning = Some(warning);
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        let action = self.keys.feed(key);
        self.apply(action);
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

    /// The Ports tree as currently expanded.
    pub fn socket_rows(&self) -> Vec<ports::Row> {
        match &self.snapshot {
            Some(s) => ports::rows(s, &self.collapsed),
            None => Vec::new(),
        }
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let rows = self.socket_rows();
        let selected = rows.get(self.pane(Pane::Sockets).sel).map(|r| r.id.clone());
        self.findings = health::check(&snapshot);
        self.snapshot = Some(snapshot);
        let rows = self.socket_rows();
        let state = self.pane_mut(Pane::Sockets);
        if let Some(i) = selected.and_then(|id| rows.iter().position(|r| r.id == id)) {
            state.sel = i;
        }
        state.sel = state.sel.min(rows.len().saturating_sub(1));
    }

    fn push_events(&mut self, events: Vec<Event>) {
        self.events.extend(events);
        let extra = self.events.len().saturating_sub(EVENTS_MAX);
        self.events.drain(..extra);
        let len = self.events.len();
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
            Action::Pause => self.toggle_pause(),
            Action::ClearEvents => {
                self.events.clear();
                *self.pane_mut(Pane::Events) = PaneState {
                    height: self.pane(Pane::Events).height,
                    ..PaneState::default()
                };
                self.events_follow = true;
            }
            Action::Quit => self.should_quit = true,
            Action::NotYet | Action::None => {}
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
        if let Some(snapshot) = held.snapshot {
            self.apply_snapshot(snapshot);
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
            Pane::Events => Some(self.events.len()),
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
            Pane::UsbC | Pane::Power | Pane::Health => {
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
            Tab::Charts => vec![vec![Pane::PowerChart, Pane::CpuChart]],
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
