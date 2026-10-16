//! Smoke tests against the running machine: the readers must not panic on a
//! real sysfs. Ignored by default since they depend on the hardware;
//! `cargo test --test live -- --ignored --nocapture` prints what they found.

use std::time::Duration;

use plugii::labels;
use plugii::model::{Usb3Side, UsbDevice};
use plugii::sysfs::{Root, cpu, drm, kernel_log, power, thunderbolt, typec, usb};

fn print_tree(device: &UsbDevice, depth: usize) {
    println!(
        "      {:indent$}└ {:04x}:{:04x} {} (capable {}, lanes {:?}/{:?}, driver {}) {}",
        "",
        device.vendor_id,
        device.product_id,
        device.speed.label(),
        device.capable_speed.map_or("?".into(), |s| s.label()),
        device.rx_lanes,
        device.tx_lanes,
        device.driver.as_deref().unwrap_or("-"),
        device.product.as_deref().unwrap_or(""),
        indent = depth * 2
    );
    for child in &device.children {
        print_tree(child, depth + 1);
    }
}

#[test]
#[ignore = "reads the running machine's sysfs"]
fn usb_typec_and_thunderbolt_readers() {
    let root = Root::live();
    let labels = labels::config_dir()
        .and_then(|dir| labels::load(&dir).ok())
        .unwrap_or_default();

    println!("USB sockets:");
    for socket in usb::read_sockets(&root, &labels) {
        println!(
            "  {} {} (USB-C: {})",
            socket.label.as_deref().unwrap_or("unlabeled"),
            socket.key,
            socket.typec_port.as_deref().unwrap_or("-")
        );
        match &socket.usb3 {
            Usb3Side::Paired(half) => {
                println!(
                    "    USB 3 half {}-{} ({})",
                    half.bus,
                    half.port,
                    half.port_state.as_deref().unwrap_or("?")
                );
                if let Some(d) = &half.device {
                    print_tree(d, 0);
                }
            }
            other => println!("    USB 3 half: {other:?}"),
        }
        let half = &socket.usb2;
        println!(
            "    USB 2 half {}-{} ({})",
            half.bus,
            half.port,
            half.port_state.as_deref().unwrap_or("?")
        );
        if let Some(d) = &half.device {
            print_tree(d, 0);
        }
    }

    let state = typec::read(&root, &labels);
    println!("USB-C (UCSI present: {}):", state.ucsi_present);
    for port in &state.ports {
        println!(
            "  {} {} role={:?} charging={} contract={:?} cable={:?} pd_inconsistent={}",
            port.name,
            port.label.as_deref().unwrap_or(""),
            port.power_role,
            port.charging,
            port.pd_contract,
            port.cable_type,
            port.pd_inconsistent
        );
        if let Some(p) = &port.partner {
            println!(
                "    partner pd={:?} rev={:?} alt-modes={} source={:?} sink={:?}",
                p.supports_pd, p.pd_revision, p.alt_modes, p.source_pdos, p.sink_pdos
            );
        }
    }

    // Must name the same ports as the typec reader's charging ones.
    println!("power from USB-C: {:?}", power::read(&root).usb_c_sources);

    match thunderbolt::read(&root) {
        None => println!("Thunderbolt: none"),
        Some(tb) => {
            println!("Thunderbolt:");
            for d in &tb.domains {
                println!("  {d:?}");
            }
            for d in &tb.devices {
                println!("  {d:?}");
            }
        }
    }
}

#[test]
#[ignore = "reads the running machine's sysfs and kernel log"]
fn display_power_cpu_and_kernel_log_readers() {
    let root = Root::live();

    let mut displays = drm::read(&root);
    if let Some(json) = drm::query_niri() {
        drm::apply_niri(&mut displays, &drm::niri_outputs(&json));
    }
    println!("displays: {displays:#?}");

    println!("power: {:#?}", power::read(&root));

    println!("cpu counters: {:?}", cpu::read_counters(&root));
    println!(
        "cpu: {:#?}",
        cpu::measure(&root, Duration::from_millis(500))
    );

    let (warnings, all) = kernel_log::query();
    let log = kernel_log::parse(&warnings, &all, |_| None);
    println!(
        "kernel log: {} warning lines, {} lines in all; {} errors, {} benign, resets {:?}",
        warnings.lines().count(),
        all.lines().count(),
        log.errors.len(),
        log.benign_count,
        log.link_resets,
    );
    for line in log.errors.iter().rev().take(5).rev() {
        println!("  {line:?}");
    }
}

#[test]
#[ignore = "runs plugii against the running machine"]
fn json_report_parses() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_plugii"))
        .arg("--json")
        .output()
        .unwrap();
    // Exit 1 only means a finding needs a look; anything else is a crash.
    assert!(
        matches!(out.status.code(), Some(0 | 1)),
        "{:?}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let snapshot: plugii::model::Snapshot =
        serde_json::from_value(value["snapshot"].clone()).unwrap();
    let findings: Vec<plugii::model::Finding> =
        serde_json::from_value(value["findings"].clone()).unwrap();
    let bad = findings.iter().any(|f| f.level != plugii::model::Level::Ok);
    assert_eq!(out.status.code(), Some(i32::from(bad)));
    assert!(!snapshot.host.is_empty());
    println!(
        "{} findings on {} sockets",
        findings.len(),
        snapshot.sockets.len()
    );
}

