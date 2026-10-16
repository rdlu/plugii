use plugii::model::*;

#[test]
fn speed_from_sysfs_and_labels_match_the_script() {
    let cases = [
        ("1.5", Speed::Low, 1.5, "1.5M (USB 1 low)"),
        ("12", Speed::Full, 12.0, "12M (USB 1 full)"),
        ("480", Speed::High, 480.0, "480M (USB 2)"),
        ("5000", Speed::Super, 5000.0, "5G (USB 3 Gen 1)"),
        ("10000", Speed::SuperPlus, 10000.0, "10G (USB 3 Gen 2)"),
        ("20000", Speed::SuperPlus2x2, 20000.0, "20G (USB 3 Gen 2x2)"),
        ("40000", Speed::Other(40000), 40000.0, "40000M"),
    ];
    for (sysfs, speed, mbps, label) in cases {
        assert_eq!(Speed::from_sysfs(sysfs), Some(speed), "{sysfs}");
        assert_eq!(
            Speed::from_sysfs(&format!("{sysfs}\n")),
            Some(speed),
            "{sysfs}\\n"
        );
        assert_eq!(speed.mbps(), mbps, "{sysfs}");
        assert_eq!(speed.label(), label, "{sysfs}");
    }
}

#[test]
fn speed_from_sysfs_rejects_garbage() {
    assert_eq!(Speed::from_sysfs(""), None);
    assert_eq!(Speed::from_sysfs("unknown"), None);
    assert_eq!(Speed::from_sysfs("-1"), None);
}

#[test]
fn superspeed_is_5g_and_up() {
    assert!(!Speed::Low.is_superspeed());
    assert!(!Speed::Full.is_superspeed());
    assert!(!Speed::High.is_superspeed());
    assert!(Speed::Super.is_superspeed());
    assert!(Speed::SuperPlus.is_superspeed());
    assert!(Speed::SuperPlus2x2.is_superspeed());
    assert!(Speed::Other(40000).is_superspeed());
    assert!(!Speed::Other(1000).is_superspeed());
}

#[test]
fn pdo_max_watts() {
    let fixed = Pdo::Fixed {
        mv: 20_000,
        ma: 3_250,
    };
    assert_eq!(fixed.max_watts(), Some(65.0));
    let variable = Pdo::Variable {
        min_mv: 5_000,
        max_mv: 21_000,
        ma: 3_000,
    };
    assert_eq!(variable.max_watts(), Some(63.0));
    let battery = Pdo::Battery {
        min_mv: 5_000,
        max_mv: 20_000,
        mw: 45_000,
    };
    assert_eq!(battery.max_watts(), Some(45.0));
    let pps = Pdo::Pps {
        min_mv: 3_300,
        max_mv: 11_000,
        ma: 5_000,
    };
    assert_eq!(pps.max_watts(), Some(55.0));
    assert_eq!(Pdo::Other("epr".into()).max_watts(), None);
}

#[test]
fn contract_and_source_watts() {
    let c = PdContract {
        voltage_mv: 20_000,
        current_ma: 3_250,
        source_pdo: None,
    };
    assert_eq!(c.watts(), 65.0);
    let s = UsbCSource {
        typec_port: "port0".into(),
        voltage_mv: Some(20_000),
        current_ma: Some(2_250),
    };
    assert_eq!(s.watts(), Some(45.0));
    let unknown = UsbCSource {
        current_ma: None,
        ..s
    };
    assert_eq!(unknown.watts(), None);
}

fn device(
    sysfs_name: &str,
    ids: (u16, u16),
    product: &str,
    speed: Speed,
    class: u8,
    driver: &str,
) -> UsbDevice {
    UsbDevice {
        sysfs_name: sysfs_name.into(),
        vendor_id: ids.0,
        product_id: ids.1,
        manufacturer: Some("Realtek".into()),
        product: Some(product.into()),
        serial: None,
        label: None,
        speed,
        capable_speed: Some(speed),
        rx_lanes: Some(1),
        tx_lanes: Some(1),
        bcd_usb: Some(if speed.is_superspeed() {
            0x0320
        } else {
            0x0210
        }),
        device_class: class,
        max_power_ma: Some(0),
        driver: Some(driver.into()),
        urbnum: Some(1234),
        children: Vec::new(),
    }
}

