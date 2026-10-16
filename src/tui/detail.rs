//! The popups' content: a device's or a socket's details, and the help
//! built from the keymap's table.

use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};

use super::ghosts::{self, Ghost};
use super::keys::{Action, BINDINGS, PROMPT_KEYS};
use crate::health::{self, LinkFit, link_fit};
use crate::model::{Half, PhysicalLocation, Snapshot, Socket, Usb3Side, UsbDevice};

/// What a detail popup shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A device, by sysfs name.
    Device(String),
    /// A socket, by key.
    Socket(String),
}

/// A popup: its title and lines.
pub struct Popup {
    pub title: String,
    pub lines: Vec<Line<'static>>,
}

const LABEL: usize = 19;

fn field(label: &str, value: impl Into<Span<'static>>) -> Line<'static> {
    Line::from(vec![format!("{label:<LABEL$}").dark_gray(), value.into()])
}

fn or_none(value: Option<String>) -> Span<'static> {
    match value {
        Some(v) => Span::raw(v),
        None => "—".dark_gray(),
    }
}

/// Where a device is: its socket, half, and the devices above it.
struct Found<'a> {
    socket: &'a Socket,
    usb3: bool,
    /// From the root port down to the device itself.
    chain: Vec<&'a UsbDevice>,
    /// Unplugged at this Unix ms: a ghost.
    gone_at_ms: Option<u64>,
    /// A ghost's hub, still plugged in.
    parent: Option<&'a str>,
}

fn find<'a>(s: &'a Snapshot, ghosts: &[&'a Ghost], name: &str) -> Option<Found<'a>> {
    for socket in &s.sockets {
        let [usb3, usb2] = ghosts::halves(socket);
        for (on_usb3, devices) in [(true, usb3), (false, usb2)] {
            if let Some(chain) = ghosts::chain(devices, name) {
                return Some(Found {
                    socket,
                    usb3: on_usb3,
                    chain,
                    gone_at_ms: None,
                    parent: None,
                });
            }
        }
    }
    ghosts.iter().find_map(|g| {
        Some(Found {
            socket: s.sockets.iter().find(|x| x.key == g.socket)?,
            usb3: g.usb3,
            chain: g.find(name)?,
            gone_at_ms: Some(g.gone_at_ms),
            parent: g.parent.as_deref(),
        })
    })
}

/// bcdUSB `0x0320` as `3.20`.
fn bcd(b: u16) -> String {
    format!("{:x}.{:02x}", b >> 8, b & 0xff)
}

fn class_name(class: u8) -> &'static str {
    match class {
        0x00 => "per interface",
        0x02 => "communications",
        0x03 => "HID",
        0x08 => "mass storage",
        0x09 => "hub",
        0x0e => "video",
        0x11 => "billboard",
        0xdc => "diagnostic",
        0xe0 => "wireless",
        0xef => "miscellaneous",
        0xff => "vendor specific",
        _ => "other",
    }
}

fn half_name(usb3: bool) -> &'static str {
    if usb3 { "USB 3" } else { "USB 2" }
}

/// `05e3:0610 USB2.1 Hub`.
fn device_name(d: &UsbDevice) -> String {
    let mut name = format!("{:04x}:{:04x}", d.vendor_id, d.product_id);
    if let Some(p) = &d.product {
        name.push(' ');
        name.push_str(p);
    }
    name
}

/// A device's strings, descriptors, link and place.
pub fn device(s: &Snapshot, ghosts: &[&Ghost], name: &str, now_ms: u64) -> Popup {
    let Some(found) = find(s, ghosts, name) else {
        return Popup {
            title: name.to_owned(),
            lines: vec![Line::from(
                format!("{name} is no longer connected").dark_gray(),
            )],
        };
    };
    let d = *found.chain.last().expect("a chain ends at the device");
    let mut out = Vec::new();
    if let Some(t) = found.gone_at_ms {
        let ago = now_ms.saturating_sub(t) / 1000;
        out.push(Line::from(format!("unplugged {ago} s ago").red().bold()));
        out.push(Line::default());
    }
    out.push(field(
        "vendor:product",
        format!("{:04x}:{:04x}", d.vendor_id, d.product_id),
    ));
    out.push(field("manufacturer", or_none(d.manufacturer.clone())));
    out.push(field("product", or_none(d.product.clone())));
    out.push(field("serial", or_none(d.serial.clone())));
    out.push(field("driver", or_none(d.driver.clone())));
    out.push(field("bcdUSB", or_none(d.bcd_usb.map(bcd))));
    out.push(field(
        "device class",
        format!("{:02x} ({})", d.device_class, class_name(d.device_class)),
    ));
    out.push(field(
        "bMaxPower",
        or_none(d.max_power_ma.map(|ma| format!("{ma} mA"))),
    ));
    let lanes = match (d.rx_lanes, d.tx_lanes) {
        (None, None) => None,
        (rx, tx) => Some(format!(
            "rx {} · tx {}",
            rx.map_or("?".into(), |n| n.to_string()),
            tx.map_or("?".into(), |n| n.to_string())
        )),
    };
    out.push(field("lanes", or_none(lanes)));
    out.push(field("urbnum", or_none(d.urbnum.map(|n| n.to_string()))));
    out.push(Line::default());

    out.push(field("sysfs name", d.sysfs_name.clone()));
    let socket = found.socket;
    out.push(field(
        "socket",
        format!("{} ({})", health::socket_name(socket), socket.key),
    ));
    let half = match (&socket.usb3, found.usb3) {
        (Usb3Side::Paired(h), true) => h,
        _ => &socket.usb2,
    };
    out.push(field(
        "half",
        format!(
            "{} (bus {}, root port {})",
            half_name(found.usb3),
            half.bus,
            half.port
        ),
    ));
    let path: Vec<String> = found
        .parent
        .map(str::to_owned)
        .into_iter()
        .chain(found.chain.iter().map(|x| x.sysfs_name.clone()))
        .collect();
    out.push(field("path", path.join(" › ")));
    if found.chain.len() > 1 {
        let above: Vec<String> = found.chain[..found.chain.len() - 1]
            .iter()
            .map(|x| device_name(x))
            .collect();
        out.push(field("via", above.join(" › ")));
    }
    out.push(Line::default());

    out.push(field("linked at", d.speed.label()));
    let capable = d
        .capable_speed
        .map_or_else(|| "—".dark_gray(), |c| Span::raw(c.label()));
    out.push(field("capable of", capable));
    let fit = link_fit(socket, found.usb3, d);
    let style = match fit {
        LinkFit::Below { .. } => Style::new().fg(Color::Yellow).bold(),
        LinkFit::Best => Style::new().fg(Color::Green),
        LinkFit::Usb2HalfOfUsb3Hub | LinkFit::Unknown => Style::new(),
    };
    out.push(field(
        "capable vs linked",
        Span::styled(fit.text(d.speed), style),
    ));
    Popup {
        title: device_name(d),
        lines: out,
    }
}

