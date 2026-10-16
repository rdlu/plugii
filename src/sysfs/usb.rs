//! USB root ports, USB 2/USB 3 pairing and device trees (milestone 2).
//!
//! Mirrors the `usb-ports` script: every root port whose `connect_type` is
//! `hotplug` is a physical socket, visited once from its USB 2 (480M root hub)
//! side. Its USB 3 half comes from the kernel's `peer` link, else from a
//! `peer:` label naming the SuperSpeed root port on the same controller.

use std::fs;
use std::path::{Path, PathBuf};

use super::{Root, entries, link_name, read, read_bytes, read_hex, read_parse};
use crate::labels::Labels;
use crate::model::{Half, PhysicalLocation, Socket, Speed, Usb3Side, UsbDevice};

/// Every hotplug socket, ordered by its USB 2 root port (bus, then port).
pub fn read_sockets(root: &Root, labels: &Labels) -> Vec<Socket> {
    let devices = root.path("sys/bus/usb/devices");
    let hubs = root_hubs(&devices);
    let ports: Vec<RootPort> = hubs.iter().flat_map(|h| root_ports(&devices, h)).collect();
    // With peer links on the machine, a port without one is really USB
    // 2-only; with none at all, firmware just doesn't describe them.
    let any_peer = ports
        .iter()
        .any(|p| p.dir.join("peer").symlink_metadata().is_ok());

    let mut sockets = Vec::new();
    for port in &ports {
        let Some(hub) = hubs.iter().find(|h| h.bus == port.bus) else {
            continue;
        };
        if hub.speed != Some(Speed::High)
            || read(port.dir.join("connect_type")).as_deref() != Some("hotplug")
        {
            continue;
        }
        let Some(pci) = &hub.pci else { continue };
        let key = format!("{pci}-{}", port.port);
        let usb3 = match usb3_port(&devices, &hubs, port, labels.peer_of(&key)) {
            Some(peer) => Usb3Side::Paired(read_half(&devices, &peer, labels)),
            None if any_peer => Usb3Side::Usb2Only,
            None => Usb3Side::Unknown,
        };
        sockets.push(Socket {
            label: labels.socket_label(&key).map(str::to_owned),
            typec_port: labels.typec_port_for_socket(&key).map(str::to_owned),
            usb2: read_half(&devices, port, labels),
            usb3,
            key,
        });
    }
    sockets
}

/// A root hub: `usbN`.
#[derive(Debug)]
struct RootHub {
    bus: u32,
    speed: Option<Speed>,
    /// The PCI address of the controller it sits on.
    pci: Option<String>,
}

/// `usbN-portM` under a root hub's interface.
#[derive(Debug, Clone)]
struct RootPort {
    bus: u32,
    port: u32,
    dir: PathBuf,
}

fn root_hubs(devices: &Path) -> Vec<RootHub> {
    let mut hubs: Vec<RootHub> = entries(devices, |n| bus_of_hub(n).is_some())
        .into_iter()
        .filter_map(|dir| {
            let bus = bus_of_hub(&dir.file_name()?.to_string_lossy())?;
            Some(RootHub {
                bus,
                speed: read(dir.join("speed")).and_then(|s| Speed::from_sysfs(&s)),
                pci: parent_name(&dir),
            })
        })
        .collect();
    hubs.sort_by_key(|h| h.bus);
    hubs
}

/// `usb3` → 3.
fn bus_of_hub(name: &str) -> Option<u32> {
    let n = name.strip_prefix("usb")?;
    n.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| n.parse().ok())?
}

/// The name of the directory a sysfs device sits in, with symlinks resolved:
/// a root hub's PCI controller.
fn parent_name(dir: &Path) -> Option<String> {
    let real = fs::canonicalize(dir).ok()?;
    Some(real.parent()?.file_name()?.to_string_lossy().into_owned())
}

