//! `plugii capture <dir>`: copy the sysfs and procfs files the readers use
//! into `<dir>`, laid out as under `/` (`<dir>/sys/...`, `<dir>/proc/stat`),
//! so `Root::new(dir)` reads the capture like the live machine.
//!
//! sysfs is mostly symlinks into `sys/devices/...`. Directories are copied
//! under their real paths and the symlinks the readers follow (`sys/class/*`
//! and `sys/bus/*/devices` entries, `peer`, `driver`, `usb_power_delivery`,
//! `cpufreq`) are recreated with their original relative targets, which then
//! resolve inside the capture. Nothing else is followed: `subsystem`,
//! `firmware_node`, `device`, `power` and the like are left out, and a
//! `driver` link is kept only for its name, without its target.
//!
//! Captures end up in a public repository, so serial numbers and unique IDs
//! are replaced with `REDACTED`.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::sysfs::{Root, read};

/// Files larger than this are skipped: nothing the readers use comes close.
pub const MAX_FILE: u64 = 64 * 1024;

/// What a capture wrote.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub links: usize,
    pub bytes: u64,
    pub redacted: usize,
    /// Unreadable, write-only or oversized files left out.
    pub skipped: usize,
}

/// Copies what the readers need from `from` into `to`, which must not exist
/// or be empty.
pub fn capture(from: &Root, to: &Path) -> io::Result<Summary> {
    if to.exists() && fs::read_dir(to)?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not empty", to.display()),
        ));
    }
    fs::create_dir_all(to)?;
    let mut c = Copier {
        src: fs::canonicalize(from.base())?,
        dst: to.to_owned(),
        seen: HashSet::new(),
        summary: Summary::default(),
    };

    // USB: every device and interface; root and hub ports with their
    // physical_location, under the interfaces.
    for dev in c.list("sys/bus/usb/devices", |_| true) {
        c.dir(&dev, &|path: &[&str]| match path {
            [port] => port.contains("-port"),
            [_, "physical_location"] => true,
            _ => false,
        });
    }
    // USB-C, PD and Thunderbolt: whole trees, minus runtime-PM and NVM noise.
    for class in [
        "sys/class/typec",
        "sys/class/usb_power_delivery",
        "sys/bus/thunderbolt/devices",
    ] {
        for dev in c.list(class, |_| true) {
            c.dir(&dev, &|path: &[&str]| {
                path.last()
                    .is_some_and(|d| !d.starts_with("nvm") && !d.starts_with("wakeup"))
            });
        }
    }
    // A typec port's ACPI node names its UCSI connector (`…UBTC.CR01`).
    for port in c.list("sys/class/typec", |n| {
        n.strip_prefix("port")
            .is_some_and(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()))
    }) {
        if let Some(real) = c.resolve(&port.join("firmware_node/path")) {
            c.file(&real);
        }
    }
    for dev in c.list("sys/class/power_supply", |_| true) {
        c.dir(&dev, &|_: &[&str]| false);
    }
    for dev in c.list("sys/class/drm", |n| {
        n.starts_with("card") && n.contains('-')
    }) {
        c.files(&dev, &["status", "enabled"]);
    }
    for dev in c.list("sys/class/hwmon", |_| true) {
        c.files(&dev, &["name", "temp*_label", "temp*_input"]);
    }
    for dev in c.list("sys/class/powercap", |n| n.starts_with("intel-rapl:")) {
        c.files(&dev, &["name", "energy_uj", "max_energy_range_uj"]);
    }
    for cpu in c.list("sys/devices/system/cpu", |n| {
        n.strip_prefix("cpu")
            .is_some_and(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()))
    }) {
        for f in ["scaling_cur_freq", "cpuinfo_min_freq", "cpuinfo_max_freq"] {
            // `cpufreq` is a link to `../cpufreq/policyN`.
            let rel = cpu.join("cpufreq").join(f);
            if let Some(real) = c.resolve(&rel) {
                c.file(&real);
            }
        }
    }
    if let Some(real) = c.resolve(Path::new("sys/class/dmi/id")) {
        c.files(&real, DMI);
    }
    if let Some(real) = c.resolve(Path::new("proc/stat")) {
        c.file(&real);
    }

    fs::write(to.join("CAPTURE.txt"), about(from, &c.summary))?;
    Ok(c.summary)
}

/// DMI fields that identify the model and firmware, not the unit.
const DMI: &[&str] = &[
    "sys_vendor",
    "product_name",
    "product_family",
    "product_version",
    "product_sku",
    "board_vendor",
    "board_name",
    "bios_vendor",
    "bios_version",
    "bios_date",
    "bios_release",
    "ec_firmware_release",
];