fn location(loc: &PhysicalLocation) -> String {
    let mut parts = Vec::new();
    if let Some(p) = &loc.panel {
        parts.push(format!("{p} panel"));
    }
    parts.extend(loc.horizontal.clone());
    parts.extend(loc.vertical.clone());
    if loc.dock {
        parts.push("dock".into());
    }
    if loc.lid {
        parts.push("lid".into());
    }
    if parts.is_empty() {
        "unknown".into()
    } else {
        parts.join(" · ")
    }
}

fn half_lines(name: &str, half: &Half, out: &mut Vec<Line<'static>>) {
    out.push(field(
        name,
        format!("bus {}, root port {}", half.bus, half.port),
    ));
    let device = half
        .device
        .as_ref()
        .map_or_else(|| "idle".dark_gray(), |d| Span::raw(device_name(d)));
    out.push(field("  device", device));
    out.push(field("  port state", or_none(half.port_state.clone())));
    let oc = half.over_current_count;
    let oc = match oc {
        Some(n) if n > 0 => format!("{n}").red().bold(),
        Some(n) => Span::raw(n.to_string()),
        None => "—".dark_gray(),
    };
    out.push(field("  over-current", oc));
    out.push(field(
        "  location",
        or_none(half.physical_location.as_ref().map(location)),
    ));
}

/// A socket's halves and USB-C port.
pub fn socket(s: &Snapshot, key: &str) -> Popup {
    let Some(socket) = s.sockets.iter().find(|x| x.key == key) else {
        return Popup {
            title: key.to_owned(),
            lines: vec![Line::from(format!("{key} is gone").dark_gray())],
        };
    };
    let mut out = vec![
        field("key", socket.key.clone()),
        field("label", or_none(socket.label.clone())),
        Line::default(),
    ];
    match &socket.usb3 {
        Usb3Side::Paired(h) => half_lines("USB 3 half", h, &mut out),
        Usb3Side::Unknown => out.push(field(
            "USB 3 half",
            "unknown (no peer link — add a peer: line)".yellow(),
        )),
        Usb3Side::Usb2Only => out.push(field("USB 3 half", "none (USB 2 only)".dark_gray())),
    }
    half_lines("USB 2 half", &socket.usb2, &mut out);
    out.push(Line::default());
    let typec = socket.typec_port.as_ref().map(|name| {
        let mut text = name.clone();
        if let Some(port) = s.typec.ports.iter().find(|p| &p.name == name) {
            text.push_str(if port.partner.is_some() {
                " · partner"
            } else {
                " · empty"
            });
            if port.charging {
                text.push_str(" · charging");
            }
        }
        text
    });
    out.push(field("USB-C port", or_none(typec)));
    out.push(Line::from(
        "location is ACPI's hint: labels win over it".dark_gray(),
    ));
    Popup {
        title: health::socket_name(socket).to_owned(),
        lines: out,
    }
}

const KEYS: usize = 18;

/// `j  ↓`, and `Ctrl-w h j k l` for sequences sharing their first key.
fn keys_text(keys: &[(&str, Action)]) -> String {
    let mut out = String::new();
    let mut prev_first = None;
    for (k, _) in keys {
        match k.split_once(' ') {
            Some((first, rest)) if prev_first == Some(first) => {
                out.push(' ');
                out.push_str(rest);
            }
            split => {
                if !out.is_empty() {
                    out.push_str("  ");
                }
                out.push_str(k);
                prev_first = split.map(|(first, _)| first);
            }
        }
    }
    out
}

/// Every binding by section, then the prompt's keys.
pub fn help() -> Popup {
    let mut out = Vec::new();
    let mut section = "";
    for b in BINDINGS {
        if b.section != section {
            if !section.is_empty() {
                out.push(Line::default());
            }
            section = b.section;
            out.push(Line::from(section.to_owned().bold().cyan()));
        }
        out.push(Line::from(vec![
            format!("  {:<KEYS$}", keys_text(b.keys)).bold(),
            Span::raw(b.help),
        ]));
    }
    out.push(Line::default());
    out.push(Line::from("In the / prompt".bold().cyan()));
    for (keys, help) in PROMPT_KEYS {
        out.push(Line::from(vec![
            format!("  {keys:<KEYS$}").bold(),
            Span::raw(*help),
        ]));
    }
    out.push(Line::default());
    out.push(Line::from(
        "A dim ↓ after a speed: linked below what the device supports (⏎ tells more).".dark_gray(),
    ));
    Popup {
        title: "Keys".into(),
        lines: out,
    }
}
