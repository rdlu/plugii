//! Kernel log errors and USB link resets for this boot.
//!
//! Input is `journalctl -k -b -q [-p warning] --no-pager -o short-monotonic`
//! text, so parsing is pure and testable; [`query`] runs the two commands.

use std::collections::BTreeMap;
use std::time::Duration;

use super::command;
use crate::model::{KernelLog, LinkResets, LogLine};

/// Real USB / USB-C / link-training failures, matched case-insensitively.
/// Each pattern is a sequence of fragments that must appear in order (the
/// script's `a.*b`).
const ERROR_PATTERNS: &[&[&str]] = &[
    &["ucsi", "fail"],
    &["ucsi", "error"],
    &["over-current"],
    &["device descriptor read"],
    &["not accepting address"],
    &["unable to enumerate"],
    &["cannot enable"],
    &["link training", "fail"],
    &["hc died"],
    &["xhci", "error"],
];

/// A warning a driver prints on healthy machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenignPattern {
    /// The driver that prints it; for reading the table, not matched.
    pub driver: &'static str,
    /// Matched case-sensitively anywhere in the line.
    pub needle: &'static str,
}

/// Known-harmless warnings, by driver. Add new ones here.
pub const BENIGN_PATTERNS: &[BenignPattern] = &[
    BenignPattern {
        driver: "amdgpu",
        needle: "LTTPR",
    },
    BenignPattern {
        driver: "amdgpu",
        needle: "REG_WAIT",
    },
];

/// Builds the log summary. `warnings` is the `-p warning` output, `all` the
/// full kernel log (link resets are info-level). `socket_of` names the socket
/// a message's device is on, if any; see [`usb_location`].
///
/// Every error line is kept (the report shows the last few); lines without a
/// parsable timestamp keep their whole text and no socket.
pub fn parse(warnings: &str, all: &str, socket_of: impl Fn(&str) -> Option<String>) -> KernelLog {
    let errors = warnings
        .lines()
        .filter(|line| is_error(line))
        .map(|line| match split_line(line) {
            Some((secs, message)) => LogLine {
                secs_since_boot: Some(secs),
                message: message.to_owned(),
                socket: socket_of(message),
            },
            None => LogLine {
                secs_since_boot: None,
                message: line.to_owned(),
                socket: None,
            },
        })
        .collect();

    let benign_count = warnings
        .lines()
        .filter(|line| BENIGN_PATTERNS.iter().any(|p| line.contains(p.needle)))
        .count();

    let mut resets: BTreeMap<String, u32> = BTreeMap::new();
    for line in all.lines() {
        if !(line.contains("reset SuperSpeed") || line.contains("reset high-speed")) {
            continue;
        }
        let Some((_, message)) = split_line(line) else {
            continue;
        };
        let place = socket_of(message)
            .or_else(|| usb_device_port(message).map(|(bus, port)| format!("port {bus}-{port}")))
            .unwrap_or_else(|| "unknown port".to_owned());
        *resets.entry(place).or_default() += 1;
    }

    KernelLog {
        errors,
        benign_count: u32::try_from(benign_count).unwrap_or(u32::MAX),
        link_resets: resets
            .into_iter()
            .map(|(place, count)| LinkResets { place, count })
            .collect(),
    }
}

fn is_error(line: &str) -> bool {
    let line = line.to_lowercase();
    ERROR_PATTERNS.iter().any(|fragments| {
        let mut rest = line.as_str();
        fragments.iter().all(|fragment| match rest.find(fragment) {
            Some(i) => {
                rest = &rest[i + fragment.len()..];
                true
            }
            None => false,
        })
    })
}

/// `[  123.456789] host kernel: message` → (123, `message`).
fn split_line(line: &str) -> Option<(u64, &str)> {
    let rest = line.strip_prefix('[')?.trim_start_matches(' ');
    let (secs, rest) = leading_digits(rest)?;
    let rest = rest.strip_prefix('.')?;
    let (_, rest) = leading_digits(rest)?;
    let rest = rest.strip_prefix("] ")?;
    let (host, message) = rest.split_once(" kernel: ")?;
    (!host.is_empty() && !host.contains(' ')).then_some((secs, message))
}

/// A run of ASCII digits at the start of `s`, parsed, and what follows.
fn leading_digits(s: &str) -> Option<(u64, &str)> {
    let end = s
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(s.len());
    Some((s[..end].parse().ok()?, &s[end..]))
}

