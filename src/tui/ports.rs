//! The Ports tab's content: the socket cards as rows, and the USB-C / PD,
//! Power and Health panels as lines.

use std::collections::HashSet;

use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use crate::health::{self, hub_stuck_at_usb2, over_current};
use crate::model::{
    DisplayKind, Finding, Level, Snapshot, Socket, Speed, TypecPort, Usb3Side, UsbDevice,
};
use crate::report;

/// One row of the socket list.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Stable across scans, to keep the selection on the same row.
    pub id: String,
    pub kind: RowKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    /// A card's title: the socket's label (or key) and its key.
    Socket {
        name: String,
        key: String,
        label: Option<String>,
    },
    /// `USB 3  linked` and the like.
    Half(Line<'static>),
    Device {
        device: Box<UsbDevice>,
        /// Hub levels below the root port, from 0.
        depth: usize,
        /// A hub stuck at USB 2: badge in red.
        stuck: bool,
        /// Its subtree is hidden.
        collapsed: bool,
    },
    /// A warning or a USB-C remark under a card.
    Note(Line<'static>),
}

impl Row {
    /// The sysfs name of a device with devices below it.
    pub fn hub_name(&self) -> Option<&str> {
        match &self.kind {
            RowKind::Device { device, .. } if !device.children.is_empty() => {
                Some(&device.sysfs_name)
            }
            _ => None,
        }
    }

    /// Whether a recent event about `socket` (a label or key) belongs here.
    pub fn socket_matches(&self, socket: &str) -> bool {
        matches!(&self.kind, RowKind::Socket { key, label, .. }
            if key == socket || label.as_deref() == Some(socket))
    }
}

/// Every socket as a card: title, USB 3 half, USB 2 half (each with its
/// device tree), USB-C partner, warnings.
pub fn rows(s: &Snapshot, collapsed: &HashSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    for socket in &s.sockets {
        card(s, socket, collapsed, &mut out);
    }
    out
}

fn card(s: &Snapshot, socket: &Socket, collapsed: &HashSet<String>, out: &mut Vec<Row>) {
    let key = &socket.key;
    out.push(Row {
        id: key.clone(),
        kind: RowKind::Socket {
            name: health::socket_name(socket).to_owned(),
            key: key.clone(),
            label: socket.label.clone(),
        },
    });
    let half = |name: &str, state: Span<'static>| {
        Line::from(vec![Span::raw(format!(" {name}  ")).bold(), state])
    };
    let stuck = hub_stuck_at_usb2(socket);
    let mut usb3_linked = false;
    match &socket.usb3 {
        Usb3Side::Paired(h) => {
            let state = if h.device.is_some() {
                usb3_linked = true;
                "linked".green()
            } else {
                "idle".dark_gray()
            };
            out.push(Row {
                id: format!("{key}/usb3"),
                kind: RowKind::Half(half("USB 3", state)),
            });
            if let Some(d) = &h.device {
                tree(d, 0, false, collapsed, out);
            }
        }
        Usb3Side::Unknown => out.push(Row {
            id: format!("{key}/usb3"),
            kind: RowKind::Half(Line::from(vec![
                Span::raw(" USB 3  ").bold(),
                "unknown".yellow(),
                " (no peer link — add a peer: line)".dark_gray(),
            ])),
        }),
        Usb3Side::Usb2Only => {}
    }
    let state = if socket.usb2.device.is_some() {
        "linked".green()
    } else {
        "idle".dark_gray()
    };
    out.push(Row {
        id: format!("{key}/usb2"),
        kind: RowKind::Half(half("USB 2", state)),
    });
    if let Some(d) = &socket.usb2.device {
        tree(d, 0, stuck, collapsed, out);
    }
    let typec = socket
        .typec_port
        .as_deref()
        .and_then(|name| s.typec.ports.iter().find(|p| p.name == name));
    if let Some(port) = typec.filter(|p| p.partner.is_some()) {
        let mut spans = vec![
            Span::raw(" USB-C  ").bold(),
            "partner".green(),
            format!(" ({})", port.name).dark_gray(),
        ];
        if port.charging {
            spans.push(" · ".dark_gray());
            spans.push("charging".yellow());
        }
        out.push(Row {
            id: format!("{key}/typec"),
            kind: RowKind::Half(Line::from(spans)),
        });
        if !usb3_linked && socket.usb2.device.is_none() {
            let lit: Vec<&str> = s
                .displays
                .iter()
                .filter(|d| d.kind == DisplayKind::Dp)
                .map(|d| d.connector.as_str())
                .collect();
            let note = if lit.is_empty() {
                Line::from("   └ no USB data, no DisplayPort (charger only?)".dark_gray())
            } else {
                Line::from(vec![
                    "   └ display in DP alt mode → ".dark_gray(),
                    lit.join(" ").green(),
                ])
            };
            out.push(Row {
                id: format!("{key}/typec-note"),
                kind: RowKind::Note(note),
            });
        }
    }
    if stuck {
        out.push(Row {
            id: format!("{key}/stuck"),
            kind: RowKind::Note(Line::from(
                " ⚠ hub linked at USB 2 only — its USB 3 half is missing"
                    .red()
                    .bold(),
            )),
        });
    }
    let oc = over_current(socket);
    if oc > 0 {
        out.push(Row {
            id: format!("{key}/oc"),
            kind: RowKind::Note(Line::from(
                format!(" ⚠ over-current reported on this socket ({oc}×)")
                    .red()
                    .bold(),
            )),
        });
    }
}

