//! The health verdict: a pure function from a [`Snapshot`] to findings, with
//! the `usb-ports` script's rules and order.
//!
//! Order: USB sockets (stuck hubs, over-current, then "USB 3 links up"),
//! USB-C (UCSI up or missing, stale status, charging), Thunderbolt, external
//! displays, power, CPU throttling, kernel log.

use crate::model::{
    DisplayKind, Finding, Level, Pdo, Snapshot, Socket, Speed, TypecPort, Usb3Side, UsbCSource,
    UsbDevice, Vrr,
};

/// Link resets this boot at which the cable or link is called flaky.
pub const LINK_RESET_WARN: u32 = 5;

/// A busy CPU (at least this % utilised) ...
pub const THROTTLE_BUSY_PCT: u8 = 50;
/// ... averaging at most this % of its minimum clock is being throttled.
pub const THROTTLE_FLOOR_PCT: u64 = 115;

pub fn check(s: &Snapshot) -> Vec<Finding> {
    let mut out = Findings::default();
    usb_sockets(s, &mut out);
    usb_c(s, &mut out);
    thunderbolt(s, &mut out);
    displays(s, &mut out);
    power(s, &mut out);
    cpu(s, &mut out);
    kernel_log(s, &mut out);
    out.0
}

/// Whether anything needs a look: the script exits 1 on a warning too.
pub fn is_bad(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.level != Level::Ok)
}

#[derive(Default)]
struct Findings(Vec<Finding>);

impl Findings {
    fn push(&mut self, level: Level, message: impl Into<String>) {
        self.0.push(Finding {
            level,
            message: message.into(),
        });
    }
    fn ok(&mut self, message: impl Into<String>) {
        self.push(Level::Ok, message);
    }
    fn warn(&mut self, message: impl Into<String>) {
        self.push(Level::Warn, message);
    }
    fn fail(&mut self, message: impl Into<String>) {
        self.push(Level::Fail, message);
    }
}

/// A socket's label, else its key.
pub fn socket_name(socket: &Socket) -> &str {
    socket.label.as_deref().unwrap_or(&socket.key)
}

/// A UCSI port's label, else its name.
pub fn port_name(port: &TypecPort) -> &str {
    port.label.as_deref().unwrap_or(&port.name)
}

/// A USB 3 hub (class 09) on the USB 2 half whose paired USB 3 half is idle:
/// its SuperSpeed link never came up (USB 2-only hubs are rare now).
pub fn hub_stuck_at_usb2(socket: &Socket) -> bool {
    let Usb3Side::Paired(usb3) = &socket.usb3 else {
        return false;
    };
    usb3.device.is_none()
        && socket
            .usb2
            .device
            .as_ref()
            .is_some_and(|d| d.device_class == 9)
}

/// Over-current events on the socket's root ports, both halves.
pub fn over_current(socket: &Socket) -> u32 {
    let usb3 = match &socket.usb3 {
        Usb3Side::Paired(half) => half.over_current_count.unwrap_or(0),
        _ => 0,
    };
    socket.usb2.over_current_count.unwrap_or(0) + usb3
}

/// How a device's link compares with the fastest speed it supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFit {
    /// Linked at the fastest speed it supports.
    Best,
    /// Linked below what it supports: a cable, a hub or the port limits it.
    Below { capable: Speed },
    /// A hub on the USB 2 half while its socket's USB 3 half has a hub
    /// linked: a USB 3 hub's USB 2 half carries the same BOS as its USB 3
    /// half, so it reads "capable 5G, linked 480M" even when healthy.
    Usb2HalfOfUsb3Hub,
    /// `capable_speed` is unknown (a USB 2 device at full or low speed).
    Unknown,
}

/// The capable-vs-linked verdict for `device`, found on `socket`'s USB 3
/// half when `on_usb3` is set, else on its USB 2 half.
pub fn link_fit(socket: &Socket, on_usb3: bool, device: &UsbDevice) -> LinkFit {
    let Some(capable) = device.capable_speed else {
        return LinkFit::Unknown;
    };
    if device.speed.mbps() >= capable.mbps() {
        return LinkFit::Best;
    }
    let usb3_hub_linked = matches!(&socket.usb3, Usb3Side::Paired(h)
        if h.device.as_ref().is_some_and(|d| d.device_class == 9));
    if !on_usb3 && device.device_class == 9 && usb3_hub_linked {
        return LinkFit::Usb2HalfOfUsb3Hub;
    }
    LinkFit::Below { capable }
}

