//! The labels file: names for physical sockets, which the kernel can't know.
//!
//! Same format as the `usb-ports` script, one `<key> <value>` per line:
//!
//! ```text
//! <pci-address>-<port>  <label>      0000:06:00.4-1  USB-C (caps lock)
//! typec:<portN>         <label>      typec:port0  USB-C (caps lock)
//! esrt:<guid>           <label>      esrt:a77b326c-…  USB-C PD firmware
//! peer:<usb2-key>       <usb3-key>   peer:0000:00:14.0-1  0000:00:0d.0-1
//! ```
//!
//! Lines are split like bash `read -r key text`: the key is the first
//! whitespace-delimited token, the value the rest of the line, trimmed.
//! Blank lines and lines whose key starts with `#` are skipped, and the last
//! line for a key wins.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Parsed labels, keyed without their `peer:` / `typec:` / `esrt:` prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels {
    /// Socket key (`<pci-address>-<port>` of its USB 2 side) → label.
    pub sockets: BTreeMap<String, String>,
    /// USB 2 socket key → USB 3 key, for ports without a kernel `peer` link.
    pub peers: BTreeMap<String, String>,
    /// UCSI port name (`port0`) → label.
    pub typec: BTreeMap<String, String>,
    /// ESRT firmware GUID → label.
    pub esrt: BTreeMap<String, String>,
}

impl Labels {
    pub fn parse(text: &str) -> Self {
        let mut labels = Self::default();
        for line in text.lines() {
            let line = line.trim_matches(is_ifs);
            let (key, value) = match line.split_once(is_ifs) {
                Some((key, rest)) => (key, rest.trim_matches(is_ifs)),
                None => (line, ""),
            };
            if key.is_empty() || key.starts_with('#') {
                continue;
            }
            let (map, key, value) = if let Some(k) = key.strip_prefix("peer:") {
                let usb3 = value.split(is_ifs).next().unwrap_or("");
                (&mut labels.peers, k, usb3)
            } else if let Some(k) = key.strip_prefix("typec:") {
                (&mut labels.typec, k, value)
            } else if let Some(k) = key.strip_prefix("esrt:") {
                (&mut labels.esrt, k, value)
            } else {
                (&mut labels.sockets, key, value)
            };
            // The script stores an empty value too, then reads it as unset.
            if value.is_empty() {
                map.remove(key);
            } else {
                map.insert(key.to_owned(), value.to_owned());
            }
        }
        labels
    }

    pub fn socket_label(&self, key: &str) -> Option<&str> {
        self.sockets.get(key).map(String::as_str)
    }

    /// The USB 3 key paired by hand with this USB 2 socket key.
    pub fn peer_of(&self, usb2_key: &str) -> Option<&str> {
        self.peers.get(usb2_key).map(String::as_str)
    }

    pub fn typec_label(&self, port: &str) -> Option<&str> {
        self.typec.get(port).map(String::as_str)
    }

    /// The UCSI port carrying the same label as this socket. When several do,
    /// the first by name wins (the script's pick is hash order).
    pub fn typec_port_for_socket(&self, key: &str) -> Option<&str> {
        let label = self.socket_label(key)?;
        self.typec
            .iter()
            .find(|(_, l)| *l == label)
            .map(|(port, _)| port.as_str())
    }

    pub fn esrt_label(&self, guid: &str) -> Option<&str> {
        self.esrt.get(guid).map(String::as_str)
    }

    /// A human-readable listing, one section per kind; empty sections are left
    /// out, and UCSI ports and peers already shown on a socket aren't repeated.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        if *self == Self::default() {
            out.push_str("no labels\n");
            return out;
        }
        if !self.sockets.is_empty() {
            out.push_str("sockets:\n");
            for (key, label) in &self.sockets {
                let mut extra = Vec::new();
                if let Some(peer) = self.peer_of(key) {
                    extra.push(format!("USB 3: {peer}"));
                }
                if let Some(port) = self.typec_port_for_socket(key) {
                    extra.push(format!("USB-C: {port}"));
                }
                let _ = write!(out, "  {key}  {label}");
                if !extra.is_empty() {
                    let _ = write!(out, "  ({})", extra.join(", "));
                }
                out.push('\n');
            }
        }
        let peers: Vec<_> = self
            .peers
            .iter()
            .filter(|(k, _)| !self.sockets.contains_key(*k))
            .collect();
        if !peers.is_empty() {
            out.push_str("unlabeled peers:\n");
            for (usb2, usb3) in peers {
                let _ = writeln!(out, "  {usb2} -> {usb3}");
            }
        }
        let typec: Vec<_> = self
            .typec
            .iter()
            .filter(|(_, l)| !self.sockets.values().any(|s| s == *l))
            .collect();
        if !typec.is_empty() {
            out.push_str("USB-C ports:\n");
            for (port, label) in typec {
                let _ = writeln!(out, "  {port}  {label}");
            }
        }
        if !self.esrt.is_empty() {
            out.push_str("firmware (ESRT):\n");
            for (guid, label) in &self.esrt {
                let _ = writeln!(out, "  {guid}  {label}");
            }
        }
        out
    }
}

/// bash's default IFS whitespace (newlines are already split off).
fn is_ifs(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// `$XDG_CONFIG_HOME`, else `~/.config`; `None` without a usable `$HOME`.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}

/// [`config_dir`] with the environment passed in. An empty or relative
/// `XDG_CONFIG_HOME` is ignored, as the XDG base directory spec says.
pub fn config_dir_from(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    if let Some(dir) = xdg_config_home.map(PathBuf::from)
        && dir.is_absolute()
    {
        return Some(dir);
    }
    home.filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".config"))
}

/// Where the labels file lives under `config_dir`: `plugii/labels`.
pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join("plugii/labels")
}

/// The labels file under `config_dir`; `None` when it doesn't exist.
pub fn find(config_dir: &Path) -> Option<PathBuf> {
    Some(path(config_dir)).filter(|path| path.is_file())
}

/// Labels from [`find`]'s file; no file means no labels, not an error.
pub fn load(config_dir: &Path) -> io::Result<Labels> {
    match find(config_dir) {
        Some(path) => read(&path),
        None => Ok(Labels::default()),
    }
}

pub fn read(path: &Path) -> io::Result<Labels> {
    fs::read_to_string(path).map(|text| Labels::parse(&text))
}
