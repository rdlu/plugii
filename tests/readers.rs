//! The USB, USB-C and Thunderbolt readers against the captured XPS 9320 and
//! hand-built trees for what that machine can't show.

mod common;

use std::path::Path;

use common::{BOS_5G, BOS_10G, Dev, Sysfs};
use plugii::labels::{self, Labels};
use plugii::model::{Pdo, Speed, Usb3Side};
use plugii::sysfs::{Root, thunderbolt, typec, usb};

fn xps() -> (Root, Labels) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let labels = labels::read(&dir.join("labels/xps")).unwrap();
    (Root::new(dir.join("sysfs/xps")), labels)
}

#[test]
fn xps_sockets() {
    let (root, labels) = xps();
    let sockets = usb::read_sockets(&root, &labels);
    insta::assert_json_snapshot!(sockets);

    // The dock on the left: a 5G hub on the USB 3 half, its USB 2 companion
    // hub on the USB 2 half. Nothing on the right (the monitor is DP only).
    assert_eq!(sockets.len(), 2);
    let left = &sockets[0];
    assert_eq!(left.key, "0000:00:14.0-1");
    assert_eq!(left.label.as_deref(), Some("USB-C (left)"));
    assert_eq!(left.typec_port.as_deref(), Some("port0"));
    let Usb3Side::Paired(usb3) = &left.usb3 else {
        panic!("left socket not paired: {:?}", left.usb3)
    };
    assert_eq!((usb3.bus, usb3.port), (2, 1));
    let hub = usb3.device.as_ref().unwrap();
    assert_eq!(hub.speed, Speed::Super);
    assert_eq!(hub.device_class, 9);
    let usb2_hub = left.usb2.device.as_ref().unwrap();
    assert_eq!(usb2_hub.speed, Speed::High);
    assert_eq!(usb2_hub.capable_speed, Some(Speed::Super));
    let right = &sockets[1];
    assert_eq!(right.key, "0000:00:14.0-3");
    assert!(right.usb2.device.is_none());
    assert!(matches!(&right.usb3, Usb3Side::Paired(h) if h.device.is_none()));
}

#[test]
fn xps_typec() {
    let (root, labels) = xps();
    let state = typec::read(&root, &labels);
    insta::assert_json_snapshot!(state);

    assert!(state.ucsi_present);
    let [left, right] = &state.ports[..] else {
        panic!("{:?}", state.ports)
    };
    // The dock offers 5/9/15/20 V and the laptop sinks 20 V from it.
    assert_eq!(left.label.as_deref(), Some("USB-C (left)"));
    assert!(left.charging);
    let contract = left.pd_contract.unwrap();
    assert_eq!(contract.voltage_mv, 20_000);
    assert_eq!(contract.source_pdo, Some(4));
    assert_eq!(left.partner.as_ref().unwrap().source_pdos.len(), 4);
    // The monitor: DP alt mode, sinks 5 V from the laptop.
    assert_eq!(right.power_role.as_deref(), Some("source"));
    assert!(!right.charging);
    let monitor = right.partner.as_ref().unwrap();
    assert_eq!(monitor.alt_modes, 1);
    assert!(monitor.source_pdos.is_empty());
}

#[test]
fn xps_thunderbolt() {
    let (root, _) = xps();
    let tb = thunderbolt::read(&root).unwrap();
    insta::assert_json_snapshot!(tb);
    assert_eq!(tb.domains.len(), 2);
    assert!(tb.devices.is_empty());
}

/// An AMD laptop whose firmware describes `peer` links: two USB-C sockets
/// paired by the kernel and a USB 2-only socket with no peer.
fn peer_links() -> Sysfs {
    let fs = Sysfs::new();
    let pci = "0000:06:00.4";
    fs.root_hub(
        pci,
        1,
        "480",
        &[
            (1, "hotplug"),
            (2, "hotplug"),
            (3, "hotplug"),
            (4, "hardwired"),
        ],
    );
    fs.root_hub(pci, 2, "10000", &[(1, "hotplug"), (2, "hotplug")]);
    fs.peer((1, 1), (2, 1));
    fs.peer((1, 2), (2, 2));
    fs.device(
        "2-1",
        Dev {
            ids: (0x0bda, 0x9210),
            speed: "10000",
            version: " 3.20",
            class: 0,
            product: "RTL9210 NVMe",
            bos: Some(BOS_10G),
            driver: "uas",
        },
    );
    fs.device(
        "1-3",
        Dev {
            ids: (0x046d, 0xc52b),
            speed: "12",
            version: " 2.00",
            class: 0,
            product: "USB Receiver",
            bos: None,
            driver: "usbhid",
        },
    );
    fs.file(
        "sys/devices/pci0000:00/0000:06:00.4/usb1/1-0:1.0/usb1-port1/physical_location/panel",
        "left",
    );
    fs
}

