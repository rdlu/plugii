//! `plugii capture`: what it copies, what it leaves out, and that readers see
//! the same machine in the capture as in the source tree.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use common::{BOS_5G, Dev, Sysfs};
use plugii::capture::{MAX_FILE, capture};
use plugii::labels::Labels;
use plugii::model::Usb3Side;
use plugii::sysfs::{Root, thunderbolt, typec, usb};

/// Peer-linked USB ports with a device, a UCSI port with a partner and PDOs,
/// its power supply, a Thunderbolt domain, and some of what milestone 3 reads.
fn machine() -> Sysfs {
    let fs = Sysfs::new();
    let pci = "0000:06:00.4";
    fs.root_hub(pci, 1, "480", &[(1, "hotplug"), (2, "hotplug")]);
    fs.root_hub(pci, 2, "5000", &[(1, "hotplug")]);
    fs.peer((1, 1), (2, 1));
    fs.device(
        "2-1",
        Dev {
            ids: (0x05e3, 0x0626),
            speed: "5000",
            version: " 3.20",
            class: 9,
            product: "USB3.1 Hub",
            bos: Some(BOS_5G),
            driver: "hub",
        },
    );
    fs.file(
        "sys/devices/pci0000:00/0000:06:00.4/usb2/2-1/serial",
        "0123456789AB",
    );
    fs.file(
        "sys/devices/pci0000:00/0000:06:00.4/usb1/1-0:1.0/usb1-port1/physical_location/dock",
        "no",
    );

    let ucsi = "sys/devices/platform/USBC000:00";
    let port0 = format!("{ucsi}/typec/port0");
    fs.file(format!("{port0}/power_role"), "source [sink]");
    fs.link("sys/class/typec/port0", &port0);
    let partner = format!("{port0}/port0-partner");
    fs.file(format!("{partner}/supports_usb_power_delivery"), "yes");
    fs.link("sys/class/typec/port0-partner", &partner);
    fs.link(
        format!("{partner}/usb_power_delivery"),
        format!("{partner}/pd1"),
    );
    fs.file(
        format!("{partner}/pd1/source-capabilities/1:fixed_supply/voltage"),
        "20000mV",
    );
    fs.file(
        format!("{partner}/pd1/source-capabilities/1:fixed_supply/maximum_current"),
        "3250mA",
    );
    // Not followed: the partner's `device` link and the port's runtime PM.
    fs.link(format!("{partner}/pd1/device"), &partner);
    fs.file(format!("{port0}/power/runtime_status"), "active");
    let psy = format!("{ucsi}/power_supply/ucsi-source-psy-USBC000:001");
    fs.file(format!("{psy}/online"), "1");
    fs.file(format!("{psy}/voltage_now"), "20000000");
    fs.file(format!("{psy}/current_max"), "3250000");
    fs.file(format!("{psy}/uevent"), "POWER_SUPPLY_NAME=x");
    fs.link("sys/class/power_supply/ucsi-source-psy-USBC000:001", &psy);
    let bat = "sys/devices/LNXSYSTM:00/PNP0C0A:00/power_supply/BAT0";
    fs.file(format!("{bat}/serial_number"), "SN-4242");
    fs.file(format!("{bat}/capacity"), "78");
    fs.link("sys/class/power_supply/BAT0", bat);

    let domain = "sys/devices/pci0000:00/0000:00:0d.2/domain0";
    fs.file(format!("{domain}/security"), "user");
    fs.link("sys/bus/thunderbolt/devices/domain0", domain);
    fs.file(
        format!("{domain}/0-0/unique_id"),
        "d2010000-0000-0000-0000-000000000000",
    );
    fs.file(format!("{domain}/0-0/device_name"), "Gen12");
    fs.file(format!("{domain}/0-0/nvm_active0/nvmem"), "firmware");
    fs.link("sys/bus/thunderbolt/devices/0-0", format!("{domain}/0-0"));

    let card = "sys/devices/pci0000:00/0000:00:02.0/drm/card1";
    fs.file(format!("{card}/card1-DP-1/status"), "connected");
    fs.file(format!("{card}/card1-DP-1/edid"), "binary");
    fs.link("sys/class/drm/card1-DP-1", format!("{card}/card1-DP-1"));
    fs.link("sys/class/drm/card1", card);
    let hwmon = "sys/devices/platform/coretemp.0/hwmon/hwmon5";
    fs.file(format!("{hwmon}/name"), "coretemp");
    fs.file(format!("{hwmon}/temp1_label"), "Package id 0");
    fs.file(format!("{hwmon}/temp1_input"), "48000");
    fs.file(format!("{hwmon}/temp1_crit"), "100000");
    fs.link("sys/class/hwmon/hwmon5", hwmon);
    let rapl = "sys/devices/virtual/powercap/intel-rapl/intel-rapl:0";
    fs.file(format!("{rapl}/name"), "package-0");
    fs.file(format!("{rapl}/energy_uj"), "123456");
    fs.link("sys/class/powercap/intel-rapl:0", rapl);
    let cpu = "sys/devices/system/cpu";
    fs.file(format!("{cpu}/cpufreq/policy0/scaling_cur_freq"), "1700000");
    fs.file(format!("{cpu}/cpufreq/policy0/cpuinfo_min_freq"), "400000");
    fs.file(
        format!("{cpu}/cpufreq/policy0/scaling_governor"),
        "powersave",
    );
    fs.link(
        format!("{cpu}/cpu0/cpufreq"),
        format!("{cpu}/cpufreq/policy0"),
    );
    let dmi = "sys/devices/virtual/dmi/id";
    fs.file(format!("{dmi}/sys_vendor"), "Dell Inc.");
    fs.file(format!("{dmi}/product_name"), "XPS 9320");
    fs.file(format!("{dmi}/product_serial"), "ABC1234");
    fs.file(format!("{dmi}/chassis_asset_tag"), "asset 7");
    fs.link("sys/class/dmi/id", dmi);
    fs.file("proc/stat", "cpu  1 2 3 4 5 6 7 8 0 0");
    fs.file("proc/sys/kernel/osrelease", "6.99.0-test");
    fs
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap().trim_end().to_owned()
}