impl LinkFit {
    /// One line for the detail view.
    pub fn text(self, linked: Speed) -> String {
        match self {
            Self::Best => format!("linked at its best speed ({})", linked.short()),
            Self::Below { capable } => format!(
                "{} capable, linked at {} → cable or hub limiting",
                capable.short(),
                linked.short()
            ),
            Self::Usb2HalfOfUsb3Hub => {
                "USB 2 half of a USB 3 hub (its USB 3 half is linked)".into()
            }
            Self::Unknown => format!(
                "can't be told: linked at {}, and a USB 2 device this slow may be \
                 full-speed only",
                linked.short()
            ),
        }
    }
}

fn usb_sockets(s: &Snapshot, out: &mut Findings) {
    let mut ss_seen = false;
    let mut ss_down = false;
    for socket in &s.sockets {
        let name = socket_name(socket);
        if matches!(&socket.usb3, Usb3Side::Paired(h) if h.device.is_some()) {
            ss_seen = true;
        }
        if hub_stuck_at_usb2(socket) {
            out.fail(format!(
                "{name}: hub stuck at USB 2 — the port's SuperSpeed lanes are down, so no \
                 USB 3 and no DP alt mode either. Flip the plug; if that fails, EC reset \
                 (power off, unplug all, hold power 15 s)"
            ));
            ss_down = true;
        }
        let oc = over_current(socket);
        if oc > 0 {
            out.fail(format!(
                "{name}: over-current on its root port ({oc}×) — a device or cable drew \
                 too much or shorted; unplug it and check the cable"
            ));
        }
    }
    if ss_seen && !ss_down {
        out.ok("USB 3 links up (SuperSpeed lanes working)");
    }
}

fn usb_c(s: &Snapshot, out: &mut Findings) {
    if !s.typec.ucsi_present {
        out.fail(
            "USB-C status driver (UCSI) didn't init — Linux can't see USB-C state. \
             Video still works; fix: pkexec sh -c 'modprobe -r ucsi_acpi && modprobe ucsi_acpi', \
             or reboot",
        );
        return;
    }
    out.ok(format!(
        "USB-C status driver (UCSI) up: {} port(s)",
        s.typec.ports.len()
    ));
    for port in &s.typec.ports {
        if usb_c_status_stale(port) {
            out.warn(format!(
                "USB-C status for {} looks stale (charging state and advertised power \
                 disagree); replug the cable to refresh",
                port_name(port)
            ));
        }
    }
}

/// The partner still advertises source PDOs, but the port's supply is
/// offline and UCSI calls it a sink: PDOs left over from an earlier partner
/// state (the XPS dock incident, 2026-10-16).
pub fn usb_c_status_stale(port: &TypecPort) -> bool {
    port.pd_inconsistent && !port.charging && has_source_pdos(port)
}

/// Power comes in through the port, but the partner never reported its
/// source PDOs. Some chargers and docks don't; that's not stale data.
pub fn offers_unreported(port: &TypecPort) -> bool {
    port.charging && !has_source_pdos(port)
}

fn has_source_pdos(port: &TypecPort) -> bool {
    port.partner
        .as_ref()
        .is_some_and(|p| !p.source_pdos.is_empty())
}

/// The most the partner's source PDOs offer, in W.
pub fn best_offer_w(port: &TypecPort) -> Option<f32> {
    port.partner
        .as_ref()?
        .source_pdos
        .iter()
        .filter_map(Pdo::max_watts)
        .reduce(f32::max)
}

/// Thousandths as a short decimal: 20000 → `20`, 2350 → `2.35`.
pub fn milli(n: u32) -> String {
    format!("{}", f64::from(n) / 1000.0)
}

fn thunderbolt(s: &Snapshot, out: &mut Findings) {
    let Some(tb) = &s.thunderbolt else { return };
    let unauthorized: Vec<String> = tb
        .devices
        .iter()
        .filter(|d| !d.authorized)
        .map(|d| format!("{} ({})", d.name.as_deref().unwrap_or("?"), d.route))
        .collect();
    if !unauthorized.is_empty() {
        out.warn(format!(
            "Thunderbolt device(s) not authorized, so no PCIe tunnels: {} — \
             boltctl authorize <uuid> (or enroll to make it stick)",
            unauthorized.join(", ")
        ));
        return;
    }
    let mut security: Vec<&str> = Vec::new();
    for d in &tb.domains {
        let sec = d.security.as_deref().unwrap_or("?");
        if !security.contains(&sec) {
            security.push(sec);
        }
    }
    out.ok(format!(
        "Thunderbolt/USB4: {} domain(s), security {}, {} device(s) attached",
        tb.domains.len(),
        security.join(" "),
        tb.devices.len()
    ));
}

