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
