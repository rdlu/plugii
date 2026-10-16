//! `watch --record` files: written and read back, plain and gzip, cut off,
//! with a bad header, with the 10-minute keyframe, and redacted.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::MultiGzDecoder;
use plugii::capture::{self, REDACTED};
use plugii::collect::{self, CollectOpts};
use plugii::labels;
use plugii::model::{CpuSample, Snapshot, TbDevice, Usb3Side, UsbDevice};
use plugii::record::{
    self, Encoder, Entry, Header, KEYFRAME_EVERY_MS, Output, REDACTED_SYSFS_FILES, Recorder,
};
use plugii::sysfs::Root;

/// 2025-10-16 12:00:00 UTC.
const T0: u64 = 1_760_616_000_000;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot(name: &str, labels_name: &str, t_ms: u64) -> Snapshot {
    let labels = labels::read(&fixtures().join("labels").join(labels_name)).unwrap();
    let dir = fixtures().join("kernel_log").join(labels_name);
    let log = (
        fs::read_to_string(dir.join("warnings.log")).unwrap(),
        fs::read_to_string(dir.join("all.log")).unwrap(),
    );
    let root = Root::new(fixtures().join("sysfs").join(name));
    let mut snap = collect::snapshot_with(&root, &labels, &CollectOpts::offline(), Some(log), None);
    snap.taken_at_ms = t_ms;
    snap
}

/// daisy-empty → daisy-dock → daisy-dock with only the CPU and power
/// changed → daisy-charger, a second apart.
fn session() -> Vec<Snapshot> {
    let empty = snapshot("daisy-empty", "daisy", T0);
    let dock = snapshot("daisy-dock", "daisy", T0 + 1000);
    let mut busy = dock.clone();
    busy.taken_at_ms = T0 + 2000;
    busy.cpu = Some(CpuSample {
        util_pct: 87,
        avg_khz: 3_712_000,
        max_khz: 4_200_000,
        min_khz: Some(400_000),
        package_w: Some(23.5),
        package_temp_c: Some(81.25),
    });
    let battery = busy.power.battery.get_or_insert(plugii::model::Battery {
        capacity_pct: None,
        status: None,
        charge_limit_pct: None,
        flow_mw: None,
    });
    battery.capacity_pct = Some(battery.capacity_pct.unwrap_or(50) + 1);
    battery.flow_mw = Some(-12_345);
    let charger = snapshot("daisy-charger", "daisy", T0 + 3000);
    vec![empty, dock, busy, charger]
}

fn record_to(path: &Path, snaps: &[Snapshot], redact: bool) {
    let mut rec = Recorder::new(Output::create(path, false).unwrap(), redact);
    for s in snaps {
        rec.record(s).unwrap();
    }
    rec.finish().unwrap();
}

/// The file's text, decompressed for `.gz`.
fn text(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = String::new();
        MultiGzDecoder::new(&bytes[..])
            .read_to_string(&mut out)
            .unwrap();
        out
    } else {
        String::from_utf8(bytes).unwrap()
    }
}

/// `K` for a keyframe, `t` for a tick, line by line after the header.
fn kinds(text: &str) -> String {
    text.lines()
        .skip(1)
        .map(|l| {
            let line: record::Line = serde_json::from_str(l).unwrap();
            if line.entry.is_keyframe() { 'K' } else { 't' }
        })
        .collect()
}