fn displays(s: &Snapshot, out: &mut Findings) {
    let dp = s.displays.iter().any(|d| d.kind == DisplayKind::Dp);
    let hdmi: Vec<_> = s
        .displays
        .iter()
        .filter(|d| d.kind == DisplayKind::Hdmi)
        .collect();
    if dp {
        out.ok("external display on USB-C (DP alt mode working)");
    } else if !hdmi.is_empty() {
        out.ok("external display on HDMI");
        if let Some(d) = hdmi.iter().find(|d| d.vrr == Some(Vrr::On)) {
            out.warn(format!(
                "VRR is always-on over HDMI ({}) — some monitors flicker with it; if this \
                 one does, set 'variable-refresh-rate on-demand=true' on its niri output",
                d.connector
            ));
        }
    }
    // A USB-C partner that isn't feeding us power, with no DP lit: a monitor
    // there means alt mode never came up (the 2026-09-27 failure).
    let sinks: Vec<&str> = s
        .typec
        .ports
        .iter()
        .filter(|p| p.partner.is_some() && !p.charging)
        .map(port_name)
        .collect();
    if !sinks.is_empty() && !dp {
        out.warn(format!(
            "USB-C device on {} but no DisplayPort output — if it's a monitor, alt mode \
             failed: check the cable on another machine, then EC reset",
            sinks.join(", ")
        ));
    }
}

