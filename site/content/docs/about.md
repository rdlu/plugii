---
title: About
description: Where plugii comes from, why it exists, and its license.
weight: 21
---

## Where it comes from

plugii began as `usb-ports`, a bash script in its author's dotfiles: a
report of what sat on each socket, at what speed, with the USB-C state and a
few health checks at the end. Every time a port misbehaved the script grew
another rule, and another fix for one of two laptops. plugii is its rewrite
in Rust: the same report, the same rules and the same labels format (feature
parity with the script was the baseline), plus what a script can't do well:
a live view, charts over time, and recordings.

## Why it exists

Two incidents shaped it, and each became a test fixture for the rule it
produced:

- **A ThinkPad's dead USB-C port** (September 2026). A monitor on a USB-C
  port got no picture: the port's SuperSpeed lanes were down, so there was
  no USB 3 and no DisplayPort alt mode, and nothing on screen said why. That
  gave the "hub stuck at USB 2" and "USB-C device but no DisplayPort
  output" checks, with Lenovo's EC reset as the fix.
- **An XPS 13 throttling on its dock** (October 2026). On AC through a
  dock, the CPU was held down, and UCSI kept a stale view of the dock's
  power offers. That gave the BD PROCHOT throttling check and the stale
  USB-C status check.

The common thread: the kernel knows all of it, spread over sysfs, UCSI and
the kernel log, but nothing puts it together by *physical socket*. plugii
does, and keeps doing it while you swap cables.

## Design

- One generic core, no hostnames in the code: what differs between machines
  is discovered (batteries, sensors, peer links), kept in the labels file, or
  in a vendor module picked by DMI (Lenovo and Dell, for firmware catalogs
  and advice).
- Readers take a root path, so the whole tool runs against captured sysfs
  trees in the tests; the report, the TUI screens and the CLI help on this
  site are those tests' snapshots.
- std threads and channels, no async runtime: [ratatui](https://ratatui.rs)
  and crossterm for the TUI, clap for the CLI, udev for events.

## License

MIT. The source is at [github.com/rdlu/plugii](https://github.com/rdlu/plugii);
issues and pull requests are welcome there. Captures of machines plugii gets
wrong (`plugii capture`) are the most useful contribution.

This site is built with [Hugo](https://gohugo.io) and the
[Hextra](https://github.com/imfing/hextra) theme (MIT), and set in
[iA Writer Duo](https://github.com/iaolo/iA-Fonts) and
[0xProto](https://github.com/0xType/0xProto) (both SIL OFL 1.1).