#[test]
fn kernel_peer_links_pair_halves() {
    let fs = peer_links();
    let labels = Labels::parse("0000:06:00.4-1  USB-C (left)\ntypec:port0  USB-C (left)\n");
    let sockets = usb::read_sockets(&fs.root(), &labels);
    insta::assert_json_snapshot!(sockets);

    let keys: Vec<&str> = sockets.iter().map(|s| s.key.as_str()).collect();
    // The hardwired port 4 isn't a socket.
    assert_eq!(keys, ["0000:06:00.4-1", "0000:06:00.4-2", "0000:06:00.4-3"]);
    assert_eq!(sockets[0].typec_port.as_deref(), Some("port0"));
    let Usb3Side::Paired(ssd) = &sockets[0].usb3 else {
        panic!("{:?}", sockets[0].usb3)
    };
    let ssd = ssd.device.as_ref().unwrap();
    assert_eq!(ssd.speed, Speed::SuperPlus);
    assert_eq!(ssd.capable_speed, Some(Speed::SuperPlus));
    assert_eq!(ssd.driver.as_deref(), Some("uas"));
    assert!(matches!(&sockets[1].usb3, Usb3Side::Paired(h) if h.device.is_none() && h.bus == 2));
    // Other ports have peer links and this one doesn't: USB 2 only.
    assert_eq!(sockets[2].usb3, Usb3Side::Usb2Only);
    assert_eq!(
        sockets[0]
            .usb2
            .physical_location
            .as_ref()
            .unwrap()
            .panel
            .as_deref(),
        Some("left")
    );
}

/// The XPS layout (USB 2 and USB 3 on different controllers, no peer links)
/// with a USB 3 hub that only came up on the USB 2 half.
fn stuck_at_usb2() -> Sysfs {
    let fs = Sysfs::new();
    fs.root_hub("0000:00:0d.0", 1, "480", &[(1, "not used")]);
    fs.root_hub(
        "0000:00:0d.0",
        2,
        "20000",
        &[(1, "hotplug"), (3, "hotplug")],
    );
    fs.root_hub(
        "0000:00:14.0",
        3,
        "480",
        &[(1, "hotplug"), (3, "hotplug"), (10, "not used")],
    );
    fs.root_hub("0000:00:14.0", 4, "10000", &[(1, "not used")]);
    fs.device(
        "3-1",
        Dev {
            ids: (0x05e3, 0x0610),
            speed: "480",
            version: " 2.10",
            class: 9,
            product: "USB2.1 Hub",
            bos: Some(BOS_5G),
            driver: "hub",
        },
    );
    fs.device(
        "3-1.2",
        Dev {
            ids: (0x1050, 0x0407),
            speed: "12",
            version: " 2.00",
            class: 0,
            product: "YubiKey",
            bos: None,
            driver: "usbhid",
        },
    );
    fs
}

#[test]
fn usb3_hub_stuck_on_the_usb2_half() {
    let fs = stuck_at_usb2();
    // Only the left socket has a peer: label; the right one stays unknown.
    let labels =
        Labels::parse("0000:00:14.0-1  USB-C (left)\npeer:0000:00:14.0-1  0000:00:0d.0-1\n");
    let sockets = usb::read_sockets(&fs.root(), &labels);
    insta::assert_json_snapshot!(sockets);

    assert_eq!(sockets.len(), 2);
    let left = &sockets[0];
    let Usb3Side::Paired(usb3) = &left.usb3 else {
        panic!("{:?}", left.usb3)
    };
    // The peer: label's controller maps to its SuperSpeed root hub, bus 2.
    assert_eq!((usb3.bus, usb3.port), (2, 1));
    assert!(usb3.device.is_none());
    let hub = left.usb2.device.as_ref().unwrap();
    assert_eq!(hub.device_class, 9);
    assert_eq!(hub.speed, Speed::High);
    assert_eq!(hub.capable_speed, Some(Speed::Super));
    assert_eq!(hub.children.len(), 1);
    assert_eq!(hub.children[0].sysfs_name, "3-1.2");
    assert_eq!(hub.children[0].capable_speed, None);
    // No peer links on the machine and no peer: label: unknown, not absent.
    assert_eq!(sockets[1].usb3, Usb3Side::Unknown);
}