/// The XPS 9320 with the dock on the left socket (charging through it) and a
/// monitor in DP alt mode on the right one.
fn xps() -> Snapshot {
    let mut usb3_hub = device(
        "2-1",
        (0x0bda, 0x0411),
        "4-Port USB 3.0 Hub",
        Speed::SuperPlus,
        9,
        "hub",
    );
    usb3_hub.children.push(device(
        "2-1.4",
        (0x0bda, 0x8153),
        "USB 10/100/1000 LAN",
        Speed::Super,
        0,
        "r8152",
    ));
    let mut usb2_hub = device(
        "3-1",
        (0x0bda, 0x5411),
        "4-Port USB 2.0 Hub",
        Speed::High,
        9,
        "hub",
    );
    usb2_hub.children.push(UsbDevice {
        manufacturer: Some("Keychron".into()),
        serial: Some("K3-0001".into()),
        max_power_ma: Some(100),
        ..device(
            "3-1.3",
            (0x3434, 0x0331),
            "Keychron K3",
            Speed::Full,
            0,
            "usbhid",
        )
    });

    Snapshot {
        taken_at_ms: 1_791_100_000_000,
        host: "xps".into(),
        sockets: vec![
            Socket {
                key: "0000:00:14.0-1".into(),
                label: Some("USB-C (left)".into()),
                usb2: Half {
                    bus: 3,
                    port: 1,
                    device: Some(usb2_hub),
                    over_current_count: Some(0),
                    port_state: Some("not attached".into()),
                    physical_location: None,
                },
                usb3: Usb3Side::Paired(Half {
                    bus: 2,
                    port: 1,
                    device: Some(usb3_hub),
                    over_current_count: Some(0),
                    port_state: Some("not attached".into()),
                    physical_location: None,
                }),
                typec_port: Some("port0".into()),
            },
            Socket {
                key: "0000:00:14.0-3".into(),
                label: Some("USB-C (right)".into()),
                usb2: Half {
                    bus: 3,
                    port: 3,
                    device: None,
                    over_current_count: Some(0),
                    port_state: Some("not attached".into()),
                    physical_location: None,
                },
                usb3: Usb3Side::Paired(Half {
                    bus: 2,
                    port: 3,
                    device: None,
                    over_current_count: Some(0),
                    port_state: Some("not attached".into()),
                    physical_location: None,
                }),
                typec_port: Some("port1".into()),
            },
        ],
        typec: TypecState {
            ucsi_present: true,
            ports: vec![
                TypecPort {
                    name: "port0".into(),
                    label: Some("USB-C (left)".into()),
                    partner: Some(Partner {
                        supports_pd: Some(false),
                        pd_revision: None,
                        alt_modes: 0,
                        source_pdos: vec![
                            Pdo::Fixed {
                                mv: 5_000,
                                ma: 3_000,
                            },
                            Pdo::Fixed {
                                mv: 9_000,
                                ma: 3_000,
                            },
                            Pdo::Fixed {
                                mv: 15_000,
                                ma: 3_000,
                            },
                            Pdo::Fixed {
                                mv: 20_000,
                                ma: 3_250,
                            },
                        ],
                        sink_pdos: Vec::new(),
                    }),
                    cable_type: None,
                    power_role: Some("sink".into()),
                    charging: true,
                    pd_inconsistent: false,
                    pd_contract: Some(PdContract {
                        voltage_mv: 20_000,
                        current_ma: 3_250,
                        source_pdo: Some(4),
                    }),
                },
                TypecPort {
                    name: "port1".into(),
                    label: Some("USB-C (right)".into()),
                    partner: Some(Partner {
                        supports_pd: Some(true),
                        pd_revision: Some("3.0".into()),
                        alt_modes: 1,
                        source_pdos: Vec::new(),
                        sink_pdos: vec![Pdo::Fixed { mv: 5_000, ma: 900 }],
                    }),
                    cable_type: Some("passive".into()),
                    power_role: Some("source".into()),
                    charging: false,
                    pd_inconsistent: false,
                    pd_contract: None,
                },
            ],
        },
        thunderbolt: Some(Thunderbolt {
            domains: [0, 1]
                .map(|index| TbDomain {
                    index,
                    security: Some("user".into()),
                    iommu_dma_protection: true,
                    controller_vendor: Some("Intel".into()),
                    controller_device: Some("Alder Lake".into()),
                    generation: Some(4),
                })
                .into(),
            devices: Vec::new(),
        }),
        displays: vec![Display {
            connector: "DP-2".into(),
            kind: DisplayKind::Dp,
            mode: Some("2560x1440@59.951".into()),
            vrr: Some(Vrr::Unsupported),
        }],
        power: Power {
            ac_online: Some(true),
            battery: Some(Battery {
                capacity_pct: Some(78),
                status: Some("Charging".into()),
                charge_limit_pct: Some(80),
                flow_mw: Some(12_400),
            }),
            usb_c_sources: vec![UsbCSource {
                typec_port: "port0".into(),
                voltage_mv: Some(20_000),
                current_ma: Some(3_250),
            }],
        },
        cpu: Some(CpuSample {
            util_pct: 92,
            avg_khz: 1_700_000,
            max_khz: 2_400_000,
            min_khz: Some(400_000),
            package_w: Some(20.5),
            package_temp_c: Some(88.0),
        }),
        kernel_log: KernelLog {
            errors: vec![LogLine {
                secs_since_boot: Some(14),
                message: "usb 3-1.2: device descriptor read/64, error -71".into(),
                socket: Some("USB-C (left)".into()),
            }],
            benign_count: 0,
            link_resets: vec![LinkResets {
                place: "USB-C (left)".into(),
                count: 2,
            }],
        },
    }
}

