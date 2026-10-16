//! Vendor modules, picked by DMI `sys_vendor`: firmware catalogs for
//! `--firmware` and vendor-specific advice added to generic findings.
//!
//! All network access goes through [`Fetch`], so tests hand in canned
//! responses; parsed catalog results are cached for a day in [`Cache`].

pub mod dell;
pub mod lenovo;

use std::cmp::Ordering;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::model::{Catalog, Dmi, Finding};
use crate::sysfs::{self, Root};

/// A vendor with a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Lenovo,
    Dell,
}

impl Vendor {
    /// DMI `sys_vendor` (`LENOVO`, `Dell Inc.`) → its module.
    pub fn from_sys_vendor(sys_vendor: &str) -> Option<Self> {
        match sys_vendor.trim() {
            "LENOVO" => Some(Self::Lenovo),
            "Dell Inc." => Some(Self::Dell),
            _ => None,
        }
    }

    /// The running machine's (or fixture's) vendor, from DMI.
    pub fn detect(root: &Root) -> Option<Self> {
        Self::from_sys_vendor(&sysfs::read(root.path("sys/class/dmi/id/sys_vendor"))?)
    }
}

/// Reads `sys/class/dmi/id/*` under `root`.
pub fn read_dmi(root: &Root) -> Dmi {
    let read = |f: &str| sysfs::read(root.path(format!("sys/class/dmi/id/{f}")));
    Dmi {
        sys_vendor: read("sys_vendor"),
        product_name: read("product_name"),
        product_sku: read("product_sku"),
        product_family: read("product_family"),
        bios_version: read("bios_version"),
        bios_release: read("bios_release"),
        bios_date: read("bios_date"),
        ec_firmware_release: read("ec_firmware_release"),
    }
}

/// The vendor catalog's answer for this machine.
pub fn catalog(dmi: &Dmi, fetch: &dyn Fetch, cache: &Cache) -> Catalog {
    match dmi.sys_vendor.as_deref().and_then(Vendor::from_sys_vendor) {
        Some(Vendor::Lenovo) => lenovo::catalog(dmi, fetch, cache),
        Some(Vendor::Dell) => dell::catalog(dmi, fetch, cache),
        None => Catalog::NotChecked,
    }
}

/// Why a catalog fetch came back without an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gap {
    /// Offline, or the server failed: not an error, the report says so.
    Unreachable,
    /// Fetched but not readable: the cab or XML is broken.
    Unreadable(String),
    /// Read, but it has nothing for this model.
    NotListed,
}

impl Gap {
    /// The [`Catalog`] the report shows for it.
    pub fn catalog(self, vendor: &str, model: &str) -> Catalog {
        let (vendor, model) = (vendor.to_owned(), model.to_owned());
        match self {
            Self::Unreachable => Catalog::Unreachable { vendor, model },
            Self::Unreadable(error) => Catalog::Unreadable {
                vendor,
                model,
                error,
            },
            Self::NotListed => Catalog::NotListed { vendor, model },
        }
    }
}

/// Adds the vendor's fix advice to generic findings: the Lenovo EC reset
/// procedure after "EC reset", for one.
pub fn advise(vendor: Option<Vendor>, findings: &mut [Finding]) {
    if vendor == Some(Vendor::Lenovo) {
        lenovo::advise(findings);
    }
}