fn root_ports(devices: &Path, hub: &RootHub) -> Vec<RootPort> {
    let bus = hub.bus;
    let prefix = format!("usb{bus}-port");
    let mut ports: Vec<RootPort> = entries(devices.join(format!("usb{bus}/{bus}-0:1.0")), |n| {
        n.strip_prefix(&prefix)
            .is_some_and(|p| p.parse::<u32>().is_ok())
    })
    .into_iter()
    .filter_map(|dir| {
        let port = dir
            .file_name()?
            .to_str()?
            .strip_prefix(&prefix)?
            .parse()
            .ok()?;
        Some(RootPort { bus, port, dir })
    })
    .collect();
    ports.sort_by_key(|p| p.port);
    ports
}

/// The SuperSpeed root port paired with this USB 2 one: the kernel's `peer`
/// link, else the `peer:` label's `<pci-address>-<port>`.
fn usb3_port(
    devices: &Path,
    hubs: &[RootHub],
    usb2: &RootPort,
    peer_label: Option<&str>,
) -> Option<RootPort> {
    let link = usb2.dir.join("peer");
    // The script tests `-e`, so a dangling link doesn't count.
    if link.exists() {
        let real = fs::canonicalize(&link).ok()?;
        let name = real.file_name()?.to_str()?;
        let (bus, port) = name.strip_prefix("usb")?.split_once("-port")?;
        return Some(RootPort {
            bus: bus.parse().ok()?,
            port: port.parse().ok()?,
            dir: real,
        });
    }
    let (pci, port) = peer_label?.rsplit_once('-')?;
    let port: u32 = port.parse().ok()?;
    let bus = ss_bus(hubs, pci)?;
    Some(RootPort {
        bus,
        port,
        dir: devices.join(format!("usb{bus}/{bus}-0:1.0/usb{bus}-port{port}")),
    })
}

/// The SuperSpeed root hub's bus number on PCI controller `pci`. A controller
/// has one USB 2 and one USB 3 root hub.
fn ss_bus(hubs: &[RootHub], pci: &str) -> Option<u32> {
    hubs.iter()
        .find(|h| h.pci.as_deref() == Some(pci) && h.speed.is_some_and(Speed::is_superspeed))
        .map(|h| h.bus)
}

fn read_half(devices: &Path, port: &RootPort, labels: &Labels) -> Half {
    Half {
        bus: port.bus,
        port: port.port,
        device: read_device(devices, &format!("{}-{}", port.bus, port.port), labels),
        over_current_count: read_parse(port.dir.join("over_current_count")),
        port_state: read(port.dir.join("state")),
        physical_location: physical_location(&port.dir.join("physical_location")),
    }
}

fn physical_location(dir: &Path) -> Option<PhysicalLocation> {
    if !dir.is_dir() {
        return None;
    }
    let yes = |name: &str| read(dir.join(name)).as_deref() == Some("yes");
    Some(PhysicalLocation {
        panel: read(dir.join("panel")),
        horizontal: read(dir.join("horizontal_position")),
        vertical: read(dir.join("vertical_position")),
        dock: yes("dock"),
        lid: yes("lid"),
    })
}

/// The device `name` (`2-1`, `2-1.4`) with everything below it; `None` when
/// nothing is enumerated there.
fn read_device(devices: &Path, name: &str, labels: &Labels) -> Option<UsbDevice> {
    let dir = devices.join(name);
    let vendor_id = read_hex(dir.join("idVendor"))?;
    let product_id = read_hex(dir.join("idProduct")).unwrap_or(0);
    let speed = Speed::from_sysfs(&read(dir.join("speed"))?)?;
    let bcd_usb = read(dir.join("version")).and_then(|v| parse_bcd(&v));
    let capable_speed = capable_speed(
        read_bytes(dir.join("bos_descriptors")).as_deref(),
        bcd_usb,
        speed,
    );
    Some(UsbDevice {
        sysfs_name: name.to_owned(),
        vendor_id,
        product_id,
        manufacturer: read(dir.join("manufacturer")).map(|s| s.trim().to_owned()),
        product: read(dir.join("product")).map(|s| s.trim().to_owned()),
        serial: read(dir.join("serial")).map(|s| s.trim().to_owned()),
        label: labels
            .device_label(vendor_id, product_id)
            .map(str::to_owned),
        speed,
        capable_speed,
        rx_lanes: read_parse(dir.join("rx_lanes")),
        tx_lanes: read_parse(dir.join("tx_lanes")),
        bcd_usb,
        device_class: read_hex(dir.join("bDeviceClass")).unwrap_or(0),
        max_power_ma: read(dir.join("bMaxPower"))
            .and_then(|p| p.trim().trim_end_matches("mA").parse().ok()),
        driver: driver(devices, name),
        urbnum: read_parse(dir.join("urbnum")),
        children: children(devices, name)
            .iter()
            .filter_map(|child| read_device(devices, child, labels))
            .collect(),
    })
}

