//! Ghosts: devices unplugged in the last [`GHOST_MS`], kept in their
//! socket's tree as dim `(gone)` rows. Each is a removed subtree from the
//! previous snapshot, found where a Removed event names its socket.

use crate::diff::{Event, EventKind};
use crate::health::socket_name;
use crate::model::{Snapshot, Socket, Usb3Side, UsbDevice};

/// How long an unplugged device stays in the tree.
pub const GHOST_MS: u64 = 30_000;

/// A removed subtree and where it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Ghost {
    /// The socket's key.
    pub socket: String,
    /// On the USB 3 half, else the USB 2 half.
    pub usb3: bool,
    /// The sysfs name of the hub it hung off, `None` on the root port.
    pub parent: Option<String>,
    /// Hub levels below the root port, from 0.
    pub depth: usize,
    pub device: UsbDevice,
    /// Unix ms: the snapshot it was first missing from.
    pub gone_at_ms: u64,
}

impl Ghost {
    pub fn live(&self, now_ms: u64) -> bool {
        now_ms < self.gone_at_ms.saturating_add(GHOST_MS)
    }

    /// `name` in this ghost's subtree, with the devices above it in the
    /// subtree.
    pub fn find(&self, name: &str) -> Option<Vec<&UsbDevice>> {
        chain(std::slice::from_ref(&self.device), name)
    }
}

/// `name` in `devices` or below, as the path from the top: the devices
/// above it, then itself.
pub fn chain<'a>(devices: &'a [UsbDevice], name: &str) -> Option<Vec<&'a UsbDevice>> {
    for d in devices {
        if d.sysfs_name == name {
            return Some(vec![d]);
        }
        if let Some(mut below) = chain(&d.children, name) {
            below.insert(0, d);
            return Some(below);
        }
    }
    None
}

/// Subtrees in `old` and not in `new`, on sockets a Removed event in
/// `events` names. Devices match as in [`crate::diff`]: same sysfs name and
/// vid:pid.
pub fn removed(old: &Snapshot, new: &Snapshot, events: &[Event]) -> Vec<Ghost> {
    let mut out = Vec::new();
    for socket in &new.sockets {
        let name = socket_name(socket);
        let named = events
            .iter()
            .any(|e| e.kind == EventKind::Removed && e.socket.as_deref() == Some(name));
        let Some(before) = old.sockets.iter().find(|s| s.key == socket.key) else {
            continue;
        };
        if !named {
            continue;
        }
        for (usb3, (was, now)) in [true, false]
            .into_iter()
            .zip(halves(before).into_iter().zip(halves(socket)))
        {
            let mut walk = Walk {
                socket: &socket.key,
                usb3,
                t_ms: new.taken_at_ms,
                out: &mut out,
            };
            walk.devices(was, now, None, 0);
        }
    }
    out
}

/// The USB 3 half's device, then the USB 2 half's.
pub fn halves(socket: &Socket) -> [&[UsbDevice]; 2] {
    let usb3 = match &socket.usb3 {
        Usb3Side::Paired(half) => half.device.as_slice(),
        _ => &[],
    };
    [usb3, socket.usb2.device.as_slice()]
}

fn same(a: &UsbDevice, b: &UsbDevice) -> bool {
    a.sysfs_name == b.sysfs_name && a.vendor_id == b.vendor_id && a.product_id == b.product_id
}

struct Walk<'a> {
    socket: &'a str,
    usb3: bool,
    t_ms: u64,
    out: &'a mut Vec<Ghost>,
}

impl Walk<'_> {
    fn devices(
        &mut self,
        was: &[UsbDevice],
        now: &[UsbDevice],
        parent: Option<&str>,
        depth: usize,
    ) {
        for d in was {
            match now.iter().find(|n| same(d, n)) {
                None => self.out.push(Ghost {
                    socket: self.socket.to_owned(),
                    usb3: self.usb3,
                    parent: parent.map(str::to_owned),
                    depth,
                    device: d.clone(),
                    gone_at_ms: self.t_ms,
                }),
                Some(n) => self.devices(&d.children, &n.children, Some(&d.sysfs_name), depth + 1),
            }
        }
    }
}

/// Whether `snapshot` has the ghost's device back: same sysfs name and
/// vid:pid anywhere.
pub fn returned(ghost: &Ghost, snapshot: &Snapshot) -> bool {
    fn any(devices: &[UsbDevice], g: &UsbDevice) -> bool {
        devices.iter().any(|d| same(d, g) || any(&d.children, g))
    }
    snapshot
        .sockets
        .iter()
        .any(|s| halves(s).into_iter().any(|h| any(h, &ghost.device)))
}