fn round_trip(name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    let snaps = session();
    record_to(&path, &snaps, false);
    let text = text(&path);
    assert_eq!(kinds(&text), "KKtK");
    let header: Header = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(header, Header::new(&snaps[0].host, T0));
    assert!(
        text.starts_with(r#"{"format":"plugii-record","version":1,"plugii":"#),
        "{}",
        &text[..80]
    );

    let read = record::read(&path).unwrap();
    assert_eq!(read.header, header);
    assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    assert_eq!(read.snapshots.len(), snaps.len());
    for (i, (got, want)) in read.snapshots.iter().zip(&snaps).enumerate() {
        assert_eq!(got, want, "snapshot {i}");
    }
}

#[test]
fn round_trip_plain() {
    round_trip("session.jsonl");
}

#[test]
fn round_trip_gzip() {
    round_trip("session.jsonl.gz");
    // It really is compressed.
    let dir = tempfile::tempdir().unwrap();
    let (plain, gz) = (dir.path().join("a.jsonl"), dir.path().join("a.jsonl.gz"));
    record_to(&plain, &session(), false);
    record_to(&gz, &session(), false);
    let (plain, gz) = (fs::metadata(plain).unwrap(), fs::metadata(gz).unwrap());
    assert!(
        gz.len() * 4 < plain.len(),
        "{} vs {}",
        gz.len(),
        plain.len()
    );
}

fn each_device(snap: &mut Snapshot, f: &mut impl FnMut(&mut UsbDevice)) {
    fn walk(d: &mut UsbDevice, f: &mut impl FnMut(&mut UsbDevice)) {
        f(d);
        for c in &mut d.children {
            walk(c, f);
        }
    }
    for socket in &mut snap.sockets {
        if let Some(d) = &mut socket.usb2.device {
            walk(d, f);
        }
        if let Usb3Side::Paired(h) = &mut socket.usb3
            && let Some(d) = &mut h.device
        {
            walk(d, f);
        }
    }
}

#[test]
fn urb_counters_ride_on_ticks() {
    let dock = snapshot("daisy-dock", "daisy", T0);
    let mut later = dock.clone();
    later.taken_at_ms = T0 + 1000;
    let mut bumped = 0;
    each_device(&mut later, &mut |d| {
        if let Some(n) = &mut d.urbnum {
            *n += 100;
            bumped += 1;
        }
    });
    assert!(bumped > 0);
    let mut enc = Encoder::default();
    assert!(enc.encode(&dock).entry.is_keyframe());
    let line = enc.encode(&later);
    let Entry::Tick(tick) = &line.entry else {
        panic!("URB counts alone made a keyframe");
    };
    assert_eq!(tick.urbs.len(), bumped);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("urbs.jsonl");
    record_to(&path, &[dock.clone(), later.clone()], false);
    assert_eq!(record::read(&path).unwrap().snapshots, [dock, later]);
}

#[test]
fn a_cut_off_last_line_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crash.jsonl");
    record_to(&path, &session(), false);
    let mut bytes = fs::read(&path).unwrap();
    bytes.truncate(bytes.len() - 40);
    fs::write(&path, &bytes).unwrap();
    let read = record::read(&path).unwrap();
    assert_eq!(read.snapshots, session()[..3]);
    assert_eq!(read.warnings, ["line 5 is cut off; skipped it"]);
}

#[test]
fn a_gzip_file_without_its_trailer_reads_up_to_the_last_flush() {
    // What a crash leaves: each line sync-flushed, no gzip trailer.
    let mut rec = Recorder::new(
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()),
        false,
    );
    for s in session() {
        rec.record(&s).unwrap();
    }
    // The bytes written so far, before dropping the encoder would finish it.
    let bytes = rec.into_inner().get_ref().clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crash.jsonl.gz");
    fs::write(&path, &bytes).unwrap();
    let read = record::read(&path).unwrap();
    assert_eq!(read.snapshots, session());
    assert_eq!(read.warnings.len(), 1, "{:?}", read.warnings);
    assert!(read.warnings[0].contains("gzip stream is cut off"));
}

fn read_err(text: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.jsonl");
    fs::write(&path, text).unwrap();
    record::read(&path).unwrap_err().to_string()
}

#[test]
fn bad_headers_are_clear_errors() {
    assert_eq!(read_err(""), "empty file: not a plugii recording");
    assert_eq!(
        read_err("hello\n"),
        "not a plugii recording: the first line isn't JSON"
    );
    assert_eq!(
        read_err("{\"format\":\"other\",\"version\":1}\n"),
        "not a plugii recording: the first line has no \"format\":\"plugii-record\""
    );
    let err = read_err(
        "{\"format\":\"plugii-record\",\"version\":2,\"plugii\":\"9.0.0\",\"host\":\"h\",\"started_at_ms\":0}\n",
    );
    assert!(
        err.starts_with("recording format version 2; this plugii (")
            && err.ends_with("reads version 1"),
        "{err}"
    );
    // A broken line in the middle is an error, not skipped.
    let header = serde_json::to_string(&Header::new("h", T0)).unwrap();
    let err = read_err(&format!("{header}\n{{\"t_ms\":1,\n{{}}\n"));
    assert!(err.starts_with("line 2: "), "{err}");
    let err = read_err(&format!(
        "{header}\n{{\"t_ms\":1,\"tick\":{{\"cpu\":null,\"power\":{{\"ac_online\":null,\"battery\":null,\"usb_c_sources\":[]}}}}}}\n"
    ));
    assert_eq!(err, "line 2: a tick before any snapshot");
}