fn tree(
    device: &UsbDevice,
    depth: usize,
    stuck: bool,
    collapsed: &HashSet<String>,
    out: &mut Vec<Row>,
) {
    let is_collapsed = !device.children.is_empty() && collapsed.contains(&device.sysfs_name);
    out.push(Row {
        id: device.sysfs_name.clone(),
        kind: RowKind::Device {
            device: Box::new(device.clone()),
            depth,
            stuck,
            collapsed: is_collapsed,
        },
    });
    if !is_collapsed {
        for child in &device.children {
            tree(child, depth + 1, false, collapsed, out);
        }
    }
}

/// `5G`, `480M`: the speed as a short badge.
pub fn speed_badge(speed: Speed) -> String {
    match speed {
        Speed::Low => "1.5M".into(),
        Speed::Full => "12M".into(),
        Speed::High => "480M".into(),
        Speed::Super => "5G".into(),
        Speed::SuperPlus => "10G".into(),
        Speed::SuperPlus2x2 => "20G".into(),
        Speed::Other(mbps) => format!("{mbps}M"),
    }
}

/// Green on a SuperSpeed link, yellow below it, red for a hub stuck at USB 2.
pub fn speed_color(speed: Speed, stuck: bool) -> Color {
    if stuck {
        Color::Red
    } else if speed.is_superspeed() {
        Color::Green
    } else {
        Color::Yellow
    }
}

fn count_below(device: &UsbDevice) -> usize {
    device.children.iter().map(|c| 1 + count_below(c)).sum()
}