/// Write-only triggers and event files: never copied.
const SKIP_FILES: &[&str] = &[
    "uevent",
    "remove",
    "bind",
    "unbind",
    "new_id",
    "remove_id",
    "rescan",
];

/// Symlinks recreated inside copied directories (all others are left out).
const KEEP_LINKS: &[&str] = &["peer", "driver", "usb_power_delivery"];

/// Never descended into.
const SKIP_DIRS: &[&str] = &["power", "subsystem", "firmware_node", "device"];

/// Per-unit identifiers: serial numbers, Thunderbolt and DMI UUIDs, and the
/// battery's Dell electronic PPID (`eppid`).
fn redact(name: &str) -> bool {
    name.contains("serial") || matches!(name, "unique_id" | "product_uuid" | "eppid")
}

struct Copier {
    /// The source root, canonical.
    src: PathBuf,
    dst: PathBuf,
    /// Real directories (relative) already copied.
    seen: HashSet<PathBuf>,
    summary: Summary,
}

type Descend<'a> = &'a dyn Fn(&[&str]) -> bool;

impl Copier {
    /// The entries of `rel` whose names pass `keep`, each resolved (links
    /// recreated) to its real relative path.
    fn list(&mut self, rel: &str, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
        let Some(real) = self.resolve(Path::new(rel)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = fs::read_dir(self.src.join(&real))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| keep(n))
            .collect();
        names.sort();
        names
            .iter()
            .filter_map(|n| self.resolve(&Path::new(rel).join(n)))
            .collect()
    }

    /// Walks `rel` from the root, creating each directory and recreating each
    /// symlink on the way in the capture; returns the real relative path it
    /// leads to, or `None` when it doesn't exist or leaves the root.
    fn resolve(&mut self, rel: &Path) -> Option<PathBuf> {
        let mut cur = PathBuf::new();
        for part in rel.components() {
            let Component::Normal(part) = part else {
                continue;
            };
            let next = cur.join(part);
            let live = self.src.join(&next);
            let meta = fs::symlink_metadata(&live).ok()?;
            if meta.file_type().is_symlink() {
                self.link(&next, &live);
                let real = fs::canonicalize(&live).ok()?;
                cur = real.strip_prefix(&self.src).ok()?.to_owned();
                if real.is_dir() {
                    fs::create_dir_all(self.dst.join(&cur)).ok()?;
                } else if let Some(parent) = cur.parent() {
                    fs::create_dir_all(self.dst.join(parent)).ok()?;
                }
            } else {
                if meta.is_dir() {
                    fs::create_dir_all(self.dst.join(&next)).ok()?;
                }
                cur = next;
            }
        }
        Some(cur)
    }

    /// Recreates the symlink at `live` as `rel` in the capture, with a target
    /// relative to its directory.
    fn link(&mut self, rel: &Path, live: &Path) {
        let at = self.dst.join(rel);
        if fs::symlink_metadata(&at).is_ok() {
            return;
        }
        let Ok(mut target) = fs::read_link(live) else {
            return;
        };
        if target.is_absolute() {
            let Ok(inside) = target.strip_prefix(&self.src).map(Path::to_owned) else {
                return;
            };
            target = relative(rel.parent().unwrap_or(Path::new("")), &inside);
        }
        if let Some(parent) = at.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if symlink(&target, &at).is_ok() {
            self.summary.links += 1;
        }
    }

    /// Copies the real directory `real`: its readable files, the symlinks in
    /// [`KEEP_LINKS`], and the subdirectories `descend` accepts, given their
    /// path below `real`.
    fn dir(&mut self, real: &Path, descend: Descend) {
        self.dir_at(real, &mut Vec::new(), descend);
    }

