//! Runs the display, power, CPU and kernel log readers against this machine.
//! `cargo test --test live_m3 -- --ignored --nocapture` prints what they saw.

use std::time::Duration;

use plugii::sysfs::{Root, cpu, drm, kernel_log, power};

#[test]
#[ignore = "reads the live machine"]
fn readers_run_on_this_machine() {
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
