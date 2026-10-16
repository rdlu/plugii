//! The text report and `--json` for whole fixture machines: the XPS 9320
//! captures (with their labels and kernel log) and the hand-made power / CPU
//! scenarios, collected offline.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use plugii::collect::{self, CollectOpts};
use plugii::health;
use plugii::labels::{self, Labels};
use plugii::model::{Level, Snapshot};
use plugii::report;
use plugii::sysfs::{Root, cpu};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn kernel_log(name: &str) -> (String, String) {
    let dir = fixtures().join("kernel_log").join(name);
    (
        fs::read_to_string(dir.join("warnings.log")).unwrap(),
        fs::read_to_string(dir.join("all.log")).unwrap(),
    )
}

fn collect(name: &str, labels: &Labels, log: Option<&str>) -> (Root, Snapshot) {
    let root = Root::new(fixtures().join("sysfs").join(name));
    let mut snap = collect::snapshot_with(
        &root,
        labels,
        &CollectOpts::offline(),
        Some(log.map(kernel_log).unwrap_or_default()),
        None,
    );
    snap.taken_at_ms = 0;
    (root, snap)
}

fn xps(name: &str) -> Snapshot {
    let labels = labels::read(&fixtures().join("labels/xps")).unwrap();
    collect(name, &labels, Some("xps")).1
}

/// A CPU sample from the fixture's counters plus `busy` and `idle` jiffies
/// over half a second, with 2 J of package energy where RAPL is readable.
fn with_cpu(root: &Root, snap: &mut Snapshot, busy: u64, idle: u64) {
    let before = cpu::read_counters(root).unwrap();
    let mut after = before;
    after.stat[0] += busy;
    after.stat[3] += idle;
    after.energy_uj = before.energy_uj.map(|e| e + 2_000_000);
    snap.cpu = Some(cpu::sample(
        root,
        &before,
        &after,
        Duration::from_millis(500),
    ));
}

fn scenario(name: &str, busy: u64, idle: u64) -> Snapshot {
    let (root, mut snap) = collect(name, &Labels::default(), None);
    with_cpu(&root, &mut snap, busy, idle);
    snap
}

fn text(snap: &Snapshot) -> String {
    report::render(snap, &health::check(snap), false)
}

fn json(snap: &Snapshot) -> serde_json::Value {
    report::json(snap, &health::check(snap))
}

#[test]
fn xps_report() {
    let snap = xps("xps");
    insta::assert_snapshot!(text(&snap));
    insta::assert_json_snapshot!(json(&snap));
}

#[test]
fn xps_stale_ucsi_report() {
    let snap = xps("xps-stale-ucsi");
    let findings = health::check(&snap);
    assert!(
        findings
            .iter()
            .any(|f| f.level == Level::Warn && f.message.contains("looks stale")),
        "{findings:#?}"
    );
    insta::assert_snapshot!(text(&snap));
    insta::assert_json_snapshot!(json(&snap));
}

/// The faults log (resets on `usb 2-1`, `2-1.3`) against the XPS sockets:
/// bus 2 port 1 is the left socket's USB 3 half.
#[test]
fn kernel_log_names_sockets_by_either_half() {
    let labels = labels::read(&fixtures().join("labels/xps")).unwrap();
    let (_, snap) = collect("xps", &labels, Some("faults"));
    let left = snap
        .kernel_log
        .link_resets
        .iter()
        .find(|r| r.place == "USB-C (left)");
    assert_eq!(left.map(|r| r.count), Some(3), "{:#?}", snap.kernel_log);
    let text = text(&snap);
    assert!(text.contains("USB link reset(s): "), "{text}");
    assert!(text.contains("USB-C (left) ×3"), "{text}");
}

#[test]
fn prochot_report_fails_on_throttle() {
    let snap = scenario("prochot", 400, 0);
    let findings = health::check(&snap);
    assert!(
        findings
            .iter()
            .any(|f| f.level == Level::Fail && f.message.contains("minimum clock")),
        "{findings:#?}"
    );
    insta::assert_snapshot!(text(&snap));
    insta::assert_json_snapshot!(json(&snap));
}

#[test]
fn intel_ac_usbc_report() {
    let snap = scenario("intel-ac-usbc", 300, 100);
    insta::assert_snapshot!(text(&snap));
}

#[test]
fn intel_on_battery_report() {
    let snap = scenario("intel-on-battery", 120, 280);
    insta::assert_snapshot!(text(&snap));
}

#[test]
fn amd_battery_report_with_kernel_faults() {
    let (root, mut snap) = collect("amd-battery", &Labels::default(), Some("faults"));
    with_cpu(&root, &mut snap, 10, 30);
    let findings = health::check(&snap);
    assert!(
        findings
            .iter()
            .any(|f| f.level == Level::Fail && f.message.contains("error line(s)"))
    );
    insta::assert_snapshot!(text(&snap));
}

#[test]
fn desktop_report() {
    let snap = scenario("desktop", 200, 200);
    insta::assert_snapshot!(text(&snap));
}

#[test]
fn energy_unreadable_report() {
    let snap = scenario("energy-unreadable", 200, 200);
    insta::assert_snapshot!(text(&snap));
}
