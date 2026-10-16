//! One scan of the machine: every reader's output assembled into a
//! [`Snapshot`].
//!
//! The two subprocesses (`journalctl`, `niri`) and the CPU sampling sleep are
//! opt-in through [`CollectOpts`], and [`snapshot_with`] takes their raw
//! output directly, so fixture tests run offline and deterministically.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::labels::Labels;
use crate::model::{KernelLog, Snapshot, Socket, Usb3Side};
use crate::sysfs::{self, Root, cpu, drm, kernel_log, power, thunderbolt, typec, usb};

/// What to collect beyond sysfs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectOpts {
    /// How long to sample CPU counters; `None` skips the CPU.
    pub cpu_window: Option<Duration>,
    /// Run `journalctl` for this boot's kernel log.
    pub kernel_log: bool,
    /// Ask niri for each display's mode and VRR.
    pub niri: bool,
}

impl CollectOpts {
    /// Everything, as the report on a live machine wants it.
    pub fn live() -> Self {
        Self {
            cpu_window: Some(Duration::from_millis(500)),
            kernel_log: true,
            niri: true,
        }
    }

    /// sysfs only: no sampling sleep, no subprocesses.
    pub fn offline() -> Self {
        Self {
            cpu_window: None,
            kernel_log: false,
            niri: false,
        }
    }
}

/// Scans `root`, running whatever `opts` asks for.
pub fn snapshot(root: &Root, labels: &Labels, opts: &CollectOpts) -> Snapshot {
    snapshot_with(root, labels, opts, None, None)
}

/// [`snapshot`] with the subprocess output supplied: `kernel_log_text` is
/// journalctl's (warnings, everything) and `niri_json` is `niri msg --json
/// outputs`. Either one given wins over running the command; `None` falls
/// back to `opts`.
pub fn snapshot_with(
    root: &Root,
    labels: &Labels,
    opts: &CollectOpts,
    kernel_log_text: Option<(String, String)>,
    niri_json: Option<String>,
) -> Snapshot {
    let sockets = usb::read_sockets(root, labels);

    let mut displays = drm::read(root);
    let niri_json = niri_json.or_else(|| opts.niri.then(drm::query_niri).flatten());
    if let Some(json) = niri_json {
        drm::apply_niri(&mut displays, &drm::niri_outputs(&json));
    }

    let kernel_log_text = kernel_log_text.or_else(|| opts.kernel_log.then(kernel_log::query));
    let kernel_log = match kernel_log_text {
        Some((warnings, all)) => {
            let places = socket_places(&sockets);
            kernel_log::parse(&warnings, &all, |msg| {
                kernel_log::usb_location(msg).and_then(|at| places.get(&at).cloned())
            })
        }
        None => KernelLog {
            errors: Vec::new(),
            benign_count: 0,
            link_resets: Vec::new(),
        },
    };

    Snapshot {
        taken_at_ms: now_ms(),
        host: sysfs::read(root.path("proc/sys/kernel/hostname"))
            .map(|h| h.trim().to_owned())
            .unwrap_or_else(|| "unknown".to_owned()),
        typec: typec::read(root, labels),
        thunderbolt: thunderbolt::read(root),
        displays,
        power: power::read(root),
        cpu: opts.cpu_window.and_then(|w| cpu::measure(root, w)),
        kernel_log,
        sockets,
    }
}

/// Root port (bus, port) → socket label, for both halves of every labelled
/// socket, so a kernel message about a device can name its socket.
pub fn socket_places(sockets: &[Socket]) -> HashMap<(u32, u32), String> {
    let mut places = HashMap::new();
    for socket in sockets {
        let Some(label) = &socket.label else { continue };
        places.insert((socket.usb2.bus, socket.usb2.port), label.clone());
        if let Usb3Side::Paired(half) = &socket.usb3 {
            places.insert((half.bus, half.port), label.clone());
        }
    }
    places
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
