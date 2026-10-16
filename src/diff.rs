//! What changed between two snapshots, as events for the TUI's Events tab
//! and chart markers: `+ 0bda:5411 hub @10G on USB-C (left)`.

use serde::{Deserialize, Serialize};

use crate::model::Snapshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Unix time in milliseconds: the newer snapshot's `taken_at_ms`.
    pub t_ms: u64,
    pub kind: EventKind,
    /// The socket label (or `<pci>-<port>` key) the event happened on, when
    /// it is about one.
    pub socket: Option<String>,
    /// One line, without the leading sign: `0bda:5411 hub @10G on USB-C (left)`.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Something plugged in or appearing (`+`).
    Added,
    /// Something unplugged or going away (`−`).
    Removed,
    /// A value changed: link speed, PD contract, charging, AC (`~`).
    Changed,
    /// Something that went wrong: a new kernel error or link reset (`!`).
    Problem,
}

impl EventKind {
    pub fn sign(self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Removed => "−",
            Self::Changed => "~",
            Self::Problem => "!",
        }
    }
}

/// Every change from `old` to `new`, in a stable order.
pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Event> {
    let _ = (old, new);
    todo!("milestone 5: engine")
}