/// Every enum variant the XPS snapshot doesn't reach.
fn odd_machine() -> Snapshot {
    Snapshot {
        taken_at_ms: 1,
        host: "odd".into(),
        sockets: vec![
            Socket {
                key: "0000:06:00.4-1".into(),
                label: None,
                usb2: Half {
                    bus: 1,
                    port: 1,
                    device: None,
                    over_current_count: Some(0),
                    port_state: Some("not attached".into()),
                    physical_location: None,
                },
                usb3: Usb3Side::Unknown,
                typec_port: None,
            },
            Socket {
                key: "0000:06:00.4-2".into(),
                label: None,
                usb2: Half {
                    bus: 1,
                    port: 2,
                    device: Some(device(
                        "1-2",
                        (0x046d, 0xc52b),
                        "Receiver",
                        Speed::Low,
                        0,
                        "usbhid",
                    )),
                    over_current_count: None,
                    port_state: None,
                    physical_location: Some(PhysicalLocation {
                        panel: Some("back".into()),
                        horizontal: Some("right".into()),
                        vertical: None,
                        dock: false,
                        lid: false,
                    }),
                },
                usb3: Usb3Side::Usb2Only,
                typec_port: None,
            },
        ],
        typec: TypecState {
            ucsi_present: true,
            ports: vec![TypecPort {
                name: "port0".into(),
                label: None,
                partner: Some(Partner {
                    supports_pd: None,
                    pd_revision: None,
                    alt_modes: 0,
                    source_pdos: vec![
                        Pdo::Variable {
                            min_mv: 5_000,
                            max_mv: 21_000,
                            ma: 3_000,
                        },
                        Pdo::Battery {
                            min_mv: 5_000,
                            max_mv: 20_000,
                            mw: 45_000,
                        },
                        Pdo::Pps {
                            min_mv: 3_300,
                            max_mv: 11_000,
                            ma: 5_000,
                        },
                        Pdo::Other("epr".into()),
                    ],
                    sink_pdos: Vec::new(),
                }),
                cable_type: None,
                power_role: None,
                charging: false,
                pd_inconsistent: false,
                pd_contract: None,
            }],
        },
        thunderbolt: Some(Thunderbolt {
            domains: Vec::new(),
            devices: vec![TbDevice {
                route: "0-1".into(),
                unique_id: Some("c4010000-0070-8c18-23a7-1b5d32c1e921".into()),
                vendor: Some("Dell".into()),
                name: Some("WD19TB".into()),
                authorized: false,
                generation: Some(3),
                link: Some(TbLink {
                    rx_gbps: 20.0,
                    rx_lanes: 2,
                    tx_gbps: 20.0,
                    tx_lanes: 2,
                }),
                boltd_stored: Some(false),
            }],
        }),
        displays: vec![
            Display {
                connector: "HDMI-A-1".into(),
                kind: DisplayKind::Hdmi,
                mode: None,
                vrr: Some(Vrr::On),
            },
            Display {
                connector: "DVI-D-1".into(),
                kind: DisplayKind::Other,
                mode: None,
                vrr: Some(Vrr::Off),
            },
        ],
        power: Power {
            ac_online: None,
            battery: None,
            usb_c_sources: Vec::new(),
        },
        cpu: None,
        kernel_log: KernelLog {
            errors: vec![LogLine {
                secs_since_boot: None,
                message: "unparsed".into(),
                socket: None,
            }],
            benign_count: 3,
            link_resets: Vec::new(),
        },
    }
}

fn assert_round_trips(snapshot: &Snapshot) {
    let json = serde_json::to_string(snapshot).unwrap();
    let back: Snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, snapshot);
}

#[test]
fn xps_snapshot_json() {
    let snapshot = xps();
    assert_round_trips(&snapshot);
    insta::assert_json_snapshot!(snapshot);
}

#[test]
fn odd_machine_snapshot_json() {
    let snapshot = odd_machine();
    assert_round_trips(&snapshot);
    insta::assert_json_snapshot!(snapshot);
}

#[test]
fn findings_round_trip() {
    let findings = vec![
        Finding {
            level: Level::Ok,
            message: "kernel log clean".into(),
        },
        Finding {
            level: Level::Warn,
            message: "7 USB link resets".into(),
        },
        Finding {
            level: Level::Fail,
            message: "hub stuck at USB 2".into(),
        },
    ];
    let json = serde_json::to_string(&findings).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<Finding>>(&json).unwrap(),
        findings
    );
    assert!(json.contains(r#""level":"warn""#), "{json}");
}
