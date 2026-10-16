//! `plugii watch --record <file>` and `plugii replay <file>`: snapshots as
//! JSON lines.
//!
//! The first line is a [`Header`]. Every line after it is a [`Line`]: a full
//! keyframe (`{"t_ms":…,"snapshot":{…}}`) or a tick
//! (`{"t_ms":…,"tick":{"cpu":…,"power":…,"urbs":{…}}}`) holding only what
//! changes every second: the CPU sample, the power state (battery
//! percentage and flow included) and each device's URB counter. A tick
//! applies onto the last keyframe; anything else that changes (a device, a
//! PD contract, a kernel log line) writes a new keyframe, and so does a
//! keyframe [`KEYFRAME_EVERY_MS`] old, so a cut-off file still reads.
//!
//! Files whose name ends in `.gz` are gzip-compressed. Each line is flushed
//! as it is written (a gzip sync flush for `.gz`), so a crash loses at most
//! the line being written; the reader skips a cut-off last line, and reads a
//! gzip stream with no trailer up to where it stops.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};

use crate::capture::REDACTED;
use crate::model::{CpuSample, Power, Snapshot, Socket, Usb3Side, UsbDevice};

/// The header's `format`.
pub const FORMAT: &str = "plugii-record";
/// The header's `version`; the reader takes only this one.
pub const VERSION: u32 = 1;
/// A keyframe at least this often, even when nothing but ticks changed.
pub const KEYFRAME_EVERY_MS: u64 = 10 * 60 * 1000;

/// The first line of a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub format: String,
    pub version: u32,
    /// The plugii version that wrote it.
    pub plugii: String,
    pub host: String,
    /// The first snapshot's `taken_at_ms`.
    pub started_at_ms: u64,
}

impl Header {
    pub fn new(host: &str, started_at_ms: u64) -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            plugii: env!("CARGO_PKG_VERSION").into(),
            host: host.into(),
            started_at_ms,
        }
    }
}

/// One line after the header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// The snapshot's `taken_at_ms`.
    pub t_ms: u64,
    #[serde(flatten)]
    pub entry: Entry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// A whole snapshot.
    Snapshot(Box<Snapshot>),
    /// The fast-changing fields, onto the last keyframe.
    Tick(Tick),
}

