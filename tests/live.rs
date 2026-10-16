//! Smoke test against the running machine: the readers must not panic on a
//! real sysfs. Ignored by default since it depends on the hardware;
//! `cargo test --test live -- --ignored --nocapture` prints what it found.

use plugii::labels;
use plugii::model::{Usb3Side, UsbDevice};
use plugii::sysfs::{Root, thunderbolt, typec, usb};

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
fn readers_run_on_the_live_machine() {
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
            "  {} {} role={:?} charging={} contract={:?} cable={:?}",
            port.name,
            port.label.as_deref().unwrap_or(""),
            port.power_role,
            port.charging,
            port.pd_contract,
            port.cable_type
        );
        if let Some(p) = &port.partner {
            println!(
                "    partner pd={:?} rev={:?} alt-modes={} source={:?} sink={:?}",
                p.supports_pd, p.pd_revision, p.alt_modes, p.source_pdos, p.sink_pdos
            );
        }
    }

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
