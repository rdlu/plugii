//! The text report: the `usb-ports` script's sections and layout, with ANSI
//! colours only when asked.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::health::{
    self, hub_stuck_at_usb2, link_reset_total, link_resets_text, milli, over_current,
};
use crate::model::{
    BiosCheck, Catalog, Display, DisplayKind, Finding, Firmware, Fwupd, Level, Pdo, Snapshot,
    TypecPort, Usb3Side, UsbDevice, Vrr,
};
use crate::vendor::{dell, lenovo};

/// Error lines the kernel log section shows, the latest last.
pub const LOG_LINES_SHOWN: usize = 5;
/// An error this soon after boot happened while the machine came up.
pub const AT_BOOT_SECS: u64 = 60;

/// ANSI styling, or none.
#[derive(Debug, Clone, Copy)]
struct Style(bool);

impl Style {
    fn wrap(self, code: &str, text: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
    fn bold(self, t: &str) -> String {
        self.wrap("1", t)
    }
    fn dim(self, t: &str) -> String {
        self.wrap("2", t)
    }
    fn red(self, t: &str) -> String {
        self.wrap("31", t)
    }
    fn green(self, t: &str) -> String {
        self.wrap("32", t)
    }
    fn yellow(self, t: &str) -> String {
        self.wrap("33", t)
    }
}

/// The full report: every section, then Health.
pub fn render(s: &Snapshot, findings: &[Finding], color: bool) -> String {
    let st = Style(color);
    let mut sections = vec![usb_ports(s, st), usb_c(s, st)];
    if let Some(tb) = thunderbolt(s, st) {
        sections.push(tb);
    }
    sections.extend([displays(s, st), power(s, st), kernel_log(s, st)]);
    if let Some(fw) = &s.firmware {
        sections.push(firmware(fw, st));
    }
    sections.push(render_health(findings, color));
    sections.join("\n")
}

/// The Health section alone, as `plugii -H` prints it.
pub fn render_health(findings: &[Finding], color: bool) -> String {
    let st = Style(color);
    let mut out = format!("{}\n", st.bold("Health"));
    for f in findings {
        let mark = match f.level {
            Level::Ok => st.green("✓"),
            Level::Warn => st.yellow("⚠"),
            Level::Fail => st.red("✗"),
        };
        let _ = writeln!(out, "  {mark} {}", f.message);
    }
    if health::is_bad(findings) {
        let _ = writeln!(out, "  {}", st.yellow("→ something needs a look"));
    } else {
        let _ = writeln!(out, "  {}", st.green("→ all good"));
    }
    out
}

/// `{ "snapshot": …, "findings": … }`, as `plugii --json` prints it.
pub fn json(s: &Snapshot, findings: &[Finding]) -> Value {
    json!({ "snapshot": s, "findings": findings })
}

fn usb_ports(s: &Snapshot, st: Style) -> String {
    let mut out = format!("{}\n", st.bold("USB ports"));
    let lit_dp: Vec<&str> = s
        .displays
        .iter()
        .filter(|d| d.kind == DisplayKind::Dp)
        .map(|d| d.connector.as_str())
        .collect();
    if s.sockets.is_empty() {
        let _ = writeln!(out, "  {}", st.dim("no hotplug USB ports found"));
    }
    for socket in &s.sockets {
        let label = socket.label.as_deref().unwrap_or("unlabeled");
        let _ = writeln!(
            out,
            "\n  {} {}",
            st.bold(&format!("{label:<20}")),
            st.dim(&socket.key)
        );
        let usb3_linked = match &socket.usb3 {
            Usb3Side::Paired(half) => match &half.device {
                Some(d) => {
                    let _ = writeln!(out, "    USB 3 half: {}", st.green("linked"));
                    tree(&mut out, d, st);
                    true
                }
                None => {
                    let _ = writeln!(out, "    USB 3 half: {}", st.dim("idle"));
                    false
                }
            },
            // Only on machines without `peer` links: elsewhere an unpaired
            // socket is USB 2-only and gets no USB 3 line.
            Usb3Side::Unknown => {
                let _ = writeln!(
                    out,
                    "    USB 3 half: {} {}",
                    st.yellow("unknown"),
                    st.dim("(no peer link — add a peer: line)")
                );
                false
            }
            Usb3Side::Usb2Only => false,
        };
        let usb2_linked = match &socket.usb2.device {
            Some(d) => {
                let _ = writeln!(out, "    USB 2 half: {}", st.green("linked"));
                tree(&mut out, d, st);
                true
            }
            None => {
                let _ = writeln!(out, "    USB 2 half: {}", st.dim("idle"));
                false
            }
        };
        // The socket's USB-C side, when the labels map a UCSI port to it.
        let typec = socket
            .typec_port
            .as_deref()
            .and_then(|name| s.typec.ports.iter().find(|p| p.name == name));
        if let Some(port) = typec.filter(|p| p.partner.is_some()) {
            let _ = writeln!(
                out,
                "    USB-C:      {} {}",
                st.green("partner"),
                st.dim(&format!("({})", port.name))
            );
            if !usb2_linked && !usb3_linked {
                // No USB data but a partner: a display in alt mode, or a bare charger.
                if lit_dp.is_empty() {
                    let _ = writeln!(
                        out,
                        "      {} no USB data, no DisplayPort (charger only?)",
                        st.dim("└")
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "      {} display in DP alt mode → {}",
                        st.dim("└"),
                        st.green(&lit_dp.join(" "))
                    );
                }
            }
        }
        if hub_stuck_at_usb2(socket) {
            let _ = writeln!(
                out,
                "    {}",
                st.red("⚠ hub linked at USB 2 only — its USB 3 half is missing")
            );
        }
        let oc = over_current(socket);
        if oc > 0 {
            let _ = writeln!(
                out,
                "    {}",
                st.red(&format!("⚠ over-current reported on this socket ({oc}×)"))
            );
        }
    }
    out
}

/// A device and everything below it, indented by hub depth; a device's
/// label stands in for its product string.
fn tree(out: &mut String, device: &UsbDevice, st: Style) {
    let depth = device.sysfs_name.matches('.').count();
    let line = format!(
        "      {:indent$}{} {:04x}:{:04x}  {:<22} {}",
        "",
        st.dim("└"),
        device.vendor_id,
        device.product_id,
        device.speed.label(),
        device
            .label
            .as_deref()
            .or(device.product.as_deref())
            .unwrap_or(""),
        indent = depth * 2
    );
    let _ = writeln!(out, "{}", line.trim_end());
    for child in &device.children {
        tree(out, child, st);
    }
}

fn usb_c(s: &Snapshot, st: Style) -> String {
    let mut out = format!("{}\n", st.bold("USB-C (UCSI)"));
    if !s.typec.ucsi_present {
        let _ = writeln!(out, "  {}", st.yellow("no USB-C ports registered"));
        return out;
    }
    for port in &s.typec.ports {
        let name = match &port.label {
            Some(label) => format!("{} ({label})", port.name),
            None => port.name.clone(),
        };
        let Some(partner) = &port.partner else {
            let _ = writeln!(out, "  {name}: {}", st.dim("empty"));
            continue;
        };
        let pd = match partner.supports_pd {
            Some(true) => "yes",
            Some(false) => "no",
            None => "",
        };
        let power_in = if port.charging {
            st.yellow("charging")
        } else {
            "no".to_owned()
        };
        let cable = port
            .cable_type
            .as_deref()
            .map(|c| format!("  cable={c}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "  {name}: {}  pd={pd}  alt-modes={}  power-in={power_in}{cable}",
            st.green("partner"),
            partner.alt_modes
        );
        if let Some(offers) = offers(port, st) {
            let _ = writeln!(out, "    offers: {offers}");
        }
    }
    out
}

/// The partner's source PDOs, the active one marked:
/// `5V 1A · 9V 1A · ▶20V 2.35A (47 W)`.
fn offers(port: &TypecPort, st: Style) -> Option<String> {
    let pdos = &port.partner.as_ref()?.source_pdos;
    if health::offers_unreported(port) {
        return Some(st.dim("not reported"));
    }
    if pdos.is_empty() {
        return None;
    }
    let active = port
        .pd_contract
        .filter(|_| port.charging)
        .and_then(|c| c.source_pdo);
    let parts: Vec<String> = pdos
        .iter()
        .enumerate()
        .map(|(i, pdo)| {
            let text = pdo_text(pdo);
            if active == Some(u8::try_from(i + 1).unwrap_or(0)) {
                let watts = pdo
                    .max_watts()
                    .map(|w| format!(" ({w:.0} W)"))
                    .unwrap_or_default();
                st.green(&format!("▶{text}{watts}"))
            } else {
                text
            }
        })
        .collect();
    Some(parts.join(" · "))
}

/// One PDO: `20V 2.35A`, `PPS 3.3-21V 3A`.
pub fn pdo_text(pdo: &Pdo) -> String {
    match pdo {
        Pdo::Fixed { mv, ma } => format!("{}V {}A", milli(*mv), milli(*ma)),
        Pdo::Variable { min_mv, max_mv, ma } => {
            format!("{}-{}V {}A", milli(*min_mv), milli(*max_mv), milli(*ma))
        }
        Pdo::Pps { min_mv, max_mv, ma } => {
            format!("PPS {}-{}V {}A", milli(*min_mv), milli(*max_mv), milli(*ma))
        }
        Pdo::Battery { min_mv, max_mv, mw } => {
            format!("{}-{}V {}W", milli(*min_mv), milli(*max_mv), milli(*mw))
        }
        Pdo::Other(name) => name.clone(),
    }
}

fn thunderbolt(s: &Snapshot, st: Style) -> Option<String> {
    let tb = s.thunderbolt.as_ref()?;
    let mut out = format!("{}\n", st.bold("Thunderbolt / USB4"));
    for d in &tb.domains {
        let iommu = if d.iommu_dma_protection {
            ", IOMMU DMA protection"
        } else {
            ""
        };
        let controller = format!(
            "{} {}, gen {}",
            d.controller_vendor.as_deref().unwrap_or(""),
            d.controller_device.as_deref().unwrap_or(""),
            opt(d.generation)
        );
        let _ = writeln!(
            out,
            "  domain{}  security {}{iommu}  {}",
            d.index,
            d.security.as_deref().unwrap_or("?"),
            st.dim(controller.trim_start())
        );
    }
    for d in &tb.devices {
        let auth = if d.authorized {
            st.green("authorized")
        } else {
            st.yellow("not authorized")
        };
        let mut extra = d
            .link
            .map(|l| {
                format!(
                    "rx {:.1} Gb/s×{}  tx {:.1} Gb/s×{}",
                    l.rx_gbps, l.rx_lanes, l.tx_gbps, l.tx_lanes
                )
            })
            .unwrap_or_default();
        if let Some(stored) = d.boltd_stored {
            if !extra.is_empty() {
                extra.push_str("  ");
            }
            extra.push_str(if stored { "stored yes" } else { "stored no" });
        }
        let line = format!(
            "  {}  {} {}  {auth}  gen {}  {}",
            d.route,
            d.vendor.as_deref().unwrap_or(""),
            d.name.as_deref().unwrap_or(""),
            opt(d.generation),
            if extra.is_empty() {
                String::new()
            } else {
                st.dim(&extra)
            }
        );
        let _ = writeln!(out, "{}", line.trim_end());
    }
    if tb.devices.is_empty() {
        let _ = writeln!(out, "  {}", st.dim("no Thunderbolt/USB4 devices attached"));
    }
    Some(out)
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "?".to_owned(), |v| v.to_string())
}

fn displays(s: &Snapshot, st: Style) -> String {
    let mut out = format!("{}\n", st.bold("Display connectors"));
    for d in &s.displays {
        let alt = if d.kind == DisplayKind::Dp {
            " (USB-C alt mode)"
        } else {
            ""
        };
        let detail = display_detail(d);
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!("  {}", st.dim(&detail))
        };
        let _ = writeln!(out, "  {} connected{alt}{detail}", st.green(&d.connector));
    }
    if s.displays.is_empty() {
        let _ = writeln!(out, "  {}", st.dim("no external display connected"));
    }
    out
}

