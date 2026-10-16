//! What changed between two snapshots, as events for the TUI's Events tab
//! and chart markers: `+ 0bda:5411 hub @10G on USB-C (left)`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::health::{self, milli, socket_name};
use crate::model::{
    DisplayKind, PdContract, Snapshot, Socket, TbDevice, TypecPort, Usb3Side, UsbCSource, UsbDevice,
};

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

impl Event {
    /// `{sign} {text}`, as the Events tab shows it.
    pub fn line(&self) -> String {
        format!("{} {}", self.kind.sign(), self.text)
    }
}

/// Every change from `old` to `new`, in a stable order: removals, then
/// additions, then changes, then problems; within each kind, in snapshot
/// order (sockets, USB-C, Thunderbolt, displays, power, kernel log).
///
/// Battery percentage and the current a supply draws are left out: they
/// move all the time.
pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Event> {
    let mut out = Events {
        t_ms: new.taken_at_ms,
        events: Vec::new(),
    };
    usb(old, new, &mut out);
    typec(old, new, &mut out);
    thunderbolt(old, new, &mut out);
    displays(old, new, &mut out);
    ac(old, new, &mut out);
    kernel_log(old, new, &mut out);
    let mut events = out.events;
    // Stable: snapshot order survives within each kind.
    events.sort_by_key(|e| match e.kind {
        EventKind::Removed => 0,
        EventKind::Added => 1,
        EventKind::Changed => 2,
        EventKind::Problem => 3,
    });
    events
}

struct Events {
    t_ms: u64,
    events: Vec<Event>,
}

impl Events {
    fn push(&mut self, kind: EventKind, socket: Option<&str>, text: String) {
        self.events.push(Event {
            t_ms: self.t_ms,
            kind,
            socket: socket.map(str::to_owned),
            text,
        });
    }
}

/// Devices per socket and half, matched by sysfs name and vid:pid. Sockets
/// only in `old` lose everything they had.
fn usb(old: &Snapshot, new: &Snapshot, out: &mut Events) {
    for socket in &new.sockets {
        let before = old.sockets.iter().find(|s| s.key == socket.key);
        let name = socket_name(socket);
        for (was, now) in halves(before).into_iter().zip(halves(Some(socket))) {
            devices(was, now, name, out);
        }
    }
    for socket in &old.sockets {
        if !new.sockets.iter().any(|s| s.key == socket.key) {
            for was in halves(Some(socket)) {
                devices(was, &[], socket_name(socket), out);
            }
        }
    }
}

/// The USB 3 half's device, then the USB 2 half's, as slices to walk.
fn halves(socket: Option<&Socket>) -> [&[UsbDevice]; 2] {
    let Some(socket) = socket else {
        return [&[], &[]];
    };
    let usb3 = match &socket.usb3 {
        Usb3Side::Paired(half) => half.device.as_slice(),
        _ => &[],
    };
    [usb3, socket.usb2.device.as_slice()]
}

fn same_device(a: &UsbDevice, b: &UsbDevice) -> bool {
    a.sysfs_name == b.sysfs_name && a.vendor_id == b.vendor_id && a.product_id == b.product_id
}

/// One event per subtree that left or arrived; matched devices are compared
/// for link speed and their children walked.
fn devices(was: &[UsbDevice], now: &[UsbDevice], socket: &str, out: &mut Events) {
    for d in was {
        if !now.iter().any(|n| same_device(d, n)) {
            out.push(EventKind::Removed, Some(socket), device_text(d, socket));
        }
    }
    for d in now {
        match was.iter().find(|w| same_device(w, d)) {
            None => out.push(EventKind::Added, Some(socket), device_text(d, socket)),
            Some(w) => {
                if w.speed != d.speed {
                    out.push(
                        EventKind::Changed,
                        Some(socket),
                        format!(
                            "{} on {socket} {} → {}",
                            device_name(d),
                            w.speed.short(),
                            d.speed.short()
                        ),
                    );
                }
                devices(&w.children, &d.children, socket, out);
            }
        }
    }
}