impl Entry {
    pub fn is_keyframe(&self) -> bool {
        matches!(self, Entry::Snapshot(_))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    pub cpu: Option<CpuSample>,
    pub power: Power,
    /// `urbnum` of every device that has one, by sysfs name. The keyframe
    /// has the same devices with a counter; only the counts differ.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub urbs: BTreeMap<String, u64>,
}

/// Chooses keyframe or tick for each snapshot: no IO, see [`Recorder`].
#[derive(Debug, Clone, Default)]
pub struct Encoder {
    /// The last snapshot written, with the tick's fields blanked.
    last: Option<Snapshot>,
    last_keyframe_ms: u64,
}

impl Encoder {
    /// The line for `snap`: a keyframe for the first snapshot, when anything
    /// but the tick's fields changed, or when the last keyframe is
    /// [`KEYFRAME_EVERY_MS`] old; else a tick.
    pub fn encode(&mut self, snap: &Snapshot) -> Line {
        let slow = slow_part(snap);
        let fresh = snap.taken_at_ms < self.last_keyframe_ms.saturating_add(KEYFRAME_EVERY_MS);
        let entry = if fresh && self.last.as_ref() == Some(&slow) {
            let mut urbs = BTreeMap::new();
            for_each_device(snap, |d| {
                if let Some(n) = d.urbnum {
                    urbs.insert(d.sysfs_name.clone(), n);
                }
            });
            Entry::Tick(Tick {
                cpu: snap.cpu.clone(),
                power: snap.power.clone(),
                urbs,
            })
        } else {
            self.last = Some(slow);
            self.last_keyframe_ms = snap.taken_at_ms;
            Entry::Snapshot(Box::new(snap.clone()))
        };
        Line {
            t_ms: snap.taken_at_ms,
            entry,
        }
    }
}

/// `snap` without what a tick carries: time, CPU and power blanked, URB
/// counts kept only as there or not.
fn slow_part(snap: &Snapshot) -> Snapshot {
    let mut s = snap.clone();
    s.taken_at_ms = 0;
    s.cpu = None;
    s.power = Power {
        ac_online: None,
        battery: None,
        usb_c_sources: Vec::new(),
    };
    for_each_device_mut(&mut s, &mut |d| {
        if d.urbnum.is_some() {
            d.urbnum = Some(0);
        }
    });
    s
}

fn halves_mut(socket: &mut Socket) -> impl Iterator<Item = &mut UsbDevice> {
    let usb3 = match &mut socket.usb3 {
        Usb3Side::Paired(half) => half.device.as_mut(),
        _ => None,
    };
    socket.usb2.device.as_mut().into_iter().chain(usb3)
}

fn for_each_device_mut(snap: &mut Snapshot, f: &mut impl FnMut(&mut UsbDevice)) {
    fn walk(d: &mut UsbDevice, f: &mut impl FnMut(&mut UsbDevice)) {
        f(d);
        for c in &mut d.children {
            walk(c, f);
        }
    }
    for socket in &mut snap.sockets {
        for d in halves_mut(socket) {
            walk(d, f);
        }
    }
}

fn for_each_device(snap: &Snapshot, mut f: impl FnMut(&UsbDevice)) {
    fn walk(d: &UsbDevice, f: &mut impl FnMut(&UsbDevice)) {
        f(d);
        for c in &d.children {
            walk(c, f);
        }
    }
    for socket in &snap.sockets {
        let usb3 = match &socket.usb3 {
            Usb3Side::Paired(half) => half.device.as_ref(),
            _ => None,
        };
        for d in socket.usb2.device.iter().chain(usb3) {
            walk(d, &mut f);
        }
    }
}

/// Replaces every per-unit identifier the model holds with `REDACTED`: the
/// fields read from sysfs files [`crate::capture::redact`] redacts in a
/// capture (`serial` → [`UsbDevice::serial`], `unique_id` →
/// [`crate::model::TbDevice::unique_id`]), and the value of a kernel log
/// line's `SerialNumber:`.
pub fn redact(snap: &mut Snapshot) {
    for_each_device_mut(snap, &mut |d| {
        if d.serial.is_some() {
            d.serial = Some(REDACTED.into());
        }
    });
    if let Some(tb) = &mut snap.thunderbolt {
        for d in &mut tb.devices {
            if d.unique_id.is_some() {
                d.unique_id = Some(REDACTED.into());
            }
        }
    }
    for line in &mut snap.kernel_log.errors {
        if let Some(i) = line.message.find("SerialNumber: ") {
            line.message.truncate(i + "SerialNumber: ".len());
            line.message.push_str(REDACTED);
        }
    }
}

/// The sysfs file names behind the fields [`redact`] replaces; tests hold
/// them to [`crate::capture::redact`].
pub const REDACTED_SYSFS_FILES: &[&str] = &["serial", "unique_id"];

/// Where a recording goes: a file, gzip-compressed when its name ends in
/// `.gz`.
#[derive(Debug)]
pub enum Output {
    Plain(BufWriter<File>),
    Gzip(GzEncoder<BufWriter<File>>),
}

impl Output {
    /// Creates `path`. An existing file is an error unless `force`, which
    /// overwrites it.
    pub fn create(path: &Path, force: bool) -> io::Result<Self> {
        let mut opts = OpenOptions::new();
        opts.write(true);
        if force {
            opts.create(true).truncate(true);
        } else {
            opts.create_new(true);
        }
        let file = match opts.open(path) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(io::Error::new(
                    e.kind(),
                    "the file already exists; pass --force to overwrite it",
                ));
            }
            other => other?,
        };
        let out = BufWriter::new(file);
        Ok(if is_gzip(path) {
            Self::Gzip(GzEncoder::new(out, Compression::default()))
        } else {
            Self::Plain(out)
        })
    }

    /// Writes the gzip trailer, then flushes the file.
    pub fn finish(self) -> io::Result<()> {
        match self {
            Self::Plain(mut w) => w.flush(),
            Self::Gzip(gz) => gz.finish()?.flush(),
        }
    }
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(w) => w.write(buf),
            Self::Gzip(w) => w.write(buf),
        }
    }

    /// For gzip, a sync flush: everything written so far decompresses.
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(w) => w.flush(),
            Self::Gzip(w) => w.flush(),
        }
    }
}

fn is_gzip(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "gz")
}

/// Writes snapshots to `W`, one flushed line each; the header goes before
/// the first.
#[derive(Debug)]
pub struct Recorder<W: Write> {
    out: W,
    encoder: Encoder,
    redact: bool,
    started: bool,
}

impl<W: Write> Recorder<W> {
    /// `redact`: [`redact`] each snapshot before writing it.
    pub fn new(out: W, redact: bool) -> Self {
        Self {
            out,
            encoder: Encoder::default(),
            redact,
            started: false,
        }
    }

    pub fn record(&mut self, snap: &Snapshot) -> io::Result<()> {
        let redacted;
        let snap = if self.redact {
            let mut s = snap.clone();
            redact(&mut s);
            redacted = s;
            &redacted
        } else {
            snap
        };
        if !self.started {
            self.line(&Header::new(&snap.host, snap.taken_at_ms))?;
            self.started = true;
        }
        let line = self.encoder.encode(snap);
        self.line(&line)
    }