/// `2560x1440@59.951  VRR on`, from niri when it runs.
fn display_detail(d: &Display) -> String {
    let mut parts = Vec::new();
    if let Some(mode) = &d.mode {
        parts.push(mode.clone());
    }
    if let Some(vrr) = d.vrr {
        parts.push(format!(
            "VRR {}",
            match vrr {
                Vrr::On => "on",
                Vrr::Off => "off",
                Vrr::Unsupported => "unsupported",
            }
        ));
    }
    parts.join("  ")
}

fn power(s: &Snapshot, st: Style) -> String {
    let mut out = format!("{}\n", st.bold("Power"));
    let ac = match s.power.ac_online {
        Some(true) => st.green("online"),
        Some(false) => st.yellow("offline"),
        None => st.dim("no mains supply"),
    };
    let sources = health::usb_c_sources_text(s);
    let via = if sources.is_empty() {
        String::new()
    } else {
        format!("  via USB-C {sources}")
    };
    let battery = match &s.power.battery {
        Some(b) => {
            let mut text = format!(
                "battery: {}% {}",
                opt(b.capacity_pct),
                b.status.as_deref().unwrap_or("")
            );
            if let Some(limit) = b.charge_limit_pct {
                let _ = write!(text, "  (charge limit {limit}%)");
            }
            text
        }
        None => "no battery".to_owned(),
    };
    let _ = writeln!(out, "  AC: {ac}{via}  {}", battery.trim_end());
    cpu_lines(s, st, &mut out);
    out
}