#[test]
fn no_usb_at_all() {
    let fs = Sysfs::new();
    assert!(usb::read_sockets(&fs.root(), &Labels::default()).is_empty());
}

#[test]
fn ucsi_missing() {
    let fs = stuck_at_usb2();
    let state = typec::read(&fs.root(), &Labels::default());
    assert!(!state.ucsi_present);
    assert!(state.ports.is_empty());
    assert_eq!(thunderbolt::read(&fs.root()), None);
}

/// A UCSI port with a PPS charger and a passive cable, and an empty one.
fn pps_charger() -> Sysfs {
    let fs = Sysfs::new();
    let ucsi = "sys/devices/platform/AMDI0042:00";
    let port0 = format!("{ucsi}/typec/port0");
    let port1 = format!("{ucsi}/typec/port1");
    fs.file(format!("{port0}/power_role"), "source [sink]");
    fs.file(format!("{port1}/power_role"), "[source] sink");
    fs.link("sys/class/typec/port0", &port0);
    fs.link("sys/class/typec/port1", &port1);
    let partner = format!("{port0}/port0-partner");
    fs.file(format!("{partner}/supports_usb_power_delivery"), "yes");
    fs.file(format!("{partner}/usb_power_delivery_revision"), "3.0");
    fs.file(format!("{partner}/number_of_alternate_modes"), "0");
    fs.link("sys/class/typec/port0-partner", &partner);
    fs.file(format!("{port0}/port0-cable/type"), "passive");
    let pd = format!("{partner}/pd1");
    fs.link(format!("{partner}/usb_power_delivery"), &pd);
    let src = format!("{pd}/source-capabilities");
    for (n, mv, ma) in [
        (1, 5000, 3000),
        (2, 9000, 3000),
        (3, 15000, 3000),
        (4, 20000, 3250),
    ] {
        fs.file(
            format!("{src}/{n}:fixed_supply/voltage"),
            &format!("{mv}mV"),
        );
        fs.file(
            format!("{src}/{n}:fixed_supply/maximum_current"),
            &format!("{ma}mA"),
        );
    }
    let pps = format!("{src}/5:programmable_supply");
    fs.file(format!("{pps}/minimum_voltage"), "3300mV");
    fs.file(format!("{pps}/maximum_voltage"), "21000mV");
    fs.file(format!("{pps}/maximum_current"), "5000mA");
    let var = format!("{src}/6:variable_supply");
    fs.file(format!("{var}/minimum_voltage"), "5000mV");
    fs.file(format!("{var}/maximum_voltage"), "20000mV");
    fs.file(format!("{var}/maximum_current"), "2000mA");
    let bat = format!("{src}/7:battery");
    fs.file(format!("{bat}/minimum_voltage"), "5000mV");
    fs.file(format!("{bat}/maximum_voltage"), "20000mV");
    fs.file(format!("{bat}/maximum_power"), "45000mW");
    fs.dir(format!("{src}/8:spr_adjustable_voltage_supply"));
    fs.file(
        format!("{pd}/sink-capabilities/1:fixed_supply/voltage"),
        "5000mV",
    );
    fs.file(
        format!("{pd}/sink-capabilities/1:fixed_supply/operational_current"),
        "900mA",
    );
    // Connector 1 sinks 15 V at 3 A; connector 2 is idle.
    for (n, online, uv) in [(1, "1", "15000000"), (2, "0", "5000000")] {
        let psy = format!("{ucsi}/power_supply/ucsi-source-psy-AMDI0042:00{n}");
        fs.file(format!("{psy}/online"), online);
        fs.file(format!("{psy}/voltage_now"), uv);
        fs.file(format!("{psy}/current_max"), "3000000");
        fs.link(
            format!("sys/class/power_supply/ucsi-source-psy-AMDI0042:00{n}"),
            &psy,
        );
    }
    fs
}