/// A row as a line `width` columns wide (the title's rule fills it).
pub fn row_line(row: &Row, width: usize) -> Line<'static> {
    match &row.kind {
        RowKind::Socket { name, key, .. } => {
            let used = name.chars().count() + key.chars().count() + 6;
            let fill = "─".repeat(width.saturating_sub(used).max(1));
            Line::from(vec![
                "─ ".dark_gray(),
                Span::raw(name.clone()).bold(),
                format!(" {fill} ").dark_gray(),
                Span::raw(key.clone()).dark_gray(),
                " ─".dark_gray(),
            ])
        }
        RowKind::Half(line) | RowKind::Note(line) => line.clone(),
        RowKind::Device {
            device,
            depth,
            stuck,
            collapsed,
        } => {
            let marker = match (device.children.is_empty(), collapsed) {
                (true, _) => "  ",
                (false, false) => "▾ ",
                (false, true) => "▸ ",
            };
            let color = speed_color(device.speed, *stuck);
            let mut spans = vec![
                Span::raw(format!("   {:indent$}", "", indent = depth * 2)),
                marker.dark_gray(),
                Span::styled(
                    format!("{:>4}", speed_badge(device.speed)),
                    Style::new().fg(color).add_modifier(Modifier::BOLD),
                ),
                format!("  {:04x}:{:04x}  ", device.vendor_id, device.product_id).dark_gray(),
                Span::raw(device.product.clone().unwrap_or_default()),
            ];
            if *collapsed {
                spans.push(format!("  (+{})", count_below(device)).dark_gray());
            }
            Line::from(spans)
        }
    }
}

/// Each UCSI port: partner, every source PDO (the active one marked), and
/// the contract.
pub fn usb_c_lines(s: &Snapshot) -> Vec<Line<'static>> {
    if !s.typec.ucsi_present {
        return vec![Line::from(
            "no USB-C ports registered (UCSI missing)".yellow(),
        )];
    }
    let mut out = Vec::new();
    for port in &s.typec.ports {
        port_lines(port, &mut out);
    }
    if out.is_empty() {
        out.push(Line::from("no USB-C ports".dark_gray()));
    }
    out
}

fn port_lines(port: &TypecPort, out: &mut Vec<Line<'static>>) {
    let mut title = vec![Span::raw(port.name.clone()).bold()];
    if let Some(label) = &port.label {
        title.push(format!(" · {label}").bold());
    }
    let Some(partner) = &port.partner else {
        title.push("  empty".dark_gray());
        out.push(Line::from(title));
        return;
    };
    title.push("  partner".green());
    out.push(Line::from(title));
    let mut details = Vec::new();
    if partner.supports_pd == Some(true) {
        let rev = partner.pd_revision.as_deref().unwrap_or("");
        details.push(Span::raw(format!("PD {rev}").trim_end().to_owned()));
    }
    if port.charging {
        details.push("charging".yellow());
    }
    if let Some(cable) = &port.cable_type {
        details.push(format!("cable {cable}").dark_gray());
    }
    if !details.is_empty() {
        let mut line = vec![Span::raw("  ")];
        for (i, d) in details.into_iter().enumerate() {
            if i > 0 {
                line.push(" · ".dark_gray());
            }
            line.push(d);
        }
        out.push(Line::from(line));
    }
    if health::offers_unreported(port) {
        out.push(Line::from("  offers not reported".dark_gray()));
    }
    let active = port
        .pd_contract
        .filter(|_| port.charging)
        .and_then(|c| c.source_pdo);
    for (i, pdo) in partner.source_pdos.iter().enumerate() {
        let watts = pdo
            .max_watts()
            .map(|w| format!("{w:>4.0} W"))
            .unwrap_or_default();
        let text = format!("{:<16}{watts}", report::pdo_text(pdo));
        if active == u8::try_from(i + 1).ok() {
            out.push(Line::from(vec![
                "  ▶ ".green().bold(),
                Span::styled(text, Style::new().fg(Color::Green).bold()),
            ]));
        } else {
            out.push(Line::from(format!("    {text}")));
        }
    }
    if let Some(c) = port.pd_contract {
        out.push(Line::from(vec![
            "  contract ".dark_gray(),
            Span::raw(format!(
                "{}V {}A ≈ {:.0} W",
                health::milli(c.voltage_mv),
                health::milli(c.current_ma),
                c.watts()
            )),
        ]));
    }
    if health::usb_c_status_stale(port) {
        out.push(Line::from(
            "  status looks stale — replug to refresh".yellow(),
        ));
    }
}