/// The drivers bound to the device's interfaces (`hub`, `uvcvideo,
/// snd-usb-audio`), in interface order; the device's own driver (always the
/// generic `usb`) only when no interface has one.
fn driver(devices: &Path, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    let mut drivers: Vec<String> = Vec::new();
    for iface in entries(devices, |n| n.starts_with(&prefix)) {
        if let Some(d) = link_name(iface.join("driver"))
            && !drivers.contains(&d)
        {
            drivers.push(d);
        }
    }
    if drivers.is_empty() {
        link_name(devices.join(name).join("driver"))
    } else {
        Some(drivers.join(","))
    }
}

/// Devices on the hub ports of `name`: `2-1` → `2-1.2`, `2-1.4`, by port.
fn children(devices: &Path, name: &str) -> Vec<String> {
    let prefix = format!("{name}.");
    let port = |n: &str| -> Option<u32> { n.strip_prefix(&prefix)?.parse().ok() };
    let mut names: Vec<String> = entries(devices, |n| port(n).is_some())
        .into_iter()
        .filter_map(|p| Some(p.file_name()?.to_str()?.to_owned()))
        .collect();
    names.sort_by_key(|n| port(n));
    names
}

/// sysfs `version` (` 3.20`, `2.01`; printed as `%2x.%02x`) → bcdUSB `0x0320`.
pub fn parse_bcd(version: &str) -> Option<u16> {
    let (major, minor) = version.trim().split_once('.')?;
    let major = u16::from_str_radix(major, 16).ok()?;
    let minor = u16::from_str_radix(minor, 16).ok()?;
    (major <= 0xff && minor <= 0xff).then_some(major << 8 | minor)
}

/// The fastest speed a device supports.
///
/// SuperSpeed capabilities in its BOS descriptor decide when present. Without
/// them, a device linked at 480M is a USB 2 high-speed device, and one with
/// bcdUSB below 2.0 can't go faster than it linked; a USB 2.0 device linked at
/// full or low speed may or may not do high speed (keyboards and receivers
/// claim bcdUSB 2.00 and are full-speed only), so that is `None`. Never below
/// the speed it actually linked at.
pub fn capable_speed(bos: Option<&[u8]>, bcd_usb: Option<u16>, linked: Speed) -> Option<Speed> {
    let capable = match bos.and_then(bos_max_speed) {
        Some(ss) => ss,
        None if linked == Speed::High => Speed::High,
        None if bcd_usb.is_some_and(|b| b < 0x0200) => linked,
        None if linked.is_superspeed() => linked,
        None => return None,
    };
    Some(if linked.mbps() > capable.mbps() {
        linked
    } else {
        capable
    })
}