/// `0bda:5411 USB3.1 Hub`, or `0bda:5411 device` without a product string.
fn device_name(d: &UsbDevice) -> String {
    format!(
        "{:04x}:{:04x} {}",
        d.vendor_id,
        d.product_id,
        d.product.as_deref().unwrap_or("device")
    )
}

/// `05e3:0610 USB2.1 Hub @480M on USB-C (hinge) with 2 devices`.
fn device_text(d: &UsbDevice, socket: &str) -> String {
    let mut text = format!("{} @{} on {socket}", device_name(d), d.speed.short());
    match descendants(d) {
        0 => {}
        1 => text.push_str(" with 1 device"),
        n => text.push_str(&format!(" with {n} devices")),
    }
    text
}

fn descendants(d: &UsbDevice) -> usize {
    d.children.iter().map(|c| 1 + descendants(c)).sum()
}

/// `port0 (USB-C (left))`, or `port1` without a label.
fn port_text(port: &TypecPort) -> String {
    match &port.label {
        Some(label) => format!("{} ({label})", port.name),
        None => port.name.clone(),
    }
}

/// `20V 2.25A ≈ 45W`.
fn contract_text(c: &PdContract) -> String {
    format!(
        "{}V {}A ≈ {:.0}W",
        milli(c.voltage_mv),
        milli(c.current_ma),
        c.watts()
    )
}

fn source<'a>(s: &'a Snapshot, port: &TypecPort) -> Option<&'a UsbCSource> {
    s.power
        .usb_c_sources
        .iter()
        .find(|src| src.typec_port == port.name)
}

/// Partners, charging, the PD contract and whether the supply reports its
/// power, per UCSI port. A charging port's supply is the same online
/// ucsi-source-psy, so a supply coming or going is told as charging
/// starting or stopping, not twice.
fn typec(old: &Snapshot, new: &Snapshot, out: &mut Events) {
    for port in &new.typec.ports {
        let before = old.typec.ports.iter().find(|p| p.name == port.name);
        typec_port(old, new, before, Some(port), out);
    }
    for port in &old.typec.ports {
        if !new.typec.ports.iter().any(|p| p.name == port.name) {
            typec_port(old, new, Some(port), None, out);
        }
    }
}

fn typec_port(
    old: &Snapshot,
    new: &Snapshot,
    was: Option<&TypecPort>,
    now: Option<&TypecPort>,
    out: &mut Events,
) {
    let Some(port) = now.or(was) else { return };
    let socket = port.label.as_deref();
    let name = port_text(port);
    let had_partner = was.is_some_and(|p| p.partner.is_some());
    let has_partner = now.is_some_and(|p| p.partner.is_some());
    if had_partner && !has_partner {
        out.push(
            EventKind::Removed,
            socket,
            format!("USB-C partner on {name}"),
        );
    }
    if has_partner && !had_partner {
        out.push(EventKind::Added, socket, format!("USB-C partner on {name}"));
    }

    let was_charging = was.is_some_and(|p| p.charging);
    let charging = now.is_some_and(|p| p.charging);
    match (was_charging, charging) {
        (false, true) => {
            let text = match source(new, port) {
                Some(src) => health::source_text(new, src),
                None => match port.pd_contract {
                    Some(c) => format!("{name} {}", contract_text(&c)),
                    None => name.clone(),
                },
            };
            out.push(EventKind::Changed, socket, format!("charging via {text}"));
        }
        (true, false) => {
            out.push(
                EventKind::Changed,
                socket,
                format!("stopped charging via {name}"),
            );
        }
        (true, true) => {
            let contracts = (was.and_then(|p| p.pd_contract), port.pd_contract);
            if let (Some(a), Some(b)) = contracts {
                if (a.voltage_mv, a.current_ma) != (b.voltage_mv, b.current_ma) {
                    out.push(
                        EventKind::Changed,
                        socket,
                        format!(
                            "PD contract on {name} {} → {}",
                            contract_text(&a),
                            contract_text(&b)
                        ),
                    );
                }
                return;
            }
            let was_w = source(old, port).and_then(UsbCSource::watts);
            let now_w = source(new, port).and_then(UsbCSource::watts);
            match (was_w, now_w, source(new, port)) {
                (None, Some(_), Some(src)) => out.push(
                    EventKind::Changed,
                    socket,
                    format!("power reported: {}", health::source_text(new, src)),
                ),
                (Some(_), None, _) => out.push(
                    EventKind::Changed,
                    socket,
                    format!("power no longer reported on {name}"),
                ),
                _ => {}
            }
        }
        (false, false) => {}
    }
}