fn small(n: u64) -> Option<u32> {
    u32::try_from(n).ok()
}

/// The root port (bus, port) a kernel message is about: a device
/// (`usb 2-3.1: …`, `usb 2-3: …`) or a root hub port (`usb usb2-port3: …`).
/// The device form is looked for first, as in the script.
pub fn usb_location(msg: &str) -> Option<(u32, u32)> {
    let device = msg.match_indices("usb ").find_map(|(i, _)| {
        let (bus, rest) = leading_digits(&msg[i + 4..])?;
        let (port, rest) = leading_digits(rest.strip_prefix('-')?)?;
        matches!(rest.chars().next(), Some('.' | ':')).then_some((small(bus)?, small(port)?))
    });
    device.or_else(|| {
        msg.match_indices("usb usb").find_map(|(i, _)| {
            let (bus, rest) = leading_digits(&msg[i + 7..])?;
            let (port, _) = leading_digits(rest.strip_prefix("-port")?)?;
            Some((small(bus)?, small(port)?))
        })
    })
}

/// The script's fallback place for a reset: the first `usb <bus>-<port>`.
fn usb_device_port(msg: &str) -> Option<(u32, u32)> {
    msg.match_indices("usb ").find_map(|(i, _)| {
        let (bus, rest) = leading_digits(&msg[i + 4..])?;
        let (port, _) = leading_digits(rest.strip_prefix('-')?)?;
        Some((small(bus)?, small(port)?))
    })
}

/// This boot's kernel log: (warnings and worse, everything). Either is empty
/// when journalctl is missing, fails or takes over 10 s.
pub fn query() -> (String, String) {
    let run = |priority: &[&str]| {
        let mut args = vec!["-k", "-b", "-q"];
        args.extend_from_slice(priority);
        args.extend_from_slice(&["--no-pager", "-o", "short-monotonic"]);
        command::output("journalctl", &args, Duration::from_secs(10)).unwrap_or_default()
    };
    (run(&["-p", "warning"]), run(&[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_monotonic_lines() {
        assert_eq!(
            split_line("[    1.372112] cachyos kernel: usb usb1: Product: xHCI"),
            Some((1, "usb usb1: Product: xHCI"))
        );
        assert_eq!(
            split_line("[12676.674026] xps kernel:   #1  #3"),
            Some((12676, "  #1  #3"))
        );
        assert_eq!(split_line("kernel: usb 3-1: oops"), None);
        assert_eq!(split_line("[ 1.5] kernel: no host"), None);
        assert_eq!(split_line("[x.5] h kernel: m"), None);
    }

    #[test]
    fn error_patterns_need_order() {
        assert!(is_error("ucsi_acpi USBC000:00: PPM init FAILED"));
        assert!(!is_error("failed to load ucsi firmware"));
        assert!(is_error("xhci_hcd 0000:00:14.0: ERROR unknown event"));
        assert!(!is_error("usb 2-1: new SuperSpeed USB device"));
    }

    #[test]
    fn usb_locations() {
        assert_eq!(usb_location("usb 2-3.1: reset SuperSpeed"), Some((2, 3)));
        assert_eq!(usb_location("usb 2-3: new device"), Some((2, 3)));
        assert_eq!(usb_location("usb usb2-port3: over-current"), Some((2, 3)));
        assert_eq!(
            usb_location("hub 2-0:1.0: usb usb12-port10: Cannot enable"),
            Some((12, 10))
        );
        // `usb 2-3` with neither `.` nor `:` after it isn't a device name.
        assert_eq!(usb_location("usb 2-3 went away"), None);
        assert_eq!(usb_location("usb usb1: Product: xHCI"), None);
        assert_eq!(usb_location("r8152 4-2.1:1.0 eth0: carrier on"), None);
    }

    #[test]
    fn reset_fallback_place() {
        assert_eq!(usb_device_port("usb 4-2 x"), Some((4, 2)));
        assert_eq!(usb_device_port("xhci_hcd 0000:06:00.4: port reset"), None);
    }

    #[test]
    fn real_log_lines_all_parse() {
        let all = include_str!("../../tests/fixtures/kernel_log/xps/all.log");
        let warnings = include_str!("../../tests/fixtures/kernel_log/xps/warnings.log");
        for line in all.lines().chain(warnings.lines()) {
            assert!(split_line(line).is_some(), "{line}");
        }
    }
}