    fn line(&mut self, value: &impl Serialize) -> io::Result<()> {
        let mut text = serde_json::to_vec(value)?;
        text.push(b'\n');
        self.out.write_all(&text)?;
        self.out.flush()
    }

    pub fn into_inner(self) -> W {
        self.out
    }
}

impl Recorder<Output> {
    /// Ends the file: the gzip trailer for `.gz`.
    pub fn finish(self) -> io::Result<()> {
        self.out.finish()
    }
}

/// A recording read back: a snapshot per line.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub header: Header,
    pub snapshots: Vec<Snapshot>,
    /// Problems that didn't stop the read: a cut-off last line.
    pub warnings: Vec<String>,
}

/// Reads `path`, decompressing it when its name ends in `.gz`.
pub fn read(path: &Path) -> io::Result<Recording> {
    let file = File::open(path)?;
    let mut warnings = Vec::new();
    let bytes = if is_gzip(path) {
        let mut out = Vec::new();
        if let Err(e) = MultiGzDecoder::new(BufReader::new(file)).read_to_end(&mut out) {
            // Bytes before the error are in `out`: a stream cut off by a
            // crash still has every line flushed before it.
            if out.is_empty() {
                return Err(invalid(format!("not gzip data: {e}")));
            }
            warnings.push(format!("the gzip stream is cut off ({e})"));
        }
        out
    } else {
        let mut out = Vec::new();
        BufReader::new(file).read_to_end(&mut out)?;
        out
    };
    let mut recording = parse(&bytes)?;
    warnings.append(&mut recording.warnings);
    recording.warnings = warnings;
    Ok(recording)
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Rebuilds a snapshot per line: a keyframe as is, a tick onto the last
/// keyframe with `taken_at_ms` set to its `t_ms`. A last line that doesn't
/// parse and has no newline (cut off by a crash) is skipped with a warning;
/// anything else that doesn't parse is an error.
pub fn parse(bytes: &[u8]) -> io::Result<Recording> {
    let mut lines = bytes.split_inclusive(|&b| b == b'\n').enumerate();
    let header = match lines.next() {
        Some((_, line)) => header(line)?,
        None => return Err(invalid("empty file: not a plugii recording".into())),
    };
    let mut snapshots: Vec<Snapshot> = Vec::new();
    let mut warnings = Vec::new();
    let mut keyframe: Option<Snapshot> = None;
    for (i, raw) in lines {
        let n = i + 1;
        let complete = raw.ends_with(b"\n");
        if raw.trim_ascii().is_empty() {
            continue;
        }
        let line: Line = match serde_json::from_slice(raw) {
            Ok(line) => line,
            Err(_) if !complete => {
                warnings.push(format!("line {n} is cut off; skipped it"));
                break;
            }
            Err(e) => return Err(invalid(format!("line {n}: {e}"))),
        };
        let snap = match line.entry {
            Entry::Snapshot(s) => {
                keyframe = Some((*s).clone());
                *s
            }
            Entry::Tick(tick) => {
                let Some(base) = &keyframe else {
                    return Err(invalid(format!("line {n}: a tick before any snapshot")));
                };
                apply_tick(base, tick, line.t_ms)
            }
        };
        snapshots.push(snap);
    }
    Ok(Recording {
        header,
        snapshots,
        warnings,
    })
}

fn header(line: &[u8]) -> io::Result<Header> {
    let value: serde_json::Value = serde_json::from_slice(line)
        .map_err(|_| invalid("not a plugii recording: the first line isn't JSON".into()))?;
    if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
        return Err(invalid(format!(
            "not a plugii recording: the first line has no \"format\":\"{FORMAT}\""
        )));
    }
    match value.get("version").and_then(|v| v.as_u64()) {
        Some(v) if v == u64::from(VERSION) => {}
        Some(v) => {
            return Err(invalid(format!(
                "recording format version {v}; this plugii ({}) reads version {VERSION}",
                env!("CARGO_PKG_VERSION")
            )));
        }
        None => return Err(invalid("the recording's header has no version".into())),
    }
    serde_json::from_value(value).map_err(|e| invalid(format!("the recording's header: {e}")))
}

fn apply_tick(base: &Snapshot, tick: Tick, t_ms: u64) -> Snapshot {
    let mut s = base.clone();
    s.taken_at_ms = t_ms;
    s.cpu = tick.cpu;
    s.power = tick.power;
    for_each_device_mut(&mut s, &mut |d| {
        if let Some(n) = tick.urbs.get(&d.sysfs_name) {
            d.urbnum = Some(*n);
        }
    });
    s
}
