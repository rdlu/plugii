//! Readers that turn a sysfs tree into [`crate::model`] data.
//!
//! Every reader takes a [`Root`] — `/` on a live machine, a fixture directory
//! in tests — and never touches absolute paths itself. Missing or unreadable
//! attributes come back as `None`: sysfs differs between kernels and
//! machines, and an absent file is information, not an error.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub mod cpu;
pub mod drm;
pub mod kernel_log;
pub mod power;
pub mod thunderbolt;
pub mod typec;
pub mod usb;

/// The filesystem root sysfs paths are resolved against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root(PathBuf);

impl Root {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// The running machine's `/`.
    pub fn live() -> Self {
        Self::new("/")
    }

    /// `rel` (e.g. `sys/bus/usb/devices`) under this root. A leading `/` is
    /// ignored, so `/sys/...` and `sys/...` mean the same.
    pub fn path(&self, rel: impl AsRef<Path>) -> PathBuf {
        let rel = rel.as_ref();
        self.0.join(rel.strip_prefix("/").unwrap_or(rel))
    }
}

/// A file's contents with trailing whitespace trimmed; `None` when it can't be
/// read or is empty.
pub fn read(path: impl AsRef<Path>) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

/// [`read`] parsed as `T`; `None` when missing or unparsable.
pub fn read_parse<T: FromStr>(path: impl AsRef<Path>) -> Option<T> {
    read(path)?.trim().parse().ok()
}

/// A hex attribute such as `idVendor` (`0bda`) or `bDeviceClass` (`09`).
pub fn read_hex<T: TryFrom<u32>>(path: impl AsRef<Path>) -> Option<T> {
    let text = read(path)?;
    let n = u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()?;
    T::try_from(n).ok()
}

/// A file's raw bytes (binary attributes like `descriptors`); `None` when
/// unreadable or empty.
pub fn read_bytes(path: impl AsRef<Path>) -> Option<Vec<u8>> {
    fs::read(path).ok().filter(|b| !b.is_empty())
}

/// The final path component a symlink points at (`driver` → `hub`).
pub fn link_name(path: impl AsRef<Path>) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    Some(target.file_name()?.to_string_lossy().into_owned())
}

/// Entries of a directory whose names satisfy `keep`, sorted by name;
/// empty when the directory is missing.
pub fn entries(dir: impl AsRef<Path>, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| keep(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_path_ignores_leading_slash() {
        let root = Root::new("/tmp/fixture");
        assert_eq!(
            root.path("/sys/class"),
            PathBuf::from("/tmp/fixture/sys/class")
        );
        assert_eq!(
            root.path("sys/class"),
            PathBuf::from("/tmp/fixture/sys/class")
        );
    }

    #[test]
    fn read_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let f = |name: &str, text: &str| {
            let p = dir.path().join(name);
            fs::write(&p, text).unwrap();
            p
        };
        assert_eq!(read(f("a", "hub\n")).as_deref(), Some("hub"));
        assert_eq!(read(f("empty", "\n")), None);
        assert_eq!(read(dir.path().join("missing")), None);
        assert_eq!(read_parse::<u32>(f("n", "480\n")), Some(480));
        assert_eq!(read_parse::<u32>(f("bad", "x\n")), None);
        assert_eq!(read_hex::<u16>(f("vid", "0bda\n")), Some(0x0bda));
        assert_eq!(read_hex::<u8>(f("cls", "09\n")), Some(9));
        assert_eq!(read_hex::<u8>(f("big", "1ff\n")), None);
    }
}
