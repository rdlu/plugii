//! Live updates for the TUI: a thread that rescans the machine on udev
//! events (debounced) and on a 1 s tick, sending each snapshot down a
//! channel.

use std::os::fd::AsRawFd;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, PoisonError, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::collect::{self, CollectOpts};
use crate::labels::Labels;
use crate::model::Snapshot;
use crate::sysfs::{Root, cpu};

/// What the watcher sends.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// A fresh scan. `cpu` is filled from the counters since the previous
    /// scan (no sampling sleep); `kernel_log` is refreshed on udev-triggered
    /// scans and every few seconds, and otherwise carried over.
    Snapshot(Box<Snapshot>),
    /// The watcher can't watch (no udev, say); scans continue on the tick
    /// when possible. Shown once in the TUI's status line.
    Warning(String),
}

/// The subsystems whose udev events trigger a rescan.
pub const SUBSYSTEMS: &[&str] = &["usb", "typec", "power_supply", "drm", "thunderbolt"];
/// Time between plain rescans.
pub const TICK: Duration = Duration::from_secs(1);
/// Quiet after a udev event before rescanning: a plug comes as a burst.
pub const DEBOUNCE_QUIET: Duration = Duration::from_millis(150);
/// The longest a burst of udev events can hold a rescan back.
pub const DEBOUNCE_MAX: Duration = Duration::from_millis(500);
/// How often the tick refreshes the kernel log.
pub const KERNEL_LOG_EVERY: Duration = Duration::from_secs(10);

/// The labels the watcher scans with, shared with the TUI: the label wizard
/// writes new ones in, and the next scan uses them.
pub type SharedLabels = Arc<RwLock<Labels>>;

/// A copy of the shared labels; a writer that panicked left whole labels
/// behind, so poisoning is ignored.
pub fn current(labels: &SharedLabels) -> Labels {
    labels
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Puts `new` in place of the shared labels.
pub fn replace(labels: &SharedLabels, new: Labels) {
    *labels.write().unwrap_or_else(PoisonError::into_inner) = new;
}

/// Starts the watcher thread. The first snapshot is sent right away; each
/// scan reads `labels` afresh. The thread ends when the receiver is dropped.
pub fn spawn(root: Root, labels: SharedLabels) -> Receiver<Update> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("plugii-watch".into())
        .spawn(move || run(&root, &labels, &tx))
        .expect("spawning the watcher thread");
    rx
}

/// What kind of rescan to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scan {
    /// Run journalctl; otherwise carry the last kernel log over.
    pub kernel_log: bool,
    /// Ask niri for display modes; otherwise carry them over.
    pub niri: bool,
}

/// What the watcher should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Wait for udev events up to this long.
    Wait(Duration),
    Scan(Scan),
}

/// The watcher's timing, apart from udev so it can be tested: udev events
/// go in through [`Schedule::event`], [`Schedule::next`] says what to do.
#[derive(Debug, Clone)]
pub struct Schedule {
    next_tick: Instant,
    last_kernel_log: Instant,
    /// First and latest udev event since the last udev rescan.
    burst: Option<(Instant, Instant)>,
}

impl Schedule {
    /// Right after the first scan (which read the kernel log) at `now`.
    pub fn new(now: Instant) -> Self {
        Self {
            next_tick: now + TICK,
            last_kernel_log: now,
            burst: None,
        }
    }

    /// A udev event arrived at `now`.
    pub fn event(&mut self, now: Instant) {
        self.burst = Some(match self.burst {
            Some((first, _)) => (first, now),
            None => (now, now),
        });
    }

    /// The next step at `now`. A burst of udev events wins over the tick: it
    /// rescans after [`DEBOUNCE_QUIET`] of quiet or [`DEBOUNCE_MAX`] after
    /// its first event, whichever is sooner, with the kernel log and niri,
    /// and restarts the tick. Ticks read the kernel log every
    /// [`KERNEL_LOG_EVERY`].
    pub fn next(&mut self, now: Instant) -> Step {
        let due = match self.burst {
            Some((first, last)) => {
                let due = (last + DEBOUNCE_QUIET).min(first + DEBOUNCE_MAX);
                if due <= now {
                    self.burst = None;
                    self.next_tick = now + TICK;
                    self.last_kernel_log = now;
                    return Step::Scan(Scan {
                        kernel_log: true,
                        niri: true,
                    });
                }
                due
            }
            None => {
                if self.next_tick <= now {
                    // Ticks don't pile up after a stall.
                    self.next_tick = (self.next_tick + TICK).max(now);
                    let kernel_log = now.duration_since(self.last_kernel_log) >= KERNEL_LOG_EVERY;
                    if kernel_log {
                        self.last_kernel_log = now;
                    }
                    return Step::Scan(Scan {
                        kernel_log,
                        niri: false,
                    });
                }
                self.next_tick
            }
        };
        Step::Wait(due - now)
    }
}

/// The udev monitor on [`SUBSYSTEMS`].
fn monitor() -> std::io::Result<udev::MonitorSocket> {
    let mut builder = udev::MonitorBuilder::new()?;
    for subsystem in SUBSYSTEMS {
        builder = builder.match_subsystem(subsystem)?;
    }
    builder.listen()
}

/// Whether `fd` became readable within `timeout`.
fn poll_readable(fd: i32, timeout: Duration) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = libc::c_int::try_from(timeout.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
    // SAFETY: one valid pollfd, and its count.
    let n = unsafe { libc::poll(&mut pfd, 1, ms) };
    n > 0 && pfd.revents & libc::POLLIN != 0
}

