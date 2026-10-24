# Changelog

All notable changes to plugii are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and plugii follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Updated dependencies, among them ureq 3.5 and rustls-webpki 0.103.16.

## [0.1.0] - 2026-10-17

The first release: a rewrite of the `usb-ports` shell script as a Rust CLI
and live TUI, with feature parity plus everything below.

### Added

- **Report** (`plugii`): every physical USB socket with what is plugged in
  and its link speed, the USB 2 and USB 3 halves side by side, so a port
  that fell back to USB 2 stands out. Device trees behind hubs, with the
  speed each device is capable of next to the one it linked at.
- **USB-C and Power Delivery**: partners, the source's offered PDOs, the
  negotiated contract, and which charger the machine is drawing from.
- **Thunderbolt / USB4** domains and devices with their authorization,
  lit DisplayPort / HDMI connectors (with mode and VRR under niri), AC and
  battery state, CPU clocks, utilisation, package power and temperature,
  and kernel log USB errors and link resets per socket this boot.
- **Health verdict** at the end of the report, and `plugii -H` for the
  verdict alone. The exit status is 1 when any finding is a warning or a
  failure, so it fits scripts and status bars. Checks include hubs stuck
  at USB 2, over-current, a USB-C status driver that didn't start or shows
  stale offers, unauthorized Thunderbolt devices, USB-C devices without
  their DisplayPort output, a CPU pinned at its minimum clock, and USB
  errors and link resets in the kernel log.
- **`--json`**: the whole snapshot and the findings as JSON.
- **Live TUI** (`plugii watch`, `-w`) with vim keys, updating as devices,
  chargers and displays come and go:
  - Ports tab with a detail view, capable-vs-linked speeds, a filter, and
    recently unplugged devices kept as ghosts;
  - Charts tab with a power and CPU timeline, per-socket link-speed strips,
    URB/s sparklines and a charger table;
  - Events tab: plugs, unplugs, speed changes and kernel errors as they
    happen;
  - `?` for the key list.
- **Labels** in `~/.config/plugii/labels`: names for sockets, USB-C ports,
  firmware entries, manual USB 2 / USB 3 pairing, and `device:<vid>:<pid>`
  names for devices. A labelled hub leaving shows as one event. `L` in the
  TUI names the selected socket or device and writes the file for you;
  `plugii labels` lists what the file names.
- **Record and replay**: `plugii watch --record session.jsonl[.gz]` saves
  every snapshot while you watch (`--redact` removes serial numbers and
  Thunderbolt IDs, for recordings you share); `plugii replay` plays it back
  in the same TUI with pause, speed steps and jumps to the next plug event.
- **Firmware checks** (`--firmware`, opt-in, uses the network): BIOS and EC
  versions, the Lenovo and Dell update catalogs for newer BIOS and USB-C PD
  firmware, fwupd devices, failed and pending updates, and Lenovo EC reset
  advice. Catalogs are cached for a day; `--refresh` fetches again.
- **`plugii capture <dir>`**: copies the sysfs files plugii reads, redacted,
  for bug reports and test fixtures.
- Man pages and bash, zsh and fish completions in the release tarballs
  (generated with the hidden `plugii manpages <dir>` and
  `plugii completions <shell>`).
- Documentation site at <https://plugii.dlu.one/>.

[Unreleased]: https://github.com/rdlu/plugii/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rdlu/plugii/releases/tag/v0.1.0