fn cpu_lines(s: &Snapshot, st: Style, out: &mut String) {
    let Some(cpu) = s.cpu.as_ref().filter(|c| c.avg_khz > 0) else {
        return;
    };
    let ghz = |khz: u32| f64::from(khz) / 1e6;
    let mut line = format!(
        "  CPU: {}% busy  {:.1} GHz avg (max {:.1}",
        cpu.util_pct,
        ghz(cpu.avg_khz),
        ghz(cpu.max_khz)
    );
    if let Some(min) = cpu.min_khz {
        let _ = write!(line, ", floor {:.1}", ghz(min));
    }
    line.push(')');
    if let Some(w) = cpu.package_w {
        let _ = write!(line, "  package {w:.0} W");
    }
    if let Some(t) = cpu.package_temp_c {
        let _ = write!(line, "  {} °C", t.trunc());
    }
    if let Some(flow) = s.power.battery.as_ref().and_then(|b| b.flow_mw) {
        let sign = if flow < 0 { '−' } else { '+' };
        let _ = write!(
            line,
            "  battery {sign}{:.1} W",
            flow.unsigned_abs() as f64 / 1000.0
        );
    }
    let _ = writeln!(out, "{line}");
    if cpu.util_pct < health::THROTTLE_BUSY_PCT {
        let _ = writeln!(
            out,
            "  {}",
            st.dim(
                "CPU mostly idle — load it (e.g. a build) to test whether this power source throttles"
            )
        );
    }
}

