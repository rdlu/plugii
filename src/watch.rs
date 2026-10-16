//! Live updates for the TUI: a thread that rescans the machine on udev
//! events (debounced) and on a 1 s tick, sending each snapshot down a
//! channel.

use std::sync::mpsc::Receiver;

use crate::labels::Labels;
use crate::model::Snapshot;
use crate::sysfs::Root;

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

/// Starts the watcher thread. The first snapshot is sent right away. The
/// thread ends when the receiver is dropped.
pub fn spawn(root: Root, labels: Labels) -> Receiver<Update> {
    let _ = (root, labels);
    todo!("milestone 5: engine")
}
