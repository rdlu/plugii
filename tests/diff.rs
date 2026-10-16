//! Events between fixture captures, both ways: what the Events tab would show
//! for plugging the dock or charger into daisy, emptying the XPS, and
//! swapping its dock and monitor between sockets.

use std::fs;
use std::path::{Path, PathBuf};

use plugii::collect::{self, CollectOpts};
use plugii::diff::{self, EventKind};
use plugii::labels;
use plugii::model::Snapshot;
use plugii::sysfs::Root;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A capture with its machine's labels and kernel log, taken at `t_ms`.
fn collect(machine: &str, name: &str, t_ms: u64) -> Snapshot {
    let labels = labels::read(&fixtures().join("labels").join(machine)).unwrap();
    let log = fixtures().join("kernel_log").join(machine);
    let mut snap = collect::snapshot_with(
        &Root::new(fixtures().join("sysfs").join(name)),
        &labels,
        &CollectOpts::offline(),
        Some((
            fs::read_to_string(log.join("warnings.log")).unwrap(),
            fs::read_to_string(log.join("all.log")).unwrap(),
        )),
        None,
    );
    snap.taken_at_ms = t_ms;
    snap
}

/// `old → new` as the Events tab lists it, one `{sign} {text}` per line.
fn lines(machine: &str, old: &str, new: &str) -> String {
    let events = diff::diff(&collect(machine, old, 1000), &collect(machine, new, 2000));
    assert!(events.iter().all(|e| e.t_ms == 2000), "{events:#?}");
    events
        .iter()
        .map(|e| e.line())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn identical_snapshots_have_no_events() {
    for (machine, name) in [
        ("daisy", "daisy-dock"),
        ("daisy", "daisy-charger"),
        ("xps", "xps"),
        ("xps", "xps-swapped"),
    ] {
        let snap = collect(machine, name, 1000);
        assert_eq!(diff::diff(&snap, &snap), [], "{name}");
    }
}

#[test]
fn daisy_dock_plugged_in() {
    let events = diff::diff(
        &collect("daisy", "daisy-empty", 1000),
        &collect("daisy", "daisy-dock", 2000),
    );
    let hub = events
        .iter()
        .find(|e| e.text.starts_with("05e3:0610"))
        .unwrap();
    assert_eq!(hub.kind, EventKind::Added);
    assert_eq!(hub.socket.as_deref(), Some("USB-C (hinge)"));
    insta::assert_snapshot!(lines("daisy", "daisy-empty", "daisy-dock"));
}

#[test]
fn daisy_dock_unplugged() {
    insta::assert_snapshot!(lines("daisy", "daisy-dock", "daisy-empty"));
}

#[test]
fn daisy_charger_plugged_in() {
    insta::assert_snapshot!(lines("daisy", "daisy-empty", "daisy-charger"));
}

#[test]
fn daisy_charger_unplugged() {
    insta::assert_snapshot!(lines("daisy", "daisy-charger", "daisy-empty"));
}

#[test]
fn xps_everything_unplugged() {
    insta::assert_snapshot!(lines("xps", "xps", "xps-empty"));
}

#[test]
fn xps_everything_plugged_in() {
    insta::assert_snapshot!(lines("xps", "xps-empty", "xps"));
}

/// Dock and monitor trade sockets: the dock's tree leaves the left socket
/// and arrives on the right, charging moves from port1 to port0.
#[test]
fn xps_swapped() {
    insta::assert_snapshot!(lines("xps", "xps", "xps-swapped"));
}

#[test]
fn xps_swapped_back() {
    insta::assert_snapshot!(lines("xps", "xps-swapped", "xps"));
}

/// A daisy capture with daisy's labels plus `extra` lines.
fn collect_labelled(name: &str, extra: &str, t_ms: u64) -> Snapshot {
    let text = fs::read_to_string(fixtures().join("labels/daisy")).unwrap() + extra;
    let mut snap = collect::snapshot_with(
        &Root::new(fixtures().join("sysfs").join(name)),
        &labels::Labels::parse(&text),
        &CollectOpts::offline(),
        None,
        None,
    );
    snap.taken_at_ms = t_ms;
    snap
}

fn labelled_lines(old: &str, new: &str, extra: &str) -> Vec<String> {
    let events = diff::diff(
        &collect_labelled(old, extra, 1000),
        &collect_labelled(new, extra, 2000),
    );
    events
        .iter()
        .filter(|e| e.socket.as_deref() == Some("USB-C (hinge)"))
        .map(|e| e.line())
        .collect()
}

/// A labelled hub leaving or arriving is one event, by name, with the
/// count of devices below it; the unlabelled USB 3 half keeps its wording.
#[test]
fn labelled_hub_collapses_to_one_event() {
    let switch = "device:05e3:0610  USB switch (desk)\n";
    let left = diff::diff(
        &collect_labelled("daisy-dock", switch, 1000),
        &collect_labelled("daisy-empty", switch, 2000),
    );
    let hub = left
        .iter()
        .find(|e| e.text.starts_with("USB switch (desk)"))
        .unwrap();
    assert_eq!(hub.kind, EventKind::Removed);
    assert_eq!(hub.socket.as_deref(), Some("USB-C (hinge)"));
    assert_eq!(
        labelled_lines("daisy-dock", "daisy-empty", switch),
        [
            "− 05e3:0626 USB3.1 Hub @5G on USB-C (hinge) with 1 device",
            "− USB switch (desk): 1 device left",
        ]
    );
    assert_eq!(
        labelled_lines("daisy-empty", "daisy-dock", switch),
        [
            "+ 05e3:0626 USB3.1 Hub @5G on USB-C (hinge) with 1 device",
            "+ USB switch (desk) @480M on USB-C (hinge), 1 device",
        ]
    );
}

/// A labelled device that isn't a hub with devices below it is named by its
/// label in the usual wording.
#[test]
fn labelled_devices_are_named_by_label() {
    let labels = "device:0b95:1790  Ethernet (desk)\ndevice:05E3:0608  inner hub\n";
    assert_eq!(
        labelled_lines("daisy-dock", "daisy-empty", labels),
        [
            "− 05e3:0626 USB3.1 Hub @5G on USB-C (hinge) with 1 device",
            "− 05e3:0610 USB2.1 Hub @480M on USB-C (hinge) with 1 device",
        ]
    );
    // The dock's USB 3 hub stays and only its Ethernet adapter goes.
    let dock = collect_labelled("daisy-dock", labels, 1000);
    let mut without = dock.clone();
    without.taken_at_ms = 2000;
    for socket in &mut without.sockets {
        if let plugii::model::Usb3Side::Paired(h) = &mut socket.usb3
            && let Some(hub) = &mut h.device
        {
            hub.children.clear();
        }
    }
    let lines: Vec<String> = diff::diff(&dock, &without)
        .iter()
        .map(|e| e.line())
        .collect();
    assert_eq!(lines, ["− Ethernet (desk) @5G on USB-C (hinge)"]);
}

/// New kernel errors and link resets are problems, counted by the increase.
#[test]
fn kernel_log_problems() {
    let old = collect("xps", "xps", 1000);
    let mut new = old.clone();
    let faults = fixtures().join("kernel_log/faults");
    let with_faults = collect::snapshot_with(
        &Root::new(fixtures().join("sysfs/xps")),
        &labels::read(&fixtures().join("labels/xps")).unwrap(),
        &CollectOpts::offline(),
        Some((
            fs::read_to_string(faults.join("warnings.log")).unwrap(),
            fs::read_to_string(faults.join("all.log")).unwrap(),
        )),
        None,
    );
    new.kernel_log = with_faults.kernel_log;
    let events = diff::diff(&old, &new);
    assert!(events.iter().all(|e| e.kind == EventKind::Problem));
    let resets = events
        .iter()
        .find(|e| e.text == "3 link resets on USB-C (left)")
        .unwrap_or_else(|| panic!("{events:#?}"));
    assert_eq!(resets.socket.as_deref(), Some("USB-C (left)"));
    assert_eq!(
        events
            .iter()
            .filter(|e| !e.text.contains("link reset"))
            .count(),
        new.kernel_log.errors.len()
    );

    // One more reset on the left: only the increase.
    let mut later = new.clone();
    later
        .kernel_log
        .link_resets
        .iter_mut()
        .find(|r| r.place == "USB-C (left)")
        .unwrap()
        .count += 1;
    let events = diff::diff(&new, &later);
    assert_eq!(
        events.iter().map(|e| e.line()).collect::<Vec<_>>(),
        ["! 1 link reset on USB-C (left)"]
    );
}