/// AC, battery, USB-C supplies and the CPU line, wrapped to `width`.
pub fn power_lines(s: &Snapshot, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let ac = match s.power.ac_online {
        Some(true) => "online".green(),
        Some(false) => "offline".yellow(),
        None => "no mains supply".dark_gray(),
    };
    out.push(Line::from(vec!["AC       ".bold(), ac]));
    let sources = health::usb_c_sources_text(s);
    if !sources.is_empty() {
        labelled(&mut out, "USB-C    ", &sources, Style::new(), width);
    }
    let battery = match &s.power.battery {
        Some(b) => {
            let mut spans = vec![Span::raw(format!(
                "{}% {}",
                b.capacity_pct.map_or("?".into(), |c| c.to_string()),
                b.status.as_deref().unwrap_or("")
            ))];
            if let Some(limit) = b.charge_limit_pct {
                spans.push(format!("  (limit {limit}%)").dark_gray());
            }
            if let Some(flow) = b.flow_mw {
                let sign = if flow < 0 { '−' } else { '+' };
                spans.push(Span::raw(format!(
                    "  {sign}{:.1} W",
                    flow.unsigned_abs() as f64 / 1000.0
                )));
            }
            spans
        }
        None => vec!["none".dark_gray()],
    };
    let mut line = vec!["Battery  ".bold()];
    line.extend(battery);
    out.push(Line::from(line));
    if let Some(cpu) = s.cpu.as_ref().filter(|c| c.avg_khz > 0) {
        let mut text = format!(
            "{}% busy  {:.1} GHz (max {:.1})",
            cpu.util_pct,
            f64::from(cpu.avg_khz) / 1e6,
            f64::from(cpu.max_khz) / 1e6
        );
        if let Some(w) = cpu.package_w {
            text.push_str(&format!("  {w:.0} W"));
        }
        if let Some(t) = cpu.package_temp_c {
            text.push_str(&format!("  {} °C", t.trunc()));
        }
        let style = match health::cpu_throttled(s) {
            Some(true) => Style::new().fg(Color::Red).bold(),
            _ => Style::new(),
        };
        labelled(&mut out, "CPU      ", &text, style, width);
    } else {
        out.push(Line::from(vec![
            "CPU      ".bold(),
            "no sample".dark_gray(),
        ]));
    }
    out
}

/// `label` then `text` wrapped under itself.
fn labelled(out: &mut Vec<Line<'static>>, label: &str, text: &str, style: Style, width: usize) {
    let indent = label.chars().count();
    for (i, part) in wrap(text, width.saturating_sub(indent).max(10))
        .into_iter()
        .enumerate()
    {
        let lead = if i == 0 {
            Span::raw(label.to_owned()).bold()
        } else {
            Span::raw(" ".repeat(indent))
        };
        out.push(Line::from(vec![lead, Span::styled(part, style)]));
    }
}

/// The findings, wrapped to `width` with a hanging indent.
pub fn health_lines(findings: &[Finding], width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for f in findings {
        let (mark, color) = match f.level {
            Level::Ok => ("✓", Color::Green),
            Level::Warn => ("!", Color::Yellow),
            Level::Fail => ("✗", Color::Red),
        };
        let style = match f.level {
            Level::Ok => Style::new(),
            _ => Style::new().fg(color),
        };
        for (i, part) in wrap(&f.message, width.saturating_sub(2).max(10))
            .into_iter()
            .enumerate()
        {
            let lead = if i == 0 {
                Span::styled(format!("{mark} "), Style::new().fg(color).bold())
            } else {
                Span::raw("  ")
            };
            out.push(Line::from(vec![lead, Span::styled(part, style)]));
        }
    }
    if findings.is_empty() {
        out.push(Line::from("no findings yet".dark_gray()));
    }
    out
}

/// Greedy word wrap by characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let len = line.chars().count();
        if len > 0 && len + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words() {
        assert_eq!(wrap("aa bb cc", 5), ["aa bb", "cc"]);
        assert_eq!(wrap("toolongword x", 4), ["toolongword", "x"]);
    }
}