#[test]
#[ignore = "watches the running machine"]
fn watcher_sends_snapshots() {
    use plugii::watch::{self, Update};

    let labels = labels::config_dir()
        .and_then(|dir| labels::load(&dir).ok())
        .unwrap_or_default();
    let started = std::time::Instant::now();
    let rx = watch::spawn(
        Root::live(),
        std::sync::Arc::new(std::sync::RwLock::new(labels)),
    );
    let mut snapshots = Vec::new();
    while snapshots.len() < 2 {
        let left = Duration::from_secs(3).saturating_sub(started.elapsed());
        match rx.recv_timeout(left).expect("two snapshots within 3 s") {
            Update::Snapshot(snap) => {
                println!(
                    "{:?}: {} sockets, {} kernel errors, cpu {:?}",
                    started.elapsed(),
                    snap.sockets.len(),
                    snap.kernel_log.errors.len(),
                    snap.cpu
                );
                snapshots.push(snap);
            }
            Update::Warning(w) => println!("warning: {w}"),
        }
    }
    assert!(snapshots[1].taken_at_ms >= snapshots[0].taken_at_ms);
    // The first scan has no counters to compare; the next one does.
    assert!(snapshots[0].cpu.is_none());
    assert!(snapshots[1].cpu.is_some());
}

#[test]
#[ignore = "watches the running machine"]
fn records_three_seconds_from_the_watcher() {
    use plugii::record::{self, Output, Recorder};
    use plugii::watch::{self, Update};

    let labels = labels::config_dir()
        .and_then(|dir| labels::load(&dir).ok())
        .unwrap_or_default();
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("live.jsonl");
    let gz = dir.path().join("live.jsonl.gz");
    // The same recorder the TUI writes with, one per file.
    let mut recorders = [
        Recorder::new(Output::create(&plain, false).unwrap(), true),
        Recorder::new(Output::create(&gz, false).unwrap(), true),
    ];
    let rx = watch::spawn(
        Root::live(),
        std::sync::Arc::new(std::sync::RwLock::new(labels)),
    );
    let started = std::time::Instant::now();
    while let Some(left) = Duration::from_secs(3).checked_sub(started.elapsed()) {
        match rx.recv_timeout(left) {
            Ok(Update::Snapshot(snap)) => {
                for r in &mut recorders {
                    r.record(&snap).unwrap();
                }
            }
            Ok(Update::Warning(w)) => println!("warning: {w}"),
            Err(_) => break,
        }
    }
    for r in recorders {
        r.finish().unwrap();
    }
    for path in [&plain, &gz] {
        let bytes = std::fs::metadata(path).unwrap().len();
        let read = record::read(path).unwrap();
        println!(
            "{}: {} bytes, {} lines after the header, warnings {:?}",
            path.file_name().unwrap().to_string_lossy(),
            bytes,
            read.snapshots.len(),
            read.warnings
        );
        assert!(read.snapshots.len() >= 2, "{}", read.snapshots.len());
        assert!(read.warnings.is_empty());
    }
    let text = std::fs::read_to_string(&plain).unwrap();
    let kinds: String = text
        .lines()
        .skip(1)
        .map(|l| {
            if l.contains("\"snapshot\":") {
                'K'
            } else {
                't'
            }
        })
        .collect();
    println!(
        "plain: {} lines; keyframes and ticks: {kinds}",
        text.lines().count()
    );
}

/// The real `--firmware` collection: DMI, the vendor catalog over the
/// network (into a fresh cache, so it is fetched), and fwupdmgr.
#[test]
#[ignore = "uses the network and the running machine's DMI and fwupd"]
fn firmware_collection() {
    use plugii::firmware::{self, FwupdInput};
    use plugii::vendor::{Cache, Http};

    let root = Root::live();
    let labels = labels::config_dir()
        .and_then(|dir| labels::load(&dir).ok())
        .unwrap_or_default();
    let cache_dir = tempfile::TempDir::new().unwrap();
    let cache = Cache::new(Some(cache_dir.path().to_owned()), false);
    let started = std::time::Instant::now();
    let fw = firmware::collect(&root, &labels, &Http, &cache, FwupdInput::Run);
    println!("collected in {:?}", started.elapsed());
    println!("{fw:#?}");
    for entry in std::fs::read_dir(cache_dir.path()).unwrap() {
        let path = entry.unwrap().path();
        println!(
            "cached {}:\n{}",
            path.display(),
            std::fs::read_to_string(&path).unwrap()
        );
    }
    assert!(fw.dmi.sys_vendor.is_some(), "{:?}", fw.dmi);
    assert!(fw.dmi.bios_version.is_some(), "{:?}", fw.dmi);
}

/// Dell's real catalogs for the XPS 9320 (system ID 0AF3), whatever this
/// machine is: both cabs as Dell ships them, through the cab crate.
#[test]
#[ignore = "uses the network"]
fn dell_catalog_for_the_xps_9320() {
    use plugii::vendor::{Http, dell};

    let bios = dell::fetch_latest("0AF3", &Http).unwrap();
    println!("{bios:#?}");
    assert!(bios.version.starts_with("2."), "{bios:?}");
    assert!(!bios.package_id.is_empty());
}
