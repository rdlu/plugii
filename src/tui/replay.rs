//! `plugii replay`: a recording fed into the same [`App`] as live, through
//! the same path (`diff::diff`, `Sample::from_snapshot`, `App::on_update`),
//! with the App's clock set to the recording's time so charts, flashes,
//! ghosts and event times show the recorded ones. [`Player`] keeps the
//! timing; [`Replay`] wires it to the App; neither needs a terminal.
//!
//! The replay's pause is the player's, not the App's: the App's pause holds
//! updates back while time goes on, which is what a live view needs, while a
//! paused replay simply stops feeding. The App never pauses in a replay.

use std::ops::Range;
use std::time::Duration;

use crossterm::event::KeyEvent;

use super::App;
use super::app::{Notice, ReplayRequest, ReplayView};
use crate::diff::{self, Event, EventKind};
use crate::history::Sample;
use crate::model::Snapshot;
use crate::record::Recording;

/// The playback speeds `+` and `-` step through.
pub const SPEEDS: [u32; 6] = [1, 2, 4, 10, 30, 60];

/// Replay timing, driven by real elapsed time × speed. Positions are in the
/// recording's time (Unix ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// Each snapshot's time, made non-decreasing.
    times: Vec<u64>,
    /// Snapshots fed so far: the current one is `fed - 1`.
    fed: usize,
    /// Where playback is, in µs of recording time.
    clock_us: u64,
    speed: usize,
    paused: bool,
    ended: bool,
}

impl Player {
    /// At the first snapshot, due at once. `speed` snaps to the nearest of
    /// [`SPEEDS`] at or below it.
    ///
    /// # Panics
    ///
    /// If `times` is empty.
    pub fn new(times: &[u64], speed: u32) -> Self {
        assert!(!times.is_empty(), "a replay needs a snapshot");
        let mut max = 0;
        let times: Vec<u64> = times
            .iter()
            .map(|&t| {
                max = max.max(t);
                max
            })
            .collect();
        Self {
            clock_us: times[0] * 1000,
            times,
            fed: 0,
            speed: SPEEDS.iter().rposition(|&s| s <= speed).unwrap_or(0),
            paused: false,
            ended: false,
        }
    }

    /// Moves the clock on by `elapsed` × speed unless paused; returns the
    /// snapshots now due. Reaching the last one pauses at the end.
    pub fn advance(&mut self, elapsed: Duration) -> Range<usize> {
        if !self.paused {
            let us = elapsed.as_micros() as u64 * u64::from(self.speed());
            self.clock_us = self.clock_us.saturating_add(us);
        }
        let from = self.fed;
        let clock_ms = self.clock_us / 1000;
        self.fed += self.times[from..].partition_point(|&t| t <= clock_ms);
        if self.fed == self.times.len() {
            self.paused = true;
            self.ended = true;
            self.clock_us = self.end_ms() * 1000;
        }
        from..self.fed
    }

    /// Real time until the next snapshot is due; `None` while paused.
    pub fn until_next(&self) -> Option<Duration> {
        if self.paused {
            return None;
        }
        let next_us = self.times.get(self.fed)? * 1000;
        let wait = next_us.saturating_sub(self.clock_us) / u64::from(self.speed());
        Some(Duration::from_micros(wait))
    }

    /// Pauses or resumes; at the end it stays paused (see [`Replay`] for
    /// starting over).
    pub fn toggle_pause(&mut self) {
        if !self.ended {
            self.paused = !self.paused;
        }
    }

    pub fn faster(&mut self) {
        self.speed = (self.speed + 1).min(SPEEDS.len() - 1);
    }

    pub fn slower(&mut self) {
        self.speed = self.speed.saturating_sub(1);
    }

    /// Puts playback on snapshot `index`, as just fed; paused or playing as
    /// before.
    pub fn seek(&mut self, index: usize) {
        let index = index.min(self.times.len() - 1);
        self.fed = index + 1;
        self.clock_us = self.times[index] * 1000;
        self.ended = self.fed == self.times.len();
        if self.ended {
            self.paused = true;
        }
    }

    /// The snapshot shown, once one is fed.
    pub fn current(&self) -> Option<usize> {
        self.fed.checked_sub(1)
    }