fn kernel_log(s: &Snapshot, st: Style) -> String {
    let mut out = format!("{}\n", st.bold("Kernel log (this boot)"));
    let log = &s.kernel_log;
    if log.errors.is_empty() {
        let benign = match log.benign_count {
            0 => String::new(),
            n => format!(" ({n} known-benign warning lines)"),
        };
        let _ = writeln!(
            out,
            "  {}",
            st.dim(&format!("no USB, USB-C or link-training errors{benign}"))
        );
    } else {
        let skip = log.errors.len().saturating_sub(LOG_LINES_SHOWN);
        for line in &log.errors[skip..] {
            match line.secs_since_boot {
                Some(secs) => {
                    let mut text = format!(
                        "  {} {}",
                        st.dim(&format!("[{secs:>5}s]")),
                        truncate(&line.message, 120)
                    );
                    if let Some(socket) = &line.socket {
                        let _ = write!(text, "  ← {socket}");
                    }
                    if secs < AT_BOOT_SECS {
                        let _ = write!(text, " {}", st.dim("(at boot)"));
                    }
                    let _ = writeln!(out, "{text}");
                }
                None => {
                    let _ = writeln!(out, "  {}", truncate(&line.message, 160));
                }
            }
        }
    }
    let resets = link_reset_total(s);
    if resets > 0 {
        let _ = writeln!(
            out,
            "  {}",
            st.dim(&format!(
                "{resets} USB link reset(s): {}",
                link_resets_text(s)
            ))
        );
    }
    out
}

