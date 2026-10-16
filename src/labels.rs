//! The labels file: names for physical sockets, which the kernel can't know.
//!
//! Same format as the `usb-ports` script, one `<key> <value>` per line:
//!
//! ```text
//! <pci-address>-<port>  <label>      0000:06:00.4-1  USB-C (caps lock)
//! typec:<portN>         <label>      typec:port0  USB-C (caps lock)
//! esrt:<guid>           <label>      esrt:a77b326c-…  USB-C PD firmware
//! peer:<usb2-key>       <usb3-key>   peer:0000:00:14.0-1  0000:00:0d.0-1
//! device:<vid>:<pid>    <name>       device:05e3:0610  USB switch (desk)
//! ```
//!
//! Lines are split like bash `read -r key text`: the key is the first
//! whitespace-delimited token, the value the rest of the line, trimmed.
//! Blank lines and lines whose key starts with `#` are skipped, and the last
//! line for a key wins. A `device:` key takes 4 hex digits each for the
//! vendor and product id, in either case; a malformed one is skipped.
//!
//! [`edit`] changes the text of a labels file one key at a time, keeping
//! comments and the order of every other line; [`write`] applies it to the
//! file atomically.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, Write as _};
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
    /// `vid:pid` (lowercase hex: `05e3:0610`) → the name of every device
    /// with these ids.
    pub devices: BTreeMap<String, String>,
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
            let id;
            let (map, key, value) = if let Some(k) = key.strip_prefix("peer:") {
                let usb3 = value.split(is_ifs).next().unwrap_or("");
                (&mut labels.peers, k, usb3)
            } else if let Some(k) = key.strip_prefix("typec:") {
                (&mut labels.typec, k, value)
            } else if let Some(k) = key.strip_prefix("esrt:") {
                (&mut labels.esrt, k, value)
            } else if let Some(k) = key.strip_prefix("device:") {
                let Some((vid, pid)) = parse_ids(k) else {
                    continue;
                };
                id = ids_text(vid, pid);
                (&mut labels.devices, id.as_str(), value)
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

    /// The name given to devices with this vendor and product id.
    pub fn device_label(&self, vendor_id: u16, product_id: u16) -> Option<&str> {
        self.devices
            .get(&ids_text(vendor_id, product_id))
            .map(String::as_str)
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
        if !self.devices.is_empty() {
            out.push_str("devices:\n");
            for (id, label) in &self.devices {
                let _ = writeln!(out, "  {id}  {label}");
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

/// `05e3:0610` (either case) → (0x05e3, 0x0610); exactly 4 hex digits each.
fn parse_ids(text: &str) -> Option<(u16, u16)> {
    let (vid, pid) = text.split_once(':')?;
    let hex = |s: &str| {
        if s.len() == 4 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            u16::from_str_radix(s, 16).ok()
        } else {
            None
        }
    };
    Some((hex(vid)?, hex(pid)?))
}

fn ids_text(vendor_id: u16, product_id: u16) -> String {
    format!("{vendor_id:04x}:{product_id:04x}")
}

/// One key of the labels file, as the label wizard writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// `<pci-address>-<port>`: a socket.
    Socket(String),
    /// `typec:<portN>`: a UCSI port.
    Typec(String),
    /// `device:<vid>:<pid>`: every device with these ids.
    Device { vendor_id: u16, product_id: u16 },
}

impl Key {
    /// Whether a line whose key (first token) is `token` sets this key.
    /// Device ids match in either case.
    fn matches(&self, token: &str) -> bool {
        match self {
            Self::Socket(k) => token == k,
            Self::Typec(port) => token.strip_prefix("typec:") == Some(port.as_str()),
            Self::Device {
                vendor_id,
                product_id,
            } => token
                .strip_prefix("device:")
                .and_then(parse_ids)
                .is_some_and(|ids| ids == (*vendor_id, *product_id)),
        }
    }
}

/// As the file spells it: `0000:06:00.3-1`, `typec:port0`, `device:05e3:0610`.
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Socket(k) => f.write_str(k),
            Self::Typec(port) => write!(f, "typec:{port}"),
            Self::Device {
                vendor_id,
                product_id,
            } => write!(f, "device:{}", ids_text(*vendor_id, *product_id)),
        }
    }
}

/// The labels file `text` with `key` set to `value`.
///
/// The value is trimmed, and line breaks in it become spaces. When the key
/// has lines already, the last one (the one that wins) is replaced in place;
/// otherwise `<key>  <value>` is appended. An empty value removes every line
/// for the key: that unlabels it. Comments, blank lines, line endings and
/// the order of everything else are kept.
pub fn edit(text: &str, key: &Key, value: &str) -> String {
    let value = value.replace(['\r', '\n'], " ");
    let value = value.trim_matches(is_ifs);
    // Each line with its own ending (`\n`, `\r\n`, or none at the very end).
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    let is_key = |line: &str| {
        let line = line.trim_end_matches(['\r', '\n']).trim_matches(is_ifs);
        key.matches(line.split(is_ifs).next().unwrap_or(""))
    };
    if value.is_empty() {
        lines.retain(|l| !is_key(l));
        return lines.concat();
    }
    let new_line = format!("{key}  {value}");
    let mut out = String::with_capacity(text.len() + new_line.len() + 1);
    match lines.iter().rposition(|l| is_key(l)) {
        Some(at) => {
            for (i, line) in lines.iter().enumerate() {
                if i == at {
                    let body = line.trim_end_matches(['\r', '\n']);
                    out.push_str(&new_line);
                    out.push_str(&line[body.len()..]);
                } else {
                    out.push_str(line);
                }
            }
        }
        None => {
            out.push_str(text);
            if !text.is_empty() && !text.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&new_line);
            out.push('\n');
        }
    }
    out
}

/// [`edit`] for each `(key, value)` in turn, on the labels file at `path`;
/// then the labels read back from it.
///
/// A missing file (and its directory) is created. The new text goes to a
/// temporary file in the same directory, renamed over the old one, so a
/// crash leaves the old file or the new one, never half of each; the file's
/// permissions are kept. When `path` is a symlink, its target is written
/// and the link left alone.
pub fn write(path: &Path, edits: &[(Key, String)]) -> io::Result<Labels> {
    let target = resolve_links(path)?;
    let old = match fs::read_to_string(&target) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let mut text = old.clone().unwrap_or_default();
    for (key, value) in edits {
        text = edit(&text, key, value);
    }
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)?;
    let name = target
        .file_name()
        .map_or_else(|| "labels".into(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        if old.is_some() {
            file.set_permissions(fs::metadata(&target)?.permissions())?;
        }
        file.sync_all()?;
        fs::rename(&tmp, &target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written?;
    read(&target)
}

/// `path` with the symlinks at its end followed, even to a target that
/// doesn't exist yet.
fn resolve_links(path: &Path) -> io::Result<PathBuf> {
    let mut at = path.to_path_buf();
    // As many hops as Linux follows before ELOOP.
    for _ in 0..40 {
        match fs::read_link(&at) {
            // A relative target is relative to the link's directory.
            Ok(target) => at = at.parent().map_or(target.clone(), |dir| dir.join(&target)),
            // Not a link, or nothing there yet: write here.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::NotFound
                ) =>
            {
                return Ok(at);
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other(format!(
        "too many levels of symlinks at {}",
        path.display()
    )))
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

/// `path` for messages, with `~` for the home directory `home`.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    match home
        .filter(|h| !h.as_os_str().is_empty())
        .and_then(|h| path.strip_prefix(h).ok())
    {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
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