#[test]
fn an_existing_file_is_kept_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.jsonl");
    fs::write(&path, "keep me\n").unwrap();
    let err = Output::create(&path, false).unwrap_err();
    assert_eq!(
        err.to_string(),
        "the file already exists; pass --force to overwrite it"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep me\n");
    let mut out = Output::create(&path, true).unwrap();
    out.write_all(b"new\n").unwrap();
    out.finish().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
}

#[test]
fn a_keyframe_at_least_every_ten_minutes() {
    let mut snap = snapshot("daisy-dock", "daisy", T0);
    let mut enc = Encoder::default();
    let mut kinds = String::new();
    for t in [
        0,
        1000,
        KEYFRAME_EVERY_MS - 1,
        KEYFRAME_EVERY_MS,
        KEYFRAME_EVERY_MS + 1000,
        2 * KEYFRAME_EVERY_MS - 1,
        2 * KEYFRAME_EVERY_MS,
    ] {
        snap.taken_at_ms = T0 + t;
        kinds.push(if enc.encode(&snap).entry.is_keyframe() {
            'K'
        } else {
            't'
        });
    }
    assert_eq!(kinds, "KttKttK");
}

/// daisy-dock with a made-up serial on every device and a Thunderbolt
/// device with a unique ID: captures are redacted already.
fn with_identifiers() -> Snapshot {
    let mut snap = snapshot("daisy-dock", "daisy", T0);
    let mut n = 0;
    each_device(&mut snap, &mut |d| {
        n += 1;
        d.serial = Some(format!("SECRET-SN-{n}"));
    });
    assert!(n > 3);
    snap.thunderbolt
        .get_or_insert_with(|| plugii::model::Thunderbolt {
            domains: Vec::new(),
            devices: Vec::new(),
        })
        .devices
        .push(TbDevice {
            route: "0-1".into(),
            unique_id: Some("SECRET-UUID".into()),
            vendor: Some("Dell".into()),
            name: Some("WD19TB".into()),
            authorized: true,
            generation: Some(3),
            link: None,
            boltd_stored: None,
        });
    snap.kernel_log.errors.push(plugii::model::LogLine {
        secs_since_boot: Some(5),
        message: "usb 3-1: SerialNumber: SECRET-LOG".into(),
        socket: None,
    });
    snap
}

#[test]
fn redact_replaces_every_identifier() {
    // The fields redacted come from the files a capture redacts.
    for name in REDACTED_SYSFS_FILES {
        assert!(capture::redact(name), "{name}");
    }

    let mut snap = with_identifiers();
    let before = snap.clone();
    record::redact(&mut snap);
    let json = serde_json::to_string(&snap).unwrap();
    assert!(!json.contains("SECRET"), "{json}");
    each_device(&mut snap, &mut |d| {
        assert_eq!(d.serial.as_deref(), Some(REDACTED));
    });
    let tb = snap.thunderbolt.as_ref().unwrap().devices.last().unwrap();
    assert_eq!(tb.unique_id.as_deref(), Some(REDACTED));
    assert_eq!(
        snap.kernel_log.errors.last().unwrap().message,
        "usb 3-1: SerialNumber: REDACTED"
    );

    // Nothing else changes: put the identifiers back and it's the same.
    let mut serials = Vec::new();
    let mut original = before.clone();
    each_device(&mut original, &mut |d| serials.push(d.serial.take()));
    let mut serials = serials.into_iter();
    each_device(&mut snap, &mut |d| d.serial = serials.next().unwrap());
    snap.thunderbolt
        .as_mut()
        .unwrap()
        .devices
        .last_mut()
        .unwrap()
        .unique_id = Some("SECRET-UUID".into());
    snap.kernel_log.errors.last_mut().unwrap().message = "usb 3-1: SerialNumber: SECRET-LOG".into();
    assert_eq!(snap, before);
}

#[test]
fn a_redacted_recording_has_no_identifiers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redacted.jsonl.gz");
    let first = with_identifiers();
    let mut second = first.clone();
    second.taken_at_ms += 1000;
    record_to(&path, &[first, second], true);
    let text = text(&path);
    assert!(!text.contains("SECRET"));
    assert!(text.contains(REDACTED));
    // Redacting doesn't make every line a keyframe.
    assert_eq!(kinds(&text), "Kt");
}