/// Something that fetches a URL: [`Http`] on a live machine, canned
/// responses in tests.
pub trait Fetch: Sync {
    /// The body at `url`, or why it couldn't be had.
    fn get(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, String>;
}

/// The network, through ureq (blocking, rustls).
#[derive(Debug, Clone, Copy, Default)]
pub struct Http;

/// The largest body read: Dell's index cab is about 0.5 MB.
const MAX_BODY: u64 = 64 << 20;

impl Fetch for Http {
    fn get(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        let mut response = agent
            .get(url)
            .header("User-Agent", concat!("plugii/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|e| e.to_string())?;
        response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }
}

/// How long a cached catalog result is good for, as in the script.
pub const CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Parsed catalog results as JSON files, one per vendor and model, fresh for
/// [`CACHE_MAX_AGE`] by modification time.
#[derive(Debug, Clone)]
pub struct Cache {
    /// `None`: no cache directory, so nothing is cached.
    dir: Option<PathBuf>,
    /// Ignore what's cached (`--refresh`); results are still stored.
    refresh: bool,
    max_age: Duration,
}

impl Cache {
    pub fn new(dir: Option<PathBuf>, refresh: bool) -> Self {
        Self {
            dir,
            refresh,
            max_age: CACHE_MAX_AGE,
        }
    }

    /// No cache at all.
    pub fn none() -> Self {
        Self::new(None, false)
    }

    /// `$XDG_CACHE_HOME/plugii`, else `~/.cache/plugii`.
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|d| !d.is_empty())
                    .map(|h| PathBuf::from(h).join(".cache"))
            })?;
        Some(base.join("plugii"))
    }

    /// Where `name` is (or would be) cached.
    pub fn path(&self, name: &str) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("{}.json", file_safe(name))))
    }

    /// The value cached under `name`, if it is fresh and readable.
    pub fn load<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        if self.refresh {
            return None;
        }
        let path = self.path(name)?;
        let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO);
        if age >= self.max_age {
            return None;
        }
        serde_json::from_slice(&fs::read(&path).ok()?).ok()
    }

    /// Caches `value` under `name`: written to a temporary file in the same
    /// directory, then renamed over the old one, so a reader never sees half
    /// a file. Without a directory it does nothing.
    pub fn store<T: Serialize>(&self, name: &str, value: &T) -> io::Result<()> {
        let Some(path) = self.path(name) else {
            return Ok(());
        };
        let dir = path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(
            ".{}.{}.tmp",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("cache"),
            std::process::id()
        ));
        let result = (|| {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(value)?)?;
            file.sync_all()?;
            fs::rename(&tmp, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

/// `name` with anything but ASCII letters, digits, `-` and `_` replaced, for
/// a file name made from DMI strings.
fn file_safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Compares versions the way `sort -V` does for these: runs of digits as
/// numbers, everything else as text. `2.9.0 < 2.10.0`.
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (chunks(a), chunks(b));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    _ => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// `2.30.0a` → `2`, `.`, `30`, `.`, `0`, `a`.
fn chunks(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let digit = first.is_ascii_digit();
        let end = rest
            .find(|c: char| c.is_ascii_digit() != digit)
            .unwrap_or(rest.len());
        let (chunk, tail) = rest.split_at(end);
        rest = tail;
        Some(chunk)
    })
}

/// Parses vendor XML, without the byte order mark some catalogs start with.
pub(crate) fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, String> {
    roxmltree::Document::parse(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())
}

/// The trimmed text of `node`'s first child element named `name`.
pub(crate) fn child_text<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|n| n.has_tag_name(name))?
        .text()
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendors_by_sys_vendor() {
        assert_eq!(Vendor::from_sys_vendor("LENOVO"), Some(Vendor::Lenovo));
        assert_eq!(Vendor::from_sys_vendor("Dell Inc.\n"), Some(Vendor::Dell));
        assert_eq!(Vendor::from_sys_vendor("Framework"), None);
    }

    #[test]
    fn versions_compare_like_sort_v() {
        assert_eq!(version_cmp("2.30.0", "2.32.0"), Ordering::Less);
        assert_eq!(version_cmp("2.9.0", "2.10.0"), Ordering::Less);
        assert_eq!(version_cmp("2.32.0", "2.32.0"), Ordering::Equal);
        assert_eq!(version_cmp("2.32.1", "2.32.0"), Ordering::Greater);
        assert_eq!(version_cmp("2.32", "2.32.0"), Ordering::Less);
        assert_eq!(version_cmp("1.10a", "1.10b"), Ordering::Less);
    }

    #[test]
    fn cache_names_are_file_safe() {
        assert_eq!(file_safe("dell-0AF3"), "dell-0AF3");
        assert_eq!(file_safe("lenovo-../x y"), "lenovo-___x_y");
    }
}