#[test]
fn readers_see_the_same_machine_in_the_capture() {
    let fs = machine();
    let out = tempfile::tempdir().unwrap();
    let summary = capture(&fs.root(), out.path()).unwrap();
    assert!(summary.files > 20, "{summary:?}");
    let copy = Root::new(out.path());
    let labels = Labels::parse("0000:06:00.4-1  USB-C\ntypec:port0  USB-C\n");

    // The serial is redacted; everything else must match.
    let mut original = usb::read_sockets(&fs.root(), &labels);
    let Usb3Side::Paired(half) = &mut original[0].usb3 else {
        panic!("{:?}", original[0].usb3)
    };
    let hub = half.device.as_mut().unwrap();
    assert_eq!(hub.serial.as_deref(), Some("0123456789AB"));
    hub.serial = Some("REDACTED".into());
    assert_eq!(usb::read_sockets(&copy, &labels), original);
    let state = typec::read(&copy, &labels);
    assert_eq!(state, typec::read(&fs.root(), &labels));
    assert_eq!(state.ports[0].pd_contract.unwrap().source_pdo, Some(1));
    assert_eq!(thunderbolt::read(&copy), thunderbolt::read(&fs.root()));
}

#[test]
fn capture_layout_links_and_privacy() {
    let fs = machine();
    let out = tempfile::tempdir().unwrap();
    capture(&fs.root(), out.path()).unwrap();
    let o = out.path();

    // Same relative layout, symlinks relative and resolving inside.
    let link = fs::read_link(o.join("sys/bus/usb/devices/2-1")).unwrap();
    assert!(link.is_relative(), "{link:?}");
    assert_eq!(read(o.join("sys/bus/usb/devices/2-1/speed")), "5000");
    assert!(
        o.join("sys/bus/usb/devices/usb1/1-0:1.0/usb1-port1/peer/connect_type")
            .exists()
    );
    assert_eq!(
        read(o.join("sys/bus/usb/devices/1-0:1.0/usb1-port1/physical_location/dock")),
        "no"
    );
    // A `driver` link keeps its name without its target being copied.
    let driver = o.join("sys/bus/usb/devices/2-1:1.0/driver");
    assert_eq!(fs::read_link(&driver).unwrap().file_name().unwrap(), "hub");
    assert!(!driver.exists());
    assert_eq!(read(o.join("sys/class/power_supply/BAT0/capacity")), "78");
    assert_eq!(
        read(o.join("sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")),
        "1700000"
    );
    assert_eq!(read(o.join("sys/class/hwmon/hwmon5/temp1_input")), "48000");
    assert_eq!(
        read(o.join("sys/class/powercap/intel-rapl:0/energy_uj")),
        "123456"
    );
    assert_eq!(read(o.join("sys/class/drm/card1-DP-1/status")), "connected");
    assert_eq!(read(o.join("proc/stat")), "cpu  1 2 3 4 5 6 7 8 0 0");
    assert_eq!(read(o.join("sys/class/dmi/id/product_name")), "XPS 9320");

    // Identifiers replaced.
    for f in [
        "sys/bus/usb/devices/2-1/serial",
        "sys/class/power_supply/BAT0/serial_number",
        "sys/bus/thunderbolt/devices/0-0/unique_id",
    ] {
        assert_eq!(read(o.join(f)), "REDACTED", "{f}");
    }

    // Left out: other DMI fields, unused attributes, runtime PM, NVM,
    // uevent, and links readers don't follow.
    for f in [
        "sys/class/dmi/id/product_serial",
        "sys/class/dmi/id/chassis_asset_tag",
        "sys/class/drm/card1-DP-1/edid",
        "sys/class/hwmon/hwmon5/temp1_crit",
        "sys/devices/system/cpu/cpufreq/policy0/scaling_governor",
        "sys/class/typec/port0/power",
        "sys/bus/thunderbolt/devices/0-0/nvm_active0",
        "sys/class/power_supply/ucsi-source-psy-USBC000:001/uevent",
        "sys/class/typec/port0/port0-partner/pd1/device",
        "sys/class/drm/card1",
    ] {
        assert!(fs::symlink_metadata(o.join(f)).is_err(), "{f} was copied");
    }

    let about = read(o.join("CAPTURE.txt"));
    assert!(about.contains("kernel: 6.99.0-test"), "{about}");
    assert!(about.contains("machine: Dell Inc. XPS 9320"), "{about}");
    assert!(about.contains(env!("CARGO_PKG_VERSION")), "{about}");
}