fn run(root: &Root, labels: &SharedLabels, tx: &Sender<Update>) {
    let mut last = collect::snapshot(
        root,
        &current(labels),
        &CollectOpts {
            cpu_window: None,
            kernel_log: true,
            niri: true,
        },
    );
    let mut counters = cpu::read_counters(root).map(|c| (c, Instant::now()));
    if tx.send(Update::Snapshot(Box::new(last.clone()))).is_err() {
        return;
    }

    let socket = match monitor() {
        Ok(socket) => Some(socket),
        Err(e) => {
            let warning = format!("no udev events ({e}); rescanning every second");
            if tx.send(Update::Warning(warning)).is_err() {
                return;
            }
            None
        }
    };

    let mut schedule = Schedule::new(Instant::now());
    loop {
        match schedule.next(Instant::now()) {
            Step::Wait(timeout) => match &socket {
                Some(socket) => {
                    if poll_readable(socket.as_raw_fd(), timeout) {
                        // Non-blocking: drains what has arrived.
                        if socket.iter().count() > 0 {
                            schedule.event(Instant::now());
                        }
                    }
                }
                None => thread::sleep(timeout),
            },
            Step::Scan(scan) => {
                last = rescan(root, &current(labels), &last, scan, &mut counters);
                if tx.send(Update::Snapshot(Box::new(last.clone()))).is_err() {
                    return;
                }
            }
        }
    }
}

/// A scan after `prev`: the CPU from the counters since the last scan, and
/// whatever `scan` skips carried over from `prev`.
fn rescan(
    root: &Root,
    labels: &Labels,
    prev: &Snapshot,
    scan: Scan,
    counters: &mut Option<(cpu::CpuCounters, Instant)>,
) -> Snapshot {
    let opts = CollectOpts {
        cpu_window: None,
        kernel_log: scan.kernel_log,
        niri: scan.niri,
    };
    let mut snap = collect::snapshot(root, labels, &opts);
    if !scan.kernel_log {
        snap.kernel_log = prev.kernel_log.clone();
    }
    if !scan.niri {
        carry_display_modes(prev, &mut snap);
    }
    let now = cpu::read_counters(root).map(|c| (c, Instant::now()));
    if let (Some((before, t0)), Some((after, t1))) = (counters.as_ref(), now.as_ref()) {
        snap.cpu = Some(cpu::sample(root, before, after, t1.duration_since(*t0)));
    }
    *counters = now;
    snap
}

/// niri's mode and VRR for each display still connected.
pub fn carry_display_modes(prev: &Snapshot, snap: &mut Snapshot) {
    for display in &mut snap.displays {
        if let Some(old) = prev
            .displays
            .iter()
            .find(|d| d.connector == display.connector)
        {
            display.mode = display.mode.take().or_else(|| old.mode.clone());
            display.vrr = display.vrr.or(old.vrr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG_AND_NIRI: Step = Step::Scan(Scan {
        kernel_log: true,
        niri: true,
    });
    const TICK_ONLY: Step = Step::Scan(Scan {
        kernel_log: false,
        niri: false,
    });

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ticks_every_second() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        assert_eq!(s.next(t0), Step::Wait(TICK));
        assert_eq!(s.next(t0 + ms(400)), Step::Wait(ms(600)));
        assert_eq!(s.next(t0 + ms(1000)), TICK_ONLY);
        assert_eq!(s.next(t0 + ms(1000)), Step::Wait(TICK));
        // Late: the next tick stays on the grid.
        assert_eq!(s.next(t0 + ms(2100)), TICK_ONLY);
        assert_eq!(s.next(t0 + ms(2100)), Step::Wait(ms(900)));
    }

    #[test]
    fn ticks_read_the_kernel_log_every_ten_seconds() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        let logs: Vec<bool> = (1..=20)
            .map(|i| match s.next(t0 + ms(i * 1000)) {
                Step::Scan(scan) => scan.kernel_log,
                Step::Wait(_) => panic!("tick {i} not due"),
            })
            .collect();
        let at: Vec<usize> = (1..)
            .zip(logs)
            .filter(|(_, l)| *l)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(at, [10, 20]);
    }

    #[test]
    fn udev_burst_waits_for_quiet() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        s.event(t0 + ms(100));
        assert_eq!(s.next(t0 + ms(100)), Step::Wait(ms(150)));
        s.event(t0 + ms(200));
        assert_eq!(s.next(t0 + ms(300)), Step::Wait(ms(50)));
        assert_eq!(s.next(t0 + ms(350)), LOG_AND_NIRI);
        // The tick restarts from the udev scan.
        assert_eq!(s.next(t0 + ms(350)), Step::Wait(TICK));
    }

    #[test]
    fn udev_burst_scans_after_half_a_second_at_most() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        for t in (0..=450).step_by(50) {
            s.event(t0 + ms(t));
            assert!(matches!(s.next(t0 + ms(t)), Step::Wait(_)), "{t}");
        }
        assert_eq!(s.next(t0 + ms(480)), Step::Wait(ms(20)));
        assert_eq!(s.next(t0 + ms(500)), LOG_AND_NIRI);
    }

    #[test]
    fn udev_burst_holds_the_tick_back() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        s.event(t0 + ms(950));
        assert_eq!(s.next(t0 + ms(1000)), Step::Wait(ms(100)));
        assert_eq!(s.next(t0 + ms(1100)), LOG_AND_NIRI);
        assert_eq!(s.next(t0 + ms(1100)), Step::Wait(TICK));
    }
}