/// The fastest SuperSpeed rate a binary BOS descriptor advertises: the
/// SuperSpeed USB Device Capability (type 0x03) for 5G, the SuperSpeedPlus
/// capability (type 0x0A) for its sublink speeds. `None` without either.
///
/// A sublink speed is per lane, and the SuperSpeedPlus capability doesn't
/// say how many lanes the device can run, so a Gen 2x2 device reads as 10G
/// here; the link itself (`speed`, `rx_lanes`) tells the rest.
pub fn bos_max_speed(bos: &[u8]) -> Option<Speed> {
    // BOS header: bLength, bDescriptorType (0x0F), wTotalLength, bNumDeviceCaps.
    if bos.len() < 5 || bos[1] != 0x0f {
        return None;
    }
    let total = usize::from(u16::from_le_bytes([bos[2], bos[3]])).min(bos.len());
    let mut best: Option<f64> = None;
    let mut at = usize::from(bos[0]);
    while at + 3 <= total {
        let len = usize::from(bos[at]);
        if len < 3 || at + len > total {
            break;
        }
        let cap = &bos[at..at + len];
        // bDescriptorType 0x10: device capability.
        if cap[1] == 0x10 {
            let gbps = match cap[2] {
                0x03 => superspeed_cap(cap),
                0x0a => superspeed_plus_cap(cap),
                _ => None,
            };
            if let Some(g) = gbps {
                best = Some(best.map_or(g, |b: f64| b.max(g)));
            }
        }
        at += len;
    }
    let gbps = best?;
    Some(if gbps >= 20.0 {
        Speed::SuperPlus2x2
    } else if gbps >= 10.0 {
        Speed::SuperPlus
    } else {
        Speed::Super
    })
}

/// SuperSpeed USB Device Capability: wSpeedsSupported bit 3 is 5 Gb/s.
fn superspeed_cap(cap: &[u8]) -> Option<f64> {
    let speeds = u16::from_le_bytes([*cap.get(4)?, *cap.get(5)?]);
    (speeds & 0x8 != 0).then_some(5.0)
}