/// `port1 (USB-C (right)) 20V 2.35A ≈ 47W, …`: the script's list of online
/// USB-C supplies, labelled from the matching UCSI port.
pub fn usb_c_sources_text(s: &Snapshot) -> String {
    s.power
        .usb_c_sources
        .iter()
        .map(|src| source_text(s, src))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn source_text(s: &Snapshot, src: &UsbCSource) -> String {
    source_with_notes(s, src, Vec::new())
}

/// The port and its label, then `20V 2.35A ≈ 47W`; or, when the supply
/// reports no voltage (Lenovo's UCSI), ` — power level not reported` with
/// the current it does report, since watts can't be told without a
/// voltage. `notes` go in the same parentheses as that current.
fn source_with_notes(s: &Snapshot, src: &UsbCSource, mut notes: Vec<String>) -> String {
    let mut text = src.typec_port.clone();
    let label = s
        .typec
        .ports
        .iter()
        .find(|p| p.name == src.typec_port)
        .and_then(|p| p.label.as_deref());
    if let Some(label) = label {
        text.push_str(&format!(" ({label})"));
    }
    let current = src.current_ma.filter(|&a| a > 0);
    if let (Some(v), Some(a), Some(w)) = (src.voltage_mv, current, src.watts()) {
        text.push_str(&format!(" {}V {}A ≈ {w:.0}W", milli(v), milli(a)));
    } else {
        text.push_str(" — power level not reported");
        if let Some(a) = current {
            notes.insert(0, format!("drawing {} A", milli(a)));
        }
    }
    if !notes.is_empty() {
        text.push_str(&format!(" ({})", notes.join("; ")));
    }
    text
}

/// `best offer 47 W`, or `charger's offers not reported` when the partner
/// on that port lists no source PDOs.
fn offer_note(s: &Snapshot, src: &UsbCSource) -> Option<String> {
    let port = s.typec.ports.iter().find(|p| p.name == src.typec_port)?;
    Some(match best_offer_w(port) {
        Some(w) => format!("best offer {w:.0} W"),
        None => "charger's offers not reported".to_owned(),
    })
}

fn power(s: &Snapshot, out: &mut Findings) {
    if s.power.ac_online != Some(true) {
        return;
    }
    if s.power.usb_c_sources.is_empty() {
        out.ok("on AC power (via a port UCSI doesn't report, e.g. a hub's charging pass-through)");
    } else {
        let sources: Vec<String> = s
            .power
            .usb_c_sources
            .iter()
            .map(|src| source_with_notes(s, src, offer_note(s, src).into_iter().collect()))
            .collect();
        out.ok(format!(
            "charging power arriving via USB-C {}",
            sources.join(", ")
        ));
    }
}

/// Whether the CPU sample shows a busy CPU held near its minimum clock.
/// `None` when there is no sample with clocks, or the CPU is mostly idle
/// (then it can't tell).
pub fn cpu_throttled(s: &Snapshot) -> Option<bool> {
    let cpu = s.cpu.as_ref().filter(|c| c.avg_khz > 0)?;
    if cpu.util_pct < THROTTLE_BUSY_PCT {
        return None;
    }
    Some(
        cpu.min_khz
            .is_some_and(|min| u64::from(cpu.avg_khz) <= u64::from(min) * THROTTLE_FLOOR_PCT / 100),
    )
}

fn cpu(s: &Snapshot, out: &mut Findings) {
    let Some(throttled) = cpu_throttled(s) else {
        return;
    };
    let Some(cpu) = &s.cpu else { return };
    let util = cpu.util_pct;
    if throttled {
        if s.power.ac_online == Some(true) {
            let pkg = cpu
                .package_w
                .map(|w| format!(" ({w:.0} W package)"))
                .unwrap_or_default();
            out.fail(format!(
                "CPU pinned at its minimum clock while {util}% busy on AC{pkg} — the EC is \
                 throttling (BD PROCHOT), usually a charger, cable or dock it doesn't trust. \
                 Plug the charger straight into the laptop, or try another charger"
            ));
        } else {
            out.fail(format!(
                "CPU pinned at its minimum clock while {util}% busy on battery — BD PROCHOT \
                 or a low-battery limit; check the power profile and the battery"
            ));
        }
    } else {
        let pkg = cpu
            .package_w
            .map(|w| format!(", {w:.0} W package"))
            .unwrap_or_default();
        out.ok(format!(
            "CPU not power-throttled ({util}% busy at {:.1} GHz{pkg})",
            f64::from(cpu.avg_khz) / 1e6
        ));
    }
}

/// `USB-C (left) ×3, port 3-2 ×2`, by place.
pub fn link_resets_text(s: &Snapshot) -> String {
    s.kernel_log
        .link_resets
        .iter()
        .map(|r| format!("{} ×{}", r.place, r.count))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn link_reset_total(s: &Snapshot) -> u32 {
    s.kernel_log.link_resets.iter().map(|r| r.count).sum()
}

fn kernel_log(s: &Snapshot, out: &mut Findings) {
    let errors = s.kernel_log.errors.len();
    if errors > 0 {
        out.fail(format!(
            "{errors} USB/USB-C/display error line(s) in the kernel log — see above, or \
             journalctl -k -b"
        ));
    } else {
        out.ok("kernel log clean");
    }
    let resets = link_reset_total(s);
    if resets >= LINK_RESET_WARN {
        out.warn(format!(
            "{resets} USB link resets this boot on {} — flaky cable/dock or a marginal \
             SuperSpeed link; reseat or swap the cable",
            link_resets_text(s)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn snap() -> Snapshot {
        Snapshot {
            taken_at_ms: 0,
            host: "test".into(),
            sockets: Vec::new(),
            typec: TypecState {
                ucsi_present: true,
                ports: Vec::new(),
            },
            thunderbolt: None,
            displays: Vec::new(),
            power: Power {
                ac_online: Some(false),
                battery: None,
                usb_c_sources: Vec::new(),
            },
            cpu: None,
            kernel_log: KernelLog {
                errors: Vec::new(),
                benign_count: 0,
                link_resets: Vec::new(),
            },
        }
    }

    fn device(name: &str, speed: Speed, class: u8) -> UsbDevice {
        UsbDevice {
            sysfs_name: name.into(),
            vendor_id: 0x05e3,
            product_id: 0x0626,
            manufacturer: None,
            product: Some("Hub".into()),
            serial: None,
            speed,
            capable_speed: None,
            rx_lanes: None,
            tx_lanes: None,
            bcd_usb: None,
            device_class: class,
            max_power_ma: None,
            driver: None,
            urbnum: None,
            children: Vec::new(),
        }
    }

    fn half(bus: u32, device: Option<UsbDevice>) -> Half {
        Half {
            bus,
            port: 1,
            device,
            over_current_count: Some(0),
            port_state: None,
            physical_location: None,
        }
    }

    fn socket(usb2: Option<UsbDevice>, usb3: Usb3Side) -> Socket {
        Socket {
            key: "0000:00:14.0-1".into(),
            label: Some("USB-C (left)".into()),
            usb2: half(3, usb2),
            usb3,
            typec_port: None,
        }
    }

    fn port(name: &str, partner: Option<Partner>, charging: bool) -> TypecPort {
        TypecPort {
            name: name.into(),
            label: Some(format!("USB-C ({name})")),
            partner,
            cable_type: None,
            power_role: None,
            charging,
            pd_inconsistent: false,
            pd_contract: None,
        }
    }

    fn partner(source_pdos: Vec<Pdo>) -> Partner {
        Partner {
            supports_pd: Some(true),
            pd_revision: None,
            alt_modes: 0,
            source_pdos,
            sink_pdos: Vec::new(),
        }
    }

    fn display(connector: &str, kind: DisplayKind, vrr: Option<Vrr>) -> Display {
        Display {
            connector: connector.into(),
            kind,
            mode: None,
            vrr,
        }
    }

    fn cpu_sample(util_pct: u8, avg_khz: u32, min_khz: Option<u32>) -> CpuSample {
        CpuSample {
            util_pct,
            avg_khz,
            max_khz: avg_khz,
            min_khz,
            package_w: Some(8.0),
            package_temp_c: None,
        }
    }

    fn levels_with(s: &Snapshot, needle: &str) -> Vec<Level> {
        check(s)
            .into_iter()
            .filter(|f| f.message.contains(needle))
            .map(|f| f.level)
            .collect()
    }

    fn has(s: &Snapshot, level: Level, needle: &str) -> bool {
        levels_with(s, needle).contains(&level)
    }

    #[test]
    fn usb3_links_up() {
        let mut s = snap();
        s.sockets.push(socket(
            Some(device("3-1", Speed::High, 9)),
            Usb3Side::Paired(half(2, Some(device("2-1", Speed::Super, 9)))),
        ));
        assert!(has(&s, Level::Ok, "USB 3 links up"));
        assert!(levels_with(&s, "stuck at USB 2").is_empty());
    }

    #[test]
    fn no_usb3_ok_without_a_superspeed_link() {
        let mut s = snap();
        s.sockets
            .push(socket(None, Usb3Side::Paired(half(2, None))));
        assert!(levels_with(&s, "USB 3 links up").is_empty());
    }

    #[test]
    fn hub_stuck_at_usb2() {
        let mut s = snap();
        s.sockets.push(socket(
            Some(device("3-1", Speed::High, 9)),
            Usb3Side::Paired(half(2, None)),
        ));
        let findings = check(&s);
        let stuck: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("hub stuck at USB 2"))
            .collect();
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].level, Level::Fail);
        assert!(stuck[0].message.starts_with("USB-C (left): "));
        assert!(stuck[0].message.contains("Flip the plug"));
        assert!(stuck[0].message.contains("EC reset"));
    }

    #[test]
    fn capable_vs_linked() {
        let at = |speed, capable| UsbDevice {
            capable_speed: capable,
            ..device("2-1", speed, 0)
        };
        let s = socket(None, Usb3Side::Paired(half(2, None)));
        // A 10G device on a 5G link: something limits it.
        assert_eq!(
            link_fit(&s, true, &at(Speed::Super, Some(Speed::SuperPlus))),
            LinkFit::Below {
                capable: Speed::SuperPlus
            }
        );
        assert_eq!(
            LinkFit::Below {
                capable: Speed::SuperPlus
            }
            .text(Speed::Super),
            "10G capable, linked at 5G → cable or hub limiting"
        );
        assert_eq!(
            link_fit(&s, true, &at(Speed::Super, Some(Speed::Super))),
            LinkFit::Best
        );
        // Full-speed USB 2 devices have no capable speed.
        assert_eq!(
            link_fit(&s, false, &at(Speed::Full, None)),
            LinkFit::Unknown
        );

        // A USB 3 hub's USB 2 half reads "capable 5G, linked 480M" while its
        // USB 3 half is linked: not flagged.
        let usb2_hub = UsbDevice {
            capable_speed: Some(Speed::Super),
            ..device("3-1", Speed::High, 9)
        };
        let linked = socket(
            Some(usb2_hub.clone()),
            Usb3Side::Paired(half(2, Some(device("2-1", Speed::Super, 9)))),
        );
        assert_eq!(
            link_fit(&linked, false, &usb2_hub),
            LinkFit::Usb2HalfOfUsb3Hub
        );
        // The same hub with its USB 3 half idle is stuck: flagged.
        let stuck = socket(Some(usb2_hub.clone()), Usb3Side::Paired(half(2, None)));
        assert_eq!(
            link_fit(&stuck, false, &usb2_hub),
            LinkFit::Below {
                capable: Speed::Super
            }
        );
        // A non-hub device on the USB 2 half is flagged even then.
        let dev = UsbDevice {
            capable_speed: Some(Speed::Super),
            ..device("3-1", Speed::High, 0)
        };
        assert_eq!(
            link_fit(&linked, false, &dev),
            LinkFit::Below {
                capable: Speed::Super
            }
        );
    }

    #[test]
    fn hub_stuck_needs_a_hub_and_a_paired_half() {
        // A non-hub USB 2 device on its own is fine.
        let mut s = snap();
        s.sockets.push(socket(
            Some(device("3-1", Speed::High, 0)),
            Usb3Side::Paired(half(2, None)),
        ));
        assert!(levels_with(&s, "stuck").is_empty());
        // A hub where the USB 3 half isn't known can't be judged.
        let mut s = snap();
        s.sockets.push(socket(
            Some(device("3-1", Speed::High, 9)),
            Usb3Side::Unknown,
        ));
        assert!(levels_with(&s, "stuck").is_empty());
    }

    #[test]
    fn over_current_fails_naming_the_socket() {
        let mut s = snap();
        let mut usb3 = half(2, None);
        usb3.over_current_count = Some(2);
        s.sockets.push(socket(None, Usb3Side::Paired(usb3)));
        let oc = levels_with(&s, "USB-C (left): over-current");
        assert_eq!(oc, vec![Level::Fail]);
        assert!(check(&s).iter().any(|f| f.message.contains("(2×)")));

        let mut s = snap();
        s.sockets
            .push(socket(None, Usb3Side::Paired(half(2, None))));
        assert!(levels_with(&s, "over-current").is_empty());
    }

    #[test]
    fn ucsi_up_and_missing() {
        let mut s = snap();
        s.typec.ports.push(port("port0", None, false));
        assert!(has(&s, Level::Ok, "UCSI) up: 1 port(s)"));

        let mut s = snap();
        s.typec.ucsi_present = false;
        let f = check(&s);
        let ucsi = f.iter().find(|f| f.message.contains("UCSI")).unwrap();
        assert_eq!(ucsi.level, Level::Fail);
        assert!(ucsi.message.contains("modprobe -r ucsi_acpi"));
        assert!(!f.iter().any(|f| f.message.contains("UCSI) up")));
    }

    #[test]
    fn stale_usb_c_status_warns() {
        // Case A: the partner still lists source PDOs, the supply is offline.
        let mut s = snap();
        let mut p = port(
            "port0",
            Some(partner(vec![Pdo::Fixed {
                mv: 20_000,
                ma: 2880,
            }])),
            false,
        );
        p.pd_inconsistent = true;
        s.typec.ports.push(p);
        assert!(usb_c_status_stale(&s.typec.ports[0]));
        let f = check(&s);
        let stale = f
            .iter()
            .find(|f| f.message.contains("looks stale"))
            .unwrap();
        assert_eq!(stale.level, Level::Warn);
        assert!(stale.message.contains("USB-C (port0)"));
        assert!(stale.message.contains("replug"));

        s.typec.ports[0].pd_inconsistent = false;
        assert!(levels_with(&s, "stale").is_empty());
    }

    #[test]
    fn charging_without_reported_offers_is_not_stale() {
        // Case B: power comes in, the partner never listed source PDOs.
        let mut s = snap();
        let mut p = port("port0", Some(partner(Vec::new())), true);
        p.pd_inconsistent = true;
        s.typec.ports.push(p);
        assert!(!usb_c_status_stale(&s.typec.ports[0]));
        assert!(offers_unreported(&s.typec.ports[0]));
        assert!(levels_with(&s, "stale").is_empty());
        assert!(!is_bad(&check(&s)));
    }

    fn usb_c_charger(pdos: Vec<Pdo>) -> Snapshot {
        let mut s = snap();
        s.power.ac_online = Some(true);
        let mut p = port("port1", Some(partner(pdos)), true);
        p.label = Some("USB-C (right)".into());
        s.typec.ports.push(p);
        s.power.usb_c_sources.push(UsbCSource {
            typec_port: "port1".into(),
            voltage_mv: Some(20_000),
            current_ma: Some(2350),
        });
        s
    }

    #[test]
    fn charging_finding_names_the_best_offer() {
        let s = usb_c_charger(vec![
            Pdo::Fixed { mv: 5000, ma: 3000 },
            Pdo::Fixed {
                mv: 20_000,
                ma: 3250,
            },
        ]);
        let f = check(&s);
        assert!(
            f.contains(&Finding {
                level: Level::Ok,
                message: "charging power arriving via USB-C port1 (USB-C (right)) 20V 2.35A ≈ 47W \
                      (best offer 65 W)"
                    .into()
            })
        );
        // One charging finding, not two.
        assert_eq!(levels_with(&s, "charging").len(), 1);
    }

    #[test]
    fn charging_finding_when_offers_are_not_reported() {
        let s = usb_c_charger(Vec::new());
        assert!(
            check(&s).contains(&Finding {
                level: Level::Ok,
                message: "charging power arriving via USB-C port1 (USB-C (right)) 20V 2.35A ≈ 47W \
                      (charger's offers not reported)"
                    .into()
            })
        );
    }

    #[test]
    fn charging_finding_when_power_level_is_not_reported() {
        let mut s = usb_c_charger(Vec::new());
        s.power.usb_c_sources[0].voltage_mv = None;
        s.power.usb_c_sources[0].current_ma = Some(3250);
        let ok = |message: &str| Finding {
            level: Level::Ok,
            message: message.into(),
        };
        assert!(check(&s).contains(&ok(
            "charging power arriving via USB-C port1 (USB-C (right)) — power level not reported \
             (drawing 3.25 A; charger's offers not reported)"
        )));
        assert_eq!(
            usb_c_sources_text(&s),
            "port1 (USB-C (right)) — power level not reported (drawing 3.25 A)"
        );
        s.power.usb_c_sources[0].current_ma = None;
        assert!(check(&s).contains(&ok(
            "charging power arriving via USB-C port1 (USB-C (right)) — power level not reported \
             (charger's offers not reported)"
        )));
    }

    fn tb(authorized: bool) -> Thunderbolt {
        Thunderbolt {
            domains: vec![
                TbDomain {
                    index: 0,
                    security: Some("user".into()),
                    iommu_dma_protection: true,
                    controller_vendor: None,
                    controller_device: None,
                    generation: Some(4),
                },
                TbDomain {
                    index: 1,
                    security: Some("user".into()),
                    iommu_dma_protection: true,
                    controller_vendor: None,
                    controller_device: None,
                    generation: Some(4),
                },
            ],
            devices: vec![TbDevice {
                route: "0-1".into(),
                unique_id: None,
                vendor: Some("Dell".into()),
                name: Some("Dock".into()),
                authorized,
                generation: Some(3),
                link: None,
                boltd_stored: None,
            }],
        }
    }

    #[test]
    fn thunderbolt_ok_and_unauthorized() {
        let mut s = snap();
        s.thunderbolt = Some(tb(true));
        assert!(check(&s).contains(&Finding {
            level: Level::Ok,
            message: "Thunderbolt/USB4: 2 domain(s), security user, 1 device(s) attached".into()
        }));

        s.thunderbolt = Some(tb(false));
        let f = check(&s);
        let w = f
            .iter()
            .find(|f| f.message.contains("Thunderbolt"))
            .unwrap();
        assert_eq!(w.level, Level::Warn);
        assert!(w.message.contains("not authorized"));
        assert!(w.message.contains("Dock (0-1)"));

        s.thunderbolt = None;
        assert!(levels_with(&s, "Thunderbolt").is_empty());
    }

    #[test]
    fn external_display_dp_and_hdmi() {
        let mut s = snap();
        s.displays.push(display("DP-1", DisplayKind::Dp, None));
        s.displays
            .push(display("HDMI-A-1", DisplayKind::Hdmi, Some(Vrr::On)));
        assert!(has(&s, Level::Ok, "DP alt mode working"));
        // DP wins, as in the script: no HDMI findings then.
        assert!(levels_with(&s, "HDMI").is_empty());

        let mut s = snap();
        s.displays
            .push(display("HDMI-A-1", DisplayKind::Hdmi, Some(Vrr::Off)));
        assert!(has(&s, Level::Ok, "external display on HDMI"));
        assert!(levels_with(&s, "VRR").is_empty());

        s.displays[0].vrr = Some(Vrr::On);
        let vrr = check(&s)
            .into_iter()
            .find(|f| f.message.contains("VRR"))
            .unwrap();
        assert_eq!(vrr.level, Level::Warn);
        assert!(vrr.message.contains("on-demand=true"));
        assert!(!vrr.message.contains("RTK"));

        let s = snap();
        assert!(levels_with(&s, "external display").is_empty());
    }

    #[test]
    fn alt_mode_failed_when_a_non_charging_partner_and_no_dp() {
        let mut s = snap();
        s.typec
            .ports
            .push(port("port0", Some(partner(Vec::new())), false));
        let f = check(&s);
        let w = f
            .iter()
            .find(|f| f.message.contains("no DisplayPort output"))
            .unwrap();
        assert_eq!(w.level, Level::Warn);
        assert!(w.message.contains("USB-C (port0)"));

        // With DP lit, or when the partner is the charger, no warning.
        s.displays.push(display("DP-1", DisplayKind::Dp, None));
        assert!(levels_with(&s, "no DisplayPort").is_empty());
        let mut s = snap();
        s.typec
            .ports
            .push(port("port0", Some(partner(Vec::new())), true));
        assert!(levels_with(&s, "no DisplayPort").is_empty());
    }

    #[test]
    fn ac_via_usb_c_or_unreported_port() {
        let mut s = snap();
        s.power.ac_online = Some(true);
        assert!(has(&s, Level::Ok, "via a port UCSI doesn't report"));

        let mut s = usb_c_charger(Vec::new());
        assert!(has(&s, Level::Ok, "arriving via USB-C port1"));
        assert!(levels_with(&s, "UCSI doesn't report").is_empty());

        s.power.ac_online = Some(false);
        assert!(levels_with(&s, "AC power").is_empty());
        assert!(levels_with(&s, "arriving").is_empty());
    }

    #[test]
    fn cpu_throttled_on_ac_and_battery() {
        let mut s = snap();
        s.power.ac_online = Some(true);
        s.cpu = Some(cpu_sample(100, 400_000, Some(400_000)));
        let f = check(&s);
        let t = f.iter().find(|f| f.message.contains("CPU")).unwrap();
        assert_eq!(t.level, Level::Fail);
        assert!(t.message.contains("100% busy on AC (8 W package)"));
        assert!(t.message.contains("BD PROCHOT"));

        s.power.ac_online = Some(false);
        let f = check(&s);
        let t = f.iter().find(|f| f.message.contains("CPU")).unwrap();
        assert_eq!(t.level, Level::Fail);
        assert!(t.message.contains("busy on battery"));
    }

    #[test]
    fn cpu_threshold_is_115_percent_of_min() {
        let mut s = snap();
        s.cpu = Some(cpu_sample(50, 460_000, Some(400_000)));
        assert!(has(&s, Level::Fail, "minimum clock"));
        s.cpu = Some(cpu_sample(50, 461_000, Some(400_000)));
        assert!(has(
            &s,
            Level::Ok,
            "CPU not power-throttled (50% busy at 0.5 GHz"
        ));
    }

    #[test]
    fn cpu_not_throttled_and_idle() {
        let mut s = snap();
        s.cpu = Some(cpu_sample(80, 1_700_000, Some(400_000)));
        assert!(s.cpu.is_some());
        assert!(check(&s).contains(&Finding {
            level: Level::Ok,
            message: "CPU not power-throttled (80% busy at 1.7 GHz, 8 W package)".into()
        }));

        // Mostly idle: nothing to judge (the report says so instead).
        s.cpu = Some(cpu_sample(49, 400_000, Some(400_000)));
        assert!(levels_with(&s, "CPU").is_empty());
        // No minimum clock known: not throttled.
        s.cpu = Some(cpu_sample(90, 400_000, None));
        assert!(has(&s, Level::Ok, "CPU not power-throttled"));
        // No CPU sample, or no clocks.
        s.cpu = None;
        assert!(levels_with(&s, "CPU").is_empty());
        s.cpu = Some(cpu_sample(90, 0, Some(400_000)));
        assert!(levels_with(&s, "CPU").is_empty());
    }

    #[test]
    fn kernel_log_errors_and_clean() {
        let mut s = snap();
        assert!(has(&s, Level::Ok, "kernel log clean"));
        s.kernel_log.errors.push(LogLine {
            secs_since_boot: Some(3),
            message: "usb 3-1: device descriptor read/64, error -71".into(),
            socket: None,
        });
        let f = check(&s);
        assert!(!f.iter().any(|f| f.message == "kernel log clean"));
        let e = f.iter().find(|f| f.message.contains("error line")).unwrap();
        assert_eq!(e.level, Level::Fail);
        assert!(e.message.starts_with("1 USB/USB-C/display error line(s)"));
    }

    #[test]
    fn link_resets_warn_at_five() {
        let mut s = snap();
        s.kernel_log.link_resets = vec![
            LinkResets {
                place: "USB-C (left)".into(),
                count: 3,
            },
            LinkResets {
                place: "port 3-2".into(),
                count: 1,
            },
        ];
        assert!(levels_with(&s, "link resets").is_empty());
        s.kernel_log.link_resets[1].count = 2;
        let f = check(&s);
        let w = f
            .iter()
            .find(|f| f.message.contains("link resets"))
            .unwrap();
        assert_eq!(w.level, Level::Warn);
        assert!(
            w.message
                .starts_with("5 USB link resets this boot on USB-C (left) ×3, port 3-2 ×2 —")
        );
    }

    #[test]
    fn order_follows_the_script() {
        let mut s = snap();
        s.sockets.push(socket(
            Some(device("3-1", Speed::High, 9)),
            Usb3Side::Paired(half(2, None)),
        ));
        s.typec
            .ports
            .push(port("port0", Some(partner(Vec::new())), false));
        s.thunderbolt = Some(tb(true));
        s.power.ac_online = Some(true);
        s.cpu = Some(cpu_sample(90, 2_000_000, Some(400_000)));
        let messages: Vec<String> = check(&s).into_iter().map(|f| f.message).collect();
        let at = |needle: &str| {
            messages
                .iter()
                .position(|m| m.contains(needle))
                .unwrap_or_else(|| panic!("{needle} not in {messages:#?}"))
        };
        let order = [
            at("hub stuck"),
            at("UCSI) up"),
            at("Thunderbolt/USB4"),
            at("no DisplayPort output"),
            at("on AC power"),
            at("CPU not power-throttled"),
            at("kernel log clean"),
        ];
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{messages:#?}");
    }

    #[test]
    fn bad_means_any_warn_or_fail() {
        let ok = Finding {
            level: Level::Ok,
            message: String::new(),
        };
        let warn = Finding {
            level: Level::Warn,
            ..ok.clone()
        };
        assert!(!is_bad(std::slice::from_ref(&ok)));
        assert!(is_bad(&[ok, warn]));
        assert!(!is_bad(&[]));
    }
}