    fn dir_at(&mut self, real: &Path, below: &mut Vec<String>, descend: Descend) {
        let here = below.iter().fold(real.to_owned(), |p, d| p.join(d));
        if !self.seen.insert(here.clone()) {
            return;
        }
        let _ = fs::create_dir_all(self.dst.join(&here));
        let mut entries: Vec<(String, fs::FileType)> = fs::read_dir(self.src.join(&here))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                Some((
                    e.file_name().to_string_lossy().into_owned(),
                    e.file_type().ok()?,
                ))
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, kind) in entries {
            let rel = here.join(&name);
            if kind.is_symlink() {
                if KEEP_LINKS.contains(&name.as_str()) {
                    self.link(&rel, &self.src.join(&rel));
                }
            } else if kind.is_dir() {
                below.push(name);
                let names: Vec<&str> = below.iter().map(String::as_str).collect();
                let go = !SKIP_DIRS.contains(&names[names.len() - 1]) && descend(&names);
                if go {
                    self.dir_at(real, below, descend);
                }
                below.pop();
            } else if kind.is_file() {
                self.file(&rel);
            }
        }
    }

    /// Copies the files of the real directory `real` whose names match one of
    /// `patterns` (`*` matches any run of characters).
    fn files(&mut self, real: &Path, patterns: &[&str]) {
        let _ = fs::create_dir_all(self.dst.join(real));
        let mut names: Vec<String> = fs::read_dir(self.src.join(real))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| patterns.iter().any(|p| glob(p, n)))
            .collect();
        names.sort();
        for name in names {
            self.file(&real.join(name));
        }
    }

    /// Copies one regular file (real relative path) when readable and small,
    /// redacting identifiers.
    fn file(&mut self, rel: &Path) {
        let name = rel
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if SKIP_FILES.contains(&name.as_str()) {
            return;
        }
        let live = self.src.join(rel);
        let Some(bytes) = read_small(&live) else {
            self.summary.skipped += 1;
            return;
        };
        let bytes = if redact(&name) {
            self.summary.redacted += 1;
            b"REDACTED\n".to_vec()
        } else {
            bytes
        };
        let at = self.dst.join(rel);
        if let Some(parent) = at.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(&at, &bytes).is_ok() {
            self.summary.files += 1;
            self.summary.bytes += bytes.len() as u64;
        }
    }
}

/// The file's contents when it is readable by someone and at most
/// [`MAX_FILE`] bytes. sysfs sizes are nominal (4096, or 65535 for binary
/// attributes), so the limit applies to what a read returns.
fn read_small(path: &Path) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.permissions().mode() & 0o444 == 0 {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_FILE).then_some(bytes)
}

/// `to` relative to the directory `from` (both relative to the same root).
fn relative(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let common = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for part in &to_parts[common..] {
        out.push(part);
    }
    out
}

/// Shell-style match with `*` as the only wildcard.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            let Some(rest) = name.strip_prefix(head) else {
                return false;
            };
            (0..=rest.len())
                .filter(|&i| rest.is_char_boundary(i))
                .any(|i| glob(tail, &rest[i..]))
        }
    }
}

/// The `CAPTURE.txt` contents.
fn about(from: &Root, summary: &Summary) -> String {
    let dmi = |f: &str| read(from.path(format!("sys/class/dmi/id/{f}"))).unwrap_or_default();
    let mut out = String::new();
    let _ = writeln!(out, "plugii capture");
    let _ = writeln!(out, "date: {}", utc_date(SystemTime::now()));
    let _ = writeln!(
        out,
        "kernel: {}",
        read(from.path("proc/sys/kernel/osrelease")).unwrap_or_else(|| "unknown".into())
    );
    let _ = writeln!(
        out,
        "machine: {} {}",
        dmi("sys_vendor"),
        dmi("product_name")
    );
    let _ = writeln!(out, "plugii: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "files: {}, symlinks: {}, bytes: {}; {} redacted (serials, unique IDs), {} skipped",
        summary.files, summary.links, summary.bytes, summary.redacted, summary.skipped
    );
    out
}

/// `YYYY-MM-DD` in UTC.
fn utc_date(t: SystemTime) -> String {
    let days = t
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn globs() {
        assert!(glob("temp*_label", "temp1_label"));
        assert!(glob("temp*_label", "temp12_label"));
        assert!(!glob("temp*_label", "temp1_input"));
        assert!(glob("name", "name"));
        assert!(!glob("name", "names"));
        assert!(glob("*", "anything"));
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative(
                Path::new("sys/class/typec"),
                Path::new("sys/devices/platform/x/port0")
            ),
            PathBuf::from("../../devices/platform/x/port0")
        );
        assert_eq!(
            relative(Path::new("a/b"), Path::new("a/b/c")),
            PathBuf::from("c")
        );
    }

    #[test]
    fn dates() {
        assert_eq!(utc_date(UNIX_EPOCH), "1970-01-01");
        let t = UNIX_EPOCH + Duration::from_secs(1_791_100_000);
        assert_eq!(utc_date(t), "2026-10-04");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(utc_date(leap), "2000-02-29");
    }

    #[test]
    fn redacted_names() {
        for name in [
            "serial",
            "serial_number",
            "product_serial",
            "board_serial",
            "unique_id",
            "product_uuid",
            "eppid",
        ] {
            assert!(redact(name), "{name}");
        }
        assert!(!redact("product"));
        assert!(!redact("idVendor"));
    }
}