#[test]
fn pps_and_variable_pdos() {
    let fs = pps_charger();
    let labels = Labels::parse("typec:port0  USB-C (rear)\n");
    let state = typec::read(&fs.root(), &labels);
    insta::assert_json_snapshot!(state);

    let port0 = &state.ports[0];
    assert_eq!(port0.label.as_deref(), Some("USB-C (rear)"));
    assert_eq!(port0.cable_type.as_deref(), Some("passive"));
    assert!(port0.charging);
    let contract = port0.pd_contract.unwrap();
    assert_eq!((contract.voltage_mv, contract.current_ma), (15_000, 3_000));
    assert_eq!(contract.source_pdo, Some(3));
    let partner = port0.partner.as_ref().unwrap();
    assert_eq!(partner.pd_revision.as_deref(), Some("3.0"));
    assert_eq!(
        partner.source_pdos[4..],
        [
            Pdo::Pps {
                min_mv: 3_300,
                max_mv: 21_000,
                ma: 5_000
            },
            Pdo::Variable {
                min_mv: 5_000,
                max_mv: 20_000,
                ma: 2_000
            },
            Pdo::Battery {
                min_mv: 5_000,
                max_mv: 20_000,
                mw: 45_000
            },
            Pdo::Other("8:spr_adjustable_voltage_supply".into()),
        ]
    );
    assert_eq!(partner.sink_pdos, [Pdo::Fixed { mv: 5_000, ma: 900 }]);
    let port1 = &state.ports[1];
    assert!(port1.partner.is_none());
    assert!(!port1.charging);
    assert_eq!(port1.pd_contract, None);
}

/// A Thunderbolt dock that boltd hasn't authorized, plus an XDomain service
/// and a retimer that aren't devices.
fn unauthorized_dock() -> Sysfs {
    let fs = Sysfs::new();
    let domain = "sys/devices/pci0000:00/0000:00:0d.2/domain0";
    fs.file(format!("{domain}/security"), "user");
    fs.file(format!("{domain}/iommu_dma_protection"), "1");
    fs.link("sys/bus/thunderbolt/devices/domain0", domain);
    let host = format!("{domain}/0-0");
    fs.file(format!("{host}/vendor_name"), "INTEL");
    fs.file(format!("{host}/device_name"), "Gen12");
    fs.file(format!("{host}/generation"), "4");
    fs.file(format!("{host}/authorized"), "1");
    fs.link("sys/bus/thunderbolt/devices/0-0", &host);
    let dock = format!("{host}/0-1");
    fs.file(format!("{dock}/authorized"), "0");
    fs.file(format!("{dock}/vendor_name"), "Dell");
    fs.file(format!("{dock}/device_name"), "WD19TB");
    fs.file(format!("{dock}/generation"), "3");
    fs.file(
        format!("{dock}/unique_id"),
        "c4010000-0070-8c18-23a7-1b5d32c1e921",
    );
    fs.file(format!("{dock}/rx_speed"), "20.0 Gb/s");
    fs.file(format!("{dock}/rx_lanes"), "2");
    fs.file(format!("{dock}/tx_speed"), "20.0 Gb/s");
    fs.file(format!("{dock}/tx_lanes"), "2");
    fs.link("sys/bus/thunderbolt/devices/0-1", &dock);
    for other in ["0-1.1", "0-0:1.1"] {
        let dir = format!("{host}/{other}");
        fs.file(format!("{dir}/authorized"), "1");
        fs.link(format!("sys/bus/thunderbolt/devices/{other}"), &dir);
    }
    fs
}

#[test]
fn unauthorized_thunderbolt_device() {
    let fs = unauthorized_dock();
    let tb = thunderbolt::read(&fs.root()).unwrap();
    insta::assert_json_snapshot!(tb);

    assert_eq!(tb.domains.len(), 1);
    assert_eq!(tb.domains[0].security.as_deref(), Some("user"));
    assert_eq!(tb.domains[0].controller_device.as_deref(), Some("Gen12"));
    let [dock] = &tb.devices[..] else {
        panic!("{:?}", tb.devices)
    };
    assert_eq!(dock.route, "0-1");
    assert!(!dock.authorized);
    let link = dock.link.unwrap();
    assert_eq!((link.rx_gbps, link.rx_lanes), (20.0, 2));
    assert_eq!(dock.boltd_stored, None);
}