    pub fn len(&self) -> usize {
        self.times.len()
    }

    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    pub fn speed(&self) -> u32 {
        SPEEDS[self.speed]
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn position_ms(&self) -> u64 {
        self.clock_us / 1000
    }

    pub fn start_ms(&self) -> u64 {
        self.times[0]
    }

    pub fn end_ms(&self) -> u64 {
        self.times[self.times.len() - 1]
    }
}

/// A recording playing into an [`App`].
#[derive(Debug)]
pub struct Replay {
    pub app: App,
    pub player: Player,
    snapshots: Vec<Snapshot>,
    /// The events each snapshot brings, as live would diff them.
    events: Vec<Vec<Event>>,
    /// Snapshots with an Added or Removed event: where `]` / `[` stop.
    plugs: Vec<usize>,
}

impl Replay {
    /// `file` is the recording's name for the status line. The first
    /// snapshot is fed at once. `None` when the recording has no snapshots.
    pub fn new(mut app: App, recording: Recording, file: String, speed: u32) -> Option<Self> {
        let snapshots = recording.snapshots;
        if snapshots.is_empty() {
            return None;
        }
        let times: Vec<u64> = snapshots.iter().map(|s| s.taken_at_ms).collect();
        let player = Player::new(&times, speed);
        let events: Vec<Vec<Event>> = (0..snapshots.len())
            .map(|i| match i {
                0 => Vec::new(),
                i => diff::diff(&snapshots[i - 1], &snapshots[i]),
            })
            .collect();
        let plugs = events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.iter()
                    .any(|e| matches!(e.kind, EventKind::Added | EventKind::Removed))
            })
            .map(|(i, _)| i)
            .collect();
        app.labels_file = None;
        app.warning = None;
        app.replay = Some(ReplayView {
            file,
            host: recording.header.host,
            start_ms: player.start_ms(),
            end_ms: player.end_ms(),
            at_ms: player.start_ms(),
            speed: player.speed(),
            paused: false,
            ended: false,
            request: None,
        });
        let mut replay = Self {
            app,
            player,
            snapshots,
            events,
            plugs,
        };
        replay.tick(Duration::ZERO);
        Some(replay)
    }

    /// Real time `elapsed` passed: feeds what came due.
    pub fn tick(&mut self, elapsed: Duration) {
        let due = self.player.advance(elapsed);
        self.feed(due);
        self.sync();
    }

    /// A key: through the App's keymap, then the replay key it asked for.
    pub fn on_key(&mut self, key: KeyEvent) {
        self.app.on_key(key);
        let request = self.app.replay.as_mut().and_then(|r| r.request.take());
        match request {
            Some(ReplayRequest::TogglePause) if self.player.ended() => {
                // `p` at the end plays it again from the start.
                self.seek(0);
                self.player.toggle_pause();
            }
            Some(ReplayRequest::TogglePause) => self.player.toggle_pause(),
            Some(ReplayRequest::Faster) => self.player.faster(),
            Some(ReplayRequest::Slower) => self.player.slower(),
            Some(ReplayRequest::NextPlug) => {
                let at = self.player.current().unwrap_or(0);
                match self.plugs.iter().find(|&&i| i > at) {
                    Some(&i) => self.seek(i),
                    None => self.notice("no later plug or unplug"),
                }
            }
            Some(ReplayRequest::PrevPlug) => {
                let at = self.player.current().unwrap_or(0);
                match self.plugs.iter().rev().find(|&&i| i < at) {
                    Some(&i) => self.seek(i),
                    None => self.notice("no earlier plug or unplug"),
                }
            }
            None => {}
        }
        self.sync();
    }

    /// Jumps to snapshot `index`: on from here, or rebuilt from the start
    /// when it is behind, fed without drawing either way. The Events pane
    /// follows again, so the jump's events are selected.
    pub fn seek(&mut self, index: usize) {
        let index = index.min(self.snapshots.len() - 1);
        let from = match self.player.current() {
            Some(at) if at < index => at + 1,
            _ => {
                self.app.clear_data();
                0
            }
        };
        self.app.events_follow = true;
        self.player.seek(index);
        self.feed(from..index + 1);
        self.sync();
    }

    fn feed(&mut self, range: Range<usize>) {
        for i in range {
            let snapshot = self.snapshots[i].clone();
            self.app.now_ms = snapshot.taken_at_ms;
            let sample = Sample::from_snapshot(&snapshot);
            self.app.on_update(snapshot, self.events[i].clone(), sample);
        }
    }

    fn notice(&mut self, text: &str) {
        self.app.notice = Some(Notice {
            text: text.into(),
            error: false,
        });
    }

    /// The App's clock and status line from the player.
    fn sync(&mut self) {
        let p = &self.player;
        self.app.now_ms = p.position_ms();
        if let Some(view) = &mut self.app.replay {
            view.at_ms = p.position_ms();
            view.speed = p.speed();
            view.paused = p.paused();
            view.ended = p.ended();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// Snapshots at 0, 1, 2, 5 and 10 s.
    fn player(speed: u32) -> Player {
        Player::new(&[0, 1000, 2000, 5000, 10_000], speed)
    }

    #[test]
    fn plays_in_real_time_at_x1() {
        let mut p = player(1);
        assert_eq!(p.advance(Duration::ZERO), 0..1);
        assert_eq!(p.until_next(), Some(s(1)));
        assert_eq!(p.advance(Duration::from_millis(999)), 1..1);
        assert_eq!(p.advance(Duration::from_millis(1)), 1..2);
        assert_eq!(p.advance(s(3)), 2..3);
        assert_eq!(p.position_ms(), 4000);
        assert_eq!(p.until_next(), Some(s(1)));
    }

    #[test]
    fn speed_scales_time() {
        let mut p = player(4);
        p.advance(Duration::ZERO);
        assert_eq!(p.until_next(), Some(Duration::from_millis(250)));
        assert_eq!(p.advance(Duration::from_millis(1250)), 1..4);
        assert_eq!(p.position_ms(), 5000);
    }

    #[test]
    fn speeds_step_and_stop_at_the_ends() {
        let mut p = player(1);
        p.slower();
        assert_eq!(p.speed(), 1);
        let seen: Vec<u32> = (0..7)
            .map(|_| {
                p.faster();
                p.speed()
            })
            .collect();
        assert_eq!(seen, [2, 4, 10, 30, 60, 60, 60]);
        p.slower();
        assert_eq!(p.speed(), 30);
        // An off-step speed snaps down.
        assert_eq!(player(20).speed(), 10);
        assert_eq!(player(0).speed(), 1);
    }

    #[test]
    fn pause_stops_the_clock() {
        let mut p = player(1);
        p.advance(Duration::ZERO);
        p.toggle_pause();
        assert!(p.paused());
        assert_eq!(p.advance(s(5)), 1..1);
        assert_eq!(p.position_ms(), 0);
        assert_eq!(p.until_next(), None);
        p.toggle_pause();
        assert_eq!(p.advance(s(1)), 1..2);
    }

    #[test]
    fn the_end_pauses() {
        let mut p = player(60);
        assert_eq!(p.advance(s(60)), 0..5);
        assert!(p.ended());
        assert!(p.paused());
        assert_eq!(p.position_ms(), 10_000);
        p.toggle_pause();
        assert!(p.paused(), "stays paused at the end");
        assert_eq!(p.advance(s(1)), 5..5);
    }

    #[test]
    fn seeking() {
        let mut p = player(1);
        p.advance(Duration::ZERO);
        p.seek(3);
        assert_eq!(p.current(), Some(3));
        assert_eq!(p.position_ms(), 5000);
        assert_eq!(p.advance(s(5)), 4..5);
        assert!(p.ended());
        p.seek(1);
        assert!(!p.ended());
        assert!(p.paused(), "a seek from the end stays paused");
        assert_eq!(p.current(), Some(1));
        p.toggle_pause();
        assert_eq!(p.advance(s(1)), 2..3);
    }

    #[test]
    fn clock_jumps_back_are_flattened() {
        let mut p = Player::new(&[5000, 4000, 6000], 1);
        assert_eq!(p.advance(Duration::ZERO), 0..2);
        assert_eq!(p.until_next(), Some(s(1)));
    }
}
