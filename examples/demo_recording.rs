//! Writes the docs site's demo recording: a deterministic `plugii watch
//! --record` session built from the ThinkPad fixtures, for `plugii replay`
//! to play in the VHS tapes under `site/demos/`.
//!
//! ```sh
//! cargo run --example demo_recording -- site/demos/daisy-session.jsonl.gz
//! ```
//!
//! About 75 s at the watcher's 1 s tick, from fixed timestamps, redacted:
//! nothing plugged in; the dock on the hinge port, whose hub first comes up
//! on its USB 2 half only, then copies over its Ethernet adapter for a while;
//! the dock unplugged; then a plain USB-C charger while a build loads the
//! CPU. Same input, same file, byte for byte.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use plugii::collect::{self, CollectOpts};
use plugii::labels;
use plugii::model::{CpuSample, LinkResets, Snapshot, Usb3Side, UsbDevice};
use plugii::record::{Output, Recorder};
use plugii::sysfs::Root;

/// 2025-10-16 12:00:00 UTC, as in the TUI tests.
const T0: u64 = 1_760_616_000_000;
/// Snapshots, one a second.
const SECONDS: u64 = 76;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot(name: &str) -> Snapshot {
    let labels = labels::read(&fixtures().join("labels/daisy")).expect("daisy labels");
    let dir = fixtures().join("kernel_log/daisy");
    let log = (
        fs::read_to_string(dir.join("warnings.log")).expect("warnings.log"),
        fs::read_to_string(dir.join("all.log")).expect("all.log"),
    );
    let root = Root::new(fixtures().join("sysfs").join(name));
    let mut snap = collect::snapshot_with(&root, &labels, &CollectOpts::offline(), Some(log), None);
    snap.host = "daisy".into();
    snap
}

/// Each device's URB counter goes up by `per_s(sysfs name)` per call.
fn add_urbs(d: &mut UsbDevice, counts: &mut HashMap<String, u64>, per_s: &dyn Fn(&str) -> u64) {
    let n = counts
        .entry(d.sysfs_name.clone())
        .or_insert(d.urbnum.unwrap_or(0));
    *n += per_s(&d.sysfs_name);
    d.urbnum = Some(*n);
    for child in &mut d.children {
        add_urbs(child, counts, per_s);
    }
}

fn main() -> std::io::Result<()> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("site/demos/daisy-session.jsonl.gz"));
    let empty = snapshot("daisy-empty");
    let dock = snapshot("daisy-dock");
    let charger = snapshot("daisy-charger");

    let mut recorder = Recorder::new(Output::create(&out, true)?, true);
    let mut counts = HashMap::new();
    for i in 0..SECONDS {
        let mut snap = match i {
            0..8 => empty.clone(),
            8..34 => dock.clone(),
            34..40 => empty.clone(),
            _ => charger.clone(),
        };
        snap.taken_at_ms = T0 + i * 1000;

        // The dock's SuperSpeed link takes a few seconds to come up: its
        // hub shows on the USB 2 half alone (stuck, in red) until then.
        let usb3_late = (8..12).contains(&i);
        // The Ethernet adapter behind the dock moves a file.
        let copying = (18..29).contains(&i);
        let per_s = |name: &str| match name {
            "7-1.2" if copying => 3800 + 450 * (i % 4),
            "7-1.2" => 90 + 15 * (i % 3),
            _ => 2,
        };
        for socket in &mut snap.sockets {
            if let Usb3Side::Paired(half) = &mut socket.usb3 {
                if usb3_late {
                    half.device = None;
                }
                if let Some(d) = &mut half.device {
                    add_urbs(d, &mut counts, &per_s);
                }
            }
            if let Some(d) = &mut socket.usb2.device {
                add_urbs(d, &mut counts, &per_s);
            }
        }

        // Link resets on the hinge port while the dock settles.
        let resets = match i {
            0..9 => 0,
            9..13 => 1,
            _ => 2,
        };
        if resets > 0 {
            snap.kernel_log.link_resets = vec![LinkResets {
                place: "USB-C (hinge)".into(),
                count: resets,
            }];
        }

        // A build starts once the charger is in; before that, light use.
        let load = if (48..70).contains(&i) { 1.0 } else { 0.15 };
        let wave = (i as f32 / 3.0).sin();
        let ghz = 1.3 + 2.7 * load + 0.25 * wave;
        snap.cpu = Some(CpuSample {
            util_pct: (12.0 + 80.0 * load + 5.0 * wave) as u8,
            avg_khz: (ghz * 1e6) as u32,
            max_khz: 5_100_000,
            min_khz: Some(400_000),
            package_w: Some(5.0 + 21.0 * load + 1.5 * wave),
            package_temp_c: Some(48.0 + 34.0 * load + 2.0 * wave),
        });
        // On battery it drains with the load; on AC the battery sits at its
        // 80 % charge limit ("Not charging"), so nothing flows.
        let on_ac = snap.power.ac_online == Some(true);
        if let Some(b) = &mut snap.power.battery {
            b.flow_mw = Some(if on_ac {
                0
            } else {
                -(6_500 + 700 * (i as i64 % 4))
            });
        }
        recorder.record(&snap)?;
    }
    recorder.finish()?;
    eprintln!("wrote {} ({SECONDS} snapshots)", out.display());
    Ok(())
}
