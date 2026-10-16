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
    let message = split_line(line).map_or(line, |(_, message)| message);
    if is_trace_line(message) {
        return false;
    }
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

/// Lines of a `WARNING:` / `BUG:` report after its header: module list,
/// registers, `Call Trace:` and its indented frames, `---[ end trace ]---`.
/// Function and module names in them (`xhci_handle_error`, `failover`) say
/// nothing about this boot's ports, so they never count as errors. The
/// header itself is still matched.
const TRACE_PREFIXES: &[&str] = &[
    "Modules linked in:",
    "CPU:",
    "Hardware name:",
    "Workqueue:",
    "RIP:",
    "Code:",
    "Call Trace:",
    "<TASK>",
    "</TASK>",
    "<IRQ>",
    "</IRQ>",
    "---[ end trace",
    "------------[ cut here ]",
];

fn is_trace_line(message: &str) -> bool {
    // Stack frames are indented: ` dcn20_optimize_bandwidth+0x114/0x210`.
    if message.starts_with(' ') || TRACE_PREFIXES.iter().any(|p| message.starts_with(p)) {
        return true;
    }
    // Register dumps: `RSP: 0018:...`, `R10: ...`, `CR2: ...`, `PKRU: ...`.
    // The value must be hex, so `ACPI: ...`, `DMAR: ...` and `CPU1: Core
    // temperature ...` still count.
    message.split_once(": ").is_some_and(|(reg, value)| {
        let hex = value
            .bytes()
            .take_while(|b| !matches!(b, b' ' | b':'))
            .collect::<Vec<_>>();
        (2..=4).contains(&reg.len())
            && reg
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            && hex.len() >= 4
            && hex.iter().all(u8::is_ascii_hexdigit)
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
    fn stack_trace_lines_are_never_errors() {
        let h = "[   11.1] host kernel: ";
        for message in [
            "Modules linked in: xhci_pci ucsi_acpi typec_ucsi net_failover failover",
            "RIP: 0010:xhci_handle_cmd_error+0x204/0x210 [xhci_hcd]",
            "RSP: 0018:ffffd102c0a73798 EFLAGS: 00010202",
            "Call Trace:",
            " xhci_handle_command_error+0x114/0x210 [xhci_hcd]",
            " ? ucsi_init_failed+0x1a/0x30 [typec_ucsi]",
            "---[ end trace 0000000000000000 ]---",
        ] {
            assert!(!is_error(&format!("{h}{message}")), "{message}");
            assert!(!is_error(message), "{message}");
        }
        // Short uppercase prefixes that aren't registers aren't trace lines.
        for message in [
            "ACPI: \\_SB_.PCI0: Overriding _PRW",
            "DMAR: DRHD: handling fault status reg 3",
            "CPU1: Core temperature above threshold, cpu clock throttled",
        ] {
            assert!(!is_trace_line(message), "{message}");
        }
        assert!(is_trace_line("PKRU: 55555554"));
        assert!(is_trace_line("CR2: 0000555b279c56c8 CR3: 000000029ba12000"));
        // The report's header, and ordinary lines, still count.
        assert!(is_error(&format!(
            "{h}WARNING: at xhci_ring.c:12 xhci_error+0x1"
        )));
        assert!(is_error(&format!(
            "{h}usb usb2-port3: over-current condition"
        )));
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