/// The Firmware section (`--firmware`): the running BIOS, the vendor
/// catalog's verdict, labelled capsule firmware and fwupd's pending updates.
fn firmware(fw: &Firmware, st: Style) -> String {
    let mut out = format!("{}\n", st.bold("Firmware"));
    let dmi = &fw.dmi;
    let mut line = format!("  BIOS: {}", dmi.bios_id().unwrap_or("?"));
    if let Some(rel) = &dmi.bios_release {
        let _ = write!(line, " ({rel})");
    }
    if let Some(date) = &dmi.bios_date {
        let _ = write!(line, "  {}", st.dim(date));
    }
    if let Some(ec) = &dmi.ec_firmware_release {
        let _ = write!(line, "  EC: {ec}");
    }
    let _ = writeln!(out, "{line}");

    let not_checked = "latest versions not checked";
    match &fw.catalog {
        Catalog::Lenovo(c) => {
            if let (Some(bios), Some(check)) = (&c.latest.bios, c.bios_check) {
                let text = match check {
                    BiosCheck::UpToDate => st.dim(&format!(
                        "latest BIOS in Lenovo's catalog: build {:02} — up to date",
                        bios.build
                    )),
                    BiosCheck::UpdateAvailable => {
                        let have = dmi
                            .bios_id()
                            .and_then(lenovo::bios_build)
                            .map_or_else(|| "?".to_owned(), |b| format!("{b:02}"));
                        st.yellow(&format!(
                            "Lenovo has BIOS build {:02} (you have {have})",
                            bios.build
                        ))
                    }
                    BiosCheck::NotCompared => {
                        st.dim("catalog BIOS package doesn't match this BIOS family — not compared")
                    }
                };
                let _ = writeln!(out, "  {text}");
            }
        }
        Catalog::Dell(c) => {
            let text = match c.bios_check {
                BiosCheck::UpToDate => st.dim(&format!(
                    "latest BIOS in Dell's catalog: {} — up to date",
                    c.bios.version
                )),
                BiosCheck::UpdateAvailable => {
                    let note = dell::release_note(&c.bios);
                    let note = if note.is_empty() {
                        String::new()
                    } else {
                        format!(" ({note})")
                    };
                    format!(
                        "{} {}",
                        st.yellow(&format!("Dell has BIOS {}{note}", c.bios.version)),
                        st.dim(&dell::driver_url(&c.bios.package_id))
                    )
                }
                BiosCheck::NotCompared => st.dim(&format!(
                    "latest BIOS in Dell's catalog: {} — this BIOS's version can't be read, \
                     not compared",
                    c.bios.version
                )),
            };
            let _ = writeln!(out, "  {text}");
        }
        Catalog::Unreachable { vendor, model } => {
            let _ = writeln!(
                out,
                "  {}",
                st.dim(&format!(
                    "{vendor} catalog for {model} unreachable (offline?) — {not_checked}"
                ))
            );
        }
        Catalog::Unreadable {
            vendor,
            model,
            error,
        } => {
            let _ = writeln!(
                out,
                "  {}",
                st.dim(&format!(
                    "can't read {vendor}'s catalog for {model} ({error}) — {not_checked}"
                ))
            );
        }
        Catalog::NotListed { vendor, model } => {
            let _ = writeln!(
                out,
                "  {}",
                st.dim(&format!(
                    "{vendor}'s catalog lists nothing for {model} — {not_checked}"
                ))
            );
        }
        Catalog::NotChecked => {}
    }

    match &fw.fwupd {
        Fwupd::NotInstalled => {
            let _ = writeln!(
                out,
                "  {}",
                st.dim("fwupd not installed — skipping capsule firmware checks")
            );
        }
        Fwupd::Checked(state) => {
            for d in &state.labelled {
                let hex = d
                    .version_number()
                    .map(|n| format!(" {}", st.dim(&format!("(0x{n:x})"))))
                    .unwrap_or_default();
                let _ = writeln!(out, "  {}: {}{hex}", d.label, d.version);
            }
            for p in &state.pending {
                let _ = writeln!(
                    out,
                    "  {} {}",
                    st.yellow("update:"),
                    health::pending_text(p)
                );
            }
            if state.pending.is_empty() {
                let _ = writeln!(
                    out,
                    "  {}",
                    st.dim(
                        "fwupd: no pending updates (cached metadata; fwupdmgr refresh to re-check)"
                    )
                );
            }
        }
    }
    out
}

/// The first `n` characters of `s`.
fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_by_character() {
        assert_eq!(truncate("héllo", 2), "hé");
        assert_eq!(truncate("hi", 5), "hi");
    }

    #[test]
    fn colour_only_when_asked() {
        assert_eq!(Style(false).red("x"), "x");
        assert_eq!(Style(true).red("x"), "\x1b[31mx\x1b[0m");
    }

    #[test]
    fn health_verdict_lines() {
        let ok = Finding {
            level: Level::Ok,
            message: "fine".into(),
        };
        assert_eq!(
            render_health(std::slice::from_ref(&ok), false),
            "Health\n  ✓ fine\n  → all good\n"
        );
        let fail = Finding {
            level: Level::Fail,
            message: "broken".into(),
        };
        assert_eq!(
            render_health(&[ok, fail], false),
            "Health\n  ✓ fine\n  ✗ broken\n  → something needs a look\n"
        );
    }
}
