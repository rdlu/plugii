use std::fs;
use std::path::{Path, PathBuf};

use plugii::model::{Battery, Power, UsbCSource};
use plugii::sysfs::{Root, power};

fn fixture(name: &str) -> Root {
    Root::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sysfs")
            .join(name),
    )
}

#[test]
fn intel_on_ac_charging_via_usb_c() {
    let power = power::read(&fixture("intel-ac-usbc"));
    assert_eq!(
        power,
        Power {
            ac_online: Some(true),
            battery: Some(Battery {
                capacity_pct: Some(64),
                status: Some("Charging".into()),
                charge_limit_pct: Some(80),
                flow_mw: Some(24_500),
            }),
            usb_c_sources: vec![UsbCSource {
                typec_port: "port1".into(),
                voltage_mv: Some(20_000),
                current_ma: Some(2_350),
            }],
        }
    );
    assert_eq!(power.usb_c_sources[0].watts(), Some(47.0));
}

#[test]
fn intel_on_battery_discharging() {
    let power = power::read(&fixture("intel-on-battery"));
    assert_eq!(power.ac_online, Some(false));
    let battery = power.battery.unwrap();
    assert_eq!(battery.flow_mw, Some(-8_200));
    assert_eq!(battery.charge_limit_pct, None);
    assert!(power.usb_c_sources.is_empty());
}

#[test]
fn amd_battery_flow_from_current_times_voltage() {
    // 1.5 A × 15.4 V = 23.1 W out; the mouse battery (scope Device) is skipped.
    insta::assert_json_snapshot!(power::read(&fixture("amd-battery")));
}

#[test]
fn not_charging_has_no_flow() {
    let power = power::read(&fixture("prochot"));
    let battery = power.battery.unwrap();
    assert_eq!(battery.status.as_deref(), Some("Not charging"));
    assert_eq!(battery.flow_mw, None);
    assert_eq!(power.usb_c_sources[0].typec_port, "port0");
    assert_eq!(power.usb_c_sources[0].watts(), Some(65.0));
}

/// Lenovo's UCSI (daisy) leaves `voltage_now` and `current_max` at 0 while
/// online: 0 is "not reported", the current falls back to `current_now`, and
/// no watts are made up.
#[test]
fn lenovo_ucsi_zeros_are_not_reported() {
    let power = power::read(&fixture("daisy-charger"));
    assert_eq!(
        power.usb_c_sources,
        vec![UsbCSource {
            typec_port: "port0".into(),
            voltage_mv: None,
            current_ma: Some(3_250),
        }]
    );
    assert_eq!(power.usb_c_sources[0].watts(), None);
}

#[test]
fn desktop_has_no_supplies() {
    assert_eq!(
        power::read(&fixture("desktop")),
        Power {
            ac_online: None,
            battery: None,
            usb_c_sources: vec![],
        }
    );
}

fn psy(root: &Path, name: &str, attrs: &[(&str, &str)]) {
    let dir = root.join("sys/class/power_supply").join(name);
    fs::create_dir_all(&dir).unwrap();
    for (key, value) in attrs {
        fs::write(dir.join(key), format!("{value}\n")).unwrap();
    }
}

#[test]
fn supplies_found_by_type_not_name() {
    let dir = tempfile::tempdir().unwrap();
    // A peripheral battery that sorts first, then the system battery and
    // two mains supplies under unusual names, one of them online.
    psy(
        dir.path(),
        "AAA-mouse",
        &[("type", "Battery"), ("scope", "Device"), ("capacity", "5")],
    );
    psy(
        dir.path(),
        "CMB1",
        &[("type", "Battery"), ("capacity", "99"), ("status", "Full")],
    );
    psy(dir.path(), "ACAD", &[("type", "Mains"), ("online", "0")]);
    psy(dir.path(), "ADP1", &[("type", "Mains"), ("online", "1")]);
    let power = power::read(&Root::new(dir.path()));
    assert_eq!(power.ac_online, Some(true));
    assert_eq!(power.battery.unwrap().capacity_pct, Some(99));
}

#[test]
fn current_without_voltage_gives_no_flow() {
    let dir = tempfile::tempdir().unwrap();
    psy(
        dir.path(),
        "BAT1",
        &[
            ("type", "Battery"),
            ("status", "Discharging"),
            ("current_now", "-1500000"),
        ],
    );
    let power = power::read(&Root::new(dir.path()));
    assert_eq!(power.battery.unwrap().flow_mw, None);
}

#[test]
fn negative_current_is_taken_by_magnitude() {
    // Some drivers sign current_now; the sign comes from status, as in the script.
    let dir = tempfile::tempdir().unwrap();
    psy(
        dir.path(),
        "BAT0",
        &[
            ("type", "Battery"),
            ("status", "Discharging"),
            ("current_now", "-2000000"),
            ("voltage_now", "12000000"),
        ],
    );
    let power = power::read(&Root::new(dir.path()));
    assert_eq!(power.battery.unwrap().flow_mw, Some(-24_000));
}