/// SuperSpeedPlus USB Device Capability: the fastest sublink speed attribute,
/// in Gb/s.
fn superspeed_plus_cap(cap: &[u8]) -> Option<f64> {
    let attributes = u32::from_le_bytes(cap.get(4..8)?.try_into().ok()?);
    // bmAttributes bits 4:0: sublink speed attribute count, minus one.
    let count = (attributes & 0x1f) as usize + 1;
    (0..count)
        .filter_map(|i| {
            let at = 12 + 4 * i;
            let ssa = u32::from_le_bytes(cap.get(at..at + 4)?.try_into().ok()?);
            // Bits 5:4 lane speed exponent (b/s, Kb/s, Mb/s, Gb/s), 31:16 mantissa.
            let exponent = (ssa >> 4) & 0x3;
            let mantissa = f64::from(ssa >> 16);
            Some(mantissa * 1000f64.powi(exponent as i32) / 1e9)
        })
        .reduce(f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    /// The XPS dock's Genesys USB 3.1 hub (`2-1`, 05e3:0626), and its USB 2
    /// companion (`3-1`) which carries the same BOS.
    const DOCK_HUB: &str = "05 0f 2a 00 03 07 10 02 06 00 00 00 0a 10 03 00 0e 00 01 08 be 00 \
        14 10 04 00 f0 56 4b 9f f6 1d e0 11 ac 64 08 00 20 0c 9a 66";

    /// A VIA USB 2.0 hub (2109:2822) whose BOS advertises SuperSpeedPlus with
    /// four sublink speed attributes: 5G rx/tx (SuperSpeed) and 10G rx/tx.
    const VIA_HUB: &str = "05 0f 49 00 05 07 10 02 06 00 00 00 0a 10 03 00 0e 00 01 04 e7 00 \
        14 10 04 00 5c f3 ee 30 d5 07 49 25 b0 01 80 2d 79 43 4c 30 \
        1c 10 0a 00 23 00 00 00 00 11 00 00 30 00 05 00 b0 00 05 00 31 40 0a 00 b1 40 0a 00 \
        03 10 0b";

    /// Intel Bluetooth (8087:0033): a USB 2.0 extension only.
    const BLUETOOTH: &str = "05 0f 0c 00 01 07 10 02 0e 04 00 00";

    #[test]
    fn bos_superspeed_5g() {
        assert_eq!(bos_max_speed(&hex(DOCK_HUB)), Some(Speed::Super));
    }

    #[test]
    fn bos_superspeed_plus_10g() {
        assert_eq!(bos_max_speed(&hex(VIA_HUB)), Some(Speed::SuperPlus));
    }

    #[test]
    fn bos_superspeed_plus_20g_lane() {
        // One sublink speed attribute: 20 Gb/s (exponent 3, mantissa 20).
        let bos = hex("05 0f 19 00 01 \
            14 10 0a 00 00 00 00 00 00 11 00 00 30 40 14 00 b0 40 14 00");
        assert_eq!(bos_max_speed(&bos), Some(Speed::SuperPlus2x2));
    }

    #[test]
    fn bos_without_superspeed() {
        assert_eq!(bos_max_speed(&hex(BLUETOOTH)), None);
        // SuperSpeed capability whose wSpeedsSupported lacks the 5G bit.
        let bos = hex("05 0f 0f 00 01 0a 10 03 00 06 00 01 08 be 00");
        assert_eq!(bos_max_speed(&bos), None);
    }

    #[test]
    fn bos_garbage_is_none_not_a_panic() {
        assert_eq!(bos_max_speed(&[]), None);
        assert_eq!(bos_max_speed(&hex("05 0f")), None);
        assert_eq!(bos_max_speed(&hex("05 01 2a 00 03")), None);
        // Capability length runs past the end; zero-length capability.
        assert_eq!(bos_max_speed(&hex("05 0f 0a 00 01 0a 10 03 00")), None);
        assert_eq!(bos_max_speed(&hex("05 0f 08 00 01 00 10 03")), None);
        // wTotalLength longer than the data.
        assert_eq!(
            bos_max_speed(&hex("05 0f ff 00 01 0a 10 03 00 0e 00 01 08 be 00")),
            Some(Speed::Super)
        );
    }

    #[test]
    fn capable_speed_rules() {
        let dock = hex(DOCK_HUB);
        // The USB 2 half of a USB 3 hub: capable of 5G, linked at 480M.
        assert_eq!(
            capable_speed(Some(&dock), Some(0x0210), Speed::High),
            Some(Speed::Super)
        );
        assert_eq!(
            capable_speed(Some(&dock), Some(0x0320), Speed::Super),
            Some(Speed::Super)
        );
        // USB 2 devices without SuperSpeed caps.
        let bt = hex(BLUETOOTH);
        assert_eq!(
            capable_speed(Some(&bt), Some(0x0210), Speed::High),
            Some(Speed::High)
        );
        assert_eq!(
            capable_speed(None, Some(0x0200), Speed::High),
            Some(Speed::High)
        );
        // A full-speed device claiming USB 2.0 may or may not do high speed.
        assert_eq!(capable_speed(Some(&bt), Some(0x0201), Speed::Full), None);
        assert_eq!(capable_speed(None, Some(0x0200), Speed::Full), None);
        // USB 1.1 tops out at what it linked at.
        assert_eq!(
            capable_speed(None, Some(0x0110), Speed::Full),
            Some(Speed::Full)
        );
        assert_eq!(
            capable_speed(None, Some(0x0110), Speed::Low),
            Some(Speed::Low)
        );
        // Never below the actual link.
        assert_eq!(
            capable_speed(Some(&dock), Some(0x0320), Speed::SuperPlus),
            Some(Speed::SuperPlus)
        );
        assert_eq!(
            capable_speed(None, Some(0x0320), Speed::Super),
            Some(Speed::Super)
        );
    }

    #[test]
    fn bcd_from_version() {
        assert_eq!(parse_bcd(" 3.20"), Some(0x0320));
        assert_eq!(parse_bcd("2.01"), Some(0x0201));
        assert_eq!(parse_bcd(" 1.10"), Some(0x0110));
        assert_eq!(parse_bcd("3.2"), Some(0x0302));
        assert_eq!(parse_bcd("x"), None);
        assert_eq!(parse_bcd("100.00"), None);
    }

    #[test]
    fn hub_bus_names() {
        assert_eq!(bus_of_hub("usb3"), Some(3));
        assert_eq!(bus_of_hub("usb12"), Some(12));
        assert_eq!(bus_of_hub("usb"), None);
        assert_eq!(bus_of_hub("usb3-port1"), None);
        assert_eq!(bus_of_hub("3-1"), None);
    }
}
