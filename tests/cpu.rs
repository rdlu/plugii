use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use plugii::sysfs::Root;
use plugii::sysfs::cpu::{self, CpuCounters};

fn fixture(name: &str) -> Root {
    Root::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sysfs")
            .join(name),
    )
}

const HALF_SECOND: Duration = Duration::from_millis(500);

/// `before` plus `busy` jiffies of user time, `idle` of idle time, and the
/// RAPL counter moved to `energy_uj`.
fn advance(before: &CpuCounters, busy: u64, idle: u64, energy_uj: Option<u64>) -> CpuCounters {
    let mut after = *before;
    after.stat[0] += busy;
    after.stat[3] += idle;
    after.energy_uj = energy_uj;
    after
}

#[test]
fn counters_from_proc_stat_and_package_zone() {
    let counters = cpu::read_counters(&fixture("intel-ac-usbc")).unwrap();
    assert_eq!(
        counters,
        CpuCounters {
            stat: [6781794, 28472, 1057813, 23076727, 4750767, 177814, 55769, 0],
            // intel-rapl:0, not intel-rapl-mmio:0, which also says package-0.
            energy_uj: Some(220786159822),
            energy_range_uj: Some(262143328850),
        }
    );
}

#[test]
fn intel_on_ac_sample() {
    let root = fixture("intel-ac-usbc");
    let before = cpu::read_counters(&root).unwrap();
    let after = advance(&before, 120, 280, Some(220786159822 + 10_000_000));
    insta::assert_json_snapshot!(cpu::sample(&root, &before, &after, HALF_SECOND));
}

#[test]
fn prochot_busy_at_minimum_clock() {
    let root = fixture("prochot");
    let before = cpu::read_counters(&root).unwrap();
    let after = advance(&before, 400, 0, Some(900_000_000 + 4_000_000));
    let sample = cpu::sample(&root, &before, &after, HALF_SECOND);
    insta::assert_json_snapshot!(sample);
    assert_eq!(sample.util_pct, 100);
    assert_eq!(Some(sample.avg_khz), sample.min_khz);
}

#[test]
fn amd_energy_counter_wraps() {
    let root = fixture("amd-battery");
    let before = cpu::read_counters(&root).unwrap();
    assert_eq!(before.energy_uj, Some(4294967000));
    // 295 µJ to the top of the range, then 7 499 705 more: 7.5 J in 0.5 s.
    let after = advance(&before, 10, 30, Some(7_499_705));
    let sample = cpu::sample(&root, &before, &after, HALF_SECOND);
    insta::assert_json_snapshot!(sample);
    assert_eq!(sample.package_w, Some(15.0));
    assert_eq!(sample.package_temp_c, Some(61.25));
}

#[test]
fn energy_backwards_without_range_is_unknown() {
    let root = fixture("amd-battery");
    let mut before = cpu::read_counters(&root).unwrap();
    before.energy_range_uj = None;
    let after = advance(&before, 10, 30, Some(5));
    assert_eq!(
        cpu::sample(&root, &before, &after, HALF_SECOND).package_w,
        None
    );
}

#[test]
fn desktop_without_rapl_uses_zenpower_tdie() {
    let root = fixture("desktop");
    let before = cpu::read_counters(&root).unwrap();
    assert_eq!(before.energy_uj, None);
    let after = advance(&before, 50, 50, None);
    insta::assert_json_snapshot!(cpu::sample(&root, &before, &after, HALF_SECOND));
}

#[test]
fn unreadable_energy_gives_no_package_power() {
    let root = fixture("energy-unreadable");
    let before = cpu::read_counters(&root).unwrap();
    assert_eq!(before.energy_uj, None);
    assert_eq!(before.energy_range_uj, Some(262143328850));
    let sample = cpu::sample(&root, &before, &before, HALF_SECOND);
    assert_eq!(sample.package_w, None);
    // No jiffies passed: 0 %, like the script.
    assert_eq!(sample.util_pct, 0);
    assert_eq!(sample.package_temp_c, Some(50.0));
}

#[test]
fn iowait_is_not_busy() {
    let root = fixture("intel-on-battery");
    let before = cpu::read_counters(&root).unwrap();
    let mut after = before;
    after.stat[2] += 25; // system
    after.stat[4] += 75; // iowait
    assert_eq!(
        cpu::sample(&root, &before, &after, HALF_SECOND).util_pct,
        25
    );
}

#[test]
fn no_proc_stat_no_counters() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(cpu::read_counters(&Root::new(dir.path())), None);
    fs::create_dir(dir.path().join("proc")).unwrap();
    fs::write(dir.path().join("proc/stat"), "intr 1 2 3\n").unwrap();
    assert_eq!(cpu::read_counters(&Root::new(dir.path())), None);
}

#[test]
fn measure_samples_over_a_window() {
    let sample = cpu::measure(&fixture("intel-ac-usbc"), Duration::from_millis(5)).unwrap();
    // The fixture doesn't change between reads.
    assert_eq!(sample.util_pct, 0);
    assert_eq!(sample.package_w, Some(0.0));
    assert_eq!(sample.max_khz, 3_090_372);
}