#[test]
fn skips_large_and_write_only_files() {
    let fs = machine();
    let psy = fs.path().join("sys/class/power_supply/BAT0");
    fs::write(psy.join("big"), vec![b'x'; MAX_FILE as usize + 1]).unwrap();
    fs::write(psy.join("just_fits"), vec![b'x'; MAX_FILE as usize]).unwrap();
    fs::write(psy.join("trigger"), "1").unwrap();
    fs::set_permissions(psy.join("trigger"), fs::Permissions::from_mode(0o200)).unwrap();
    let out = tempfile::tempdir().unwrap();
    let summary = capture(&fs.root(), out.path()).unwrap();
    let copied = out.path().join("sys/class/power_supply/BAT0");
    assert!(!copied.join("big").exists());
    assert!(!copied.join("trigger").exists());
    assert_eq!(
        fs::metadata(copied.join("just_fits")).unwrap().len(),
        MAX_FILE
    );
    assert_eq!(summary.skipped, 2, "{summary:?}");
}

#[test]
fn refuses_a_non_empty_directory() {
    let fs = machine();
    let out = tempfile::tempdir().unwrap();
    fs::write(out.path().join("keep"), "").unwrap();
    assert!(capture(&fs.root(), out.path()).is_err());
    // A directory that doesn't exist yet is created.
    capture(&fs.root(), &out.path().join("new/xps")).unwrap();
    assert!(out.path().join("new/xps/CAPTURE.txt").exists());
}