/// Device routers, matched by unique id (else route): arrivals, departures
/// and authorization.
fn thunderbolt(old: &Snapshot, new: &Snapshot, out: &mut Events) {
    let none = Vec::new();
    let was = old.thunderbolt.as_ref().map_or(&none, |t| &t.devices);
    let now = new.thunderbolt.as_ref().map_or(&none, |t| &t.devices);
    let same = |a: &TbDevice, b: &TbDevice| match (&a.unique_id, &b.unique_id) {
        (Some(x), Some(y)) => x == y,
        _ => a.route == b.route,
    };
    for d in was {
        if !now.iter().any(|n| same(d, n)) {
            out.push(EventKind::Removed, None, tb_text(d));
        }
    }
    for d in now {
        match was.iter().find(|w| same(w, d)) {
            None => out.push(EventKind::Added, None, tb_text(d)),
            Some(w) if w.authorized != d.authorized => {
                let state = if d.authorized {
                    "authorized"
                } else {
                    "not authorized"
                };
                out.push(EventKind::Changed, None, format!("{} {state}", tb_text(d)));
            }
            Some(_) => {}
        }
    }
}

/// `Thunderbolt Dell WD19TB (0-1)`.
fn tb_text(d: &TbDevice) -> String {
    let name = [d.vendor.as_deref(), d.name.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        format!("Thunderbolt device {}", d.route)
    } else {
        format!("Thunderbolt {name} ({})", d.route)
    }
}

fn displays(old: &Snapshot, new: &Snapshot, out: &mut Events) {
    for d in &old.displays {
        if !new.displays.iter().any(|n| n.connector == d.connector) {
            out.push(
                EventKind::Removed,
                None,
                format!("{} disconnected", d.connector),
            );
        }
    }
    for d in &new.displays {
        if !old.displays.iter().any(|o| o.connector == d.connector) {
            let alt = if d.kind == DisplayKind::Dp {
                " (USB-C alt mode)"
            } else {
                ""
            };
            out.push(
                EventKind::Added,
                None,
                format!("{} connected{alt}", d.connector),
            );
        }
    }
}

fn ac(old: &Snapshot, new: &Snapshot, out: &mut Events) {
    let Some(online) = new.power.ac_online else {
        return;
    };
    if old.power.ac_online != Some(online) {
        let state = if online { "online" } else { "offline" };
        out.push(EventKind::Changed, None, format!("AC {state}"));
    }
}

/// Error lines not in `old` (by message and time), then link-reset counts
/// that went up, by how much.
fn kernel_log(old: &Snapshot, new: &Snapshot, out: &mut Events) {
    let seen: HashSet<(Option<u64>, &str)> = old
        .kernel_log
        .errors
        .iter()
        .map(|l| (l.secs_since_boot, l.message.as_str()))
        .collect();
    for line in &new.kernel_log.errors {
        if !seen.contains(&(line.secs_since_boot, line.message.as_str())) {
            out.push(
                EventKind::Problem,
                line.socket.as_deref(),
                line.message.clone(),
            );
        }
    }
    for resets in &new.kernel_log.link_resets {
        let before = old
            .kernel_log
            .link_resets
            .iter()
            .find(|r| r.place == resets.place)
            .map_or(0, |r| r.count);
        let more = resets.count.saturating_sub(before);
        if more == 0 {
            continue;
        }
        let place = resets.place.as_str();
        let socket = new
            .sockets
            .iter()
            .any(|s| s.label.as_deref() == Some(place))
            .then_some(place);
        let noun = if more == 1 {
            "link reset"
        } else {
            "link resets"
        };
        out.push(
            EventKind::Problem,
            socket,
            format!("{more} {noun} on {place}"),
        );
    }
}
