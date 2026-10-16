# plugii — plan

A Rust CLI + ratatui TUI that shows every physical USB port on a Linux machine —
what's plugged in, at what link speed, the USB-C / PD state, power and CPU
throttling — and updates live while you swap hubs, ports, chargers and devices.

It is a rewrite of the `usb-ports` bash script
(`~/.dotfiles/scripts/dot-local/bin/usb-ports`); feature parity with that
script is the baseline, the TUI and charts are the point.

## Decisions

- **Name:** `plugii` (crate and binary). Public on GitHub, **MIT** license.
- **Commands:** `plugii` (full report + health, exit 1 if anything is wrong),
  `plugii -H` (health only), `plugii --json`, `plugii watch` / `-w` (TUI),
  `plugii capture <dir>` (dump relevant sysfs as a test fixture),
  `plugii watch --record f.jsonl`, `plugii replay f.jsonl`.
- **Labels:** read `~/.config/plugii/labels`, falling back to
  `~/.config/usb-ports/labels`. Same format as the script:
  `<pci>-<port> label`, `peer:<usb2-key> <usb3-key>`, `typec:portN label`,
  `esrt:<guid> label`.
- **Firmware section** (DMI, fwupd, Lenovo/Dell catalogs): last milestone,
  opt-in with `--firmware` (network + cache).
- **No async runtime.** std threads + channels.

## Libraries

| Job | Crate |
|---|---|
| TUI | ratatui + crossterm |
| CLI / errors / output | clap, color-eyre, serde + serde_json |
| USB enumeration + descriptors | nusb (plus our own sysfs reader for root-port `connect_type` / `peer`) |
| Events for usb, typec, power_supply, drm, thunderbolt | udev (`MonitorBuilder`) |
| Vendor/product names fallback | usb-ids |
| Type-C / PD, power, CPU, DRM, Thunderbolt | our own sysfs readers (yuca is a reference, not a dep) |
| Kernel log | `journalctl -k [-f] -o json` subprocess |
| Tests | insta, ratatui `TestBackend`, tempfile |

Reference projects: cyme (descriptor parsing, tree display), usbtree (ghost
devices after unplug, URB/s sparklines, event log), framework-tool-tui.

## Architecture

```
src/
  main.rs / cli.rs   clap commands
  labels.rs          labels file parser
  sysfs/             readers take a root path (real `/` or a fixture dir)
    usb.rs             root ports, USB2/USB3 pairing, device tree, speeds, descriptors, urbnum
    typec.rs           UCSI ports, partners, cables, alt modes, PD source/sink PDOs, active contract
    thunderbolt.rs     domains, security, routers, authorization, link speed
    drm.rs             lit external connectors (+ optional niri mode/VRR)
    power.rs           AC, battery (capacity, flow W, charge limit), ucsi-source-psy-*
    cpu.rs             util, clocks, RAPL package W, package temp (sampled)
    kernel_log.rs      error lines + link-reset counts mapped to sockets
  model.rs           Snapshot (plain data, serde) + History ring buffer of timed samples
  health.rs          pure Snapshot -> Vec<Finding>; same rules as the script
  diff.rs            Snapshot pair -> Events ("+ 0bda:5411 hub @10G on USB-C (left)")
  report.rs          colored text report
  watch.rs           udev monitor thread (debounce ~150 ms -> rescan) + 1 s tick -> channel
  record.rs          jsonl record / replay
  tui/               app state, layout, widgets, keys
```

Sysfs facts observed on the dev machine (`xps`, Dell XPS 9320): no `peer`
links (paired via `peer:` labels), USB 2 on 0000:00:14.0 and USB 3 on
0000:00:0d.0, two Thunderbolt domains, UCSI port0/port1, partner PDOs at
`/sys/class/typec/portN-partner/usb_power_delivery/source-capabilities/N:fixed_supply/{voltage,maximum_current}`,
RAPL `energy_uj` and `urbnum` readable without root. Also readable: each
device's `bos_descriptors` (SuperSpeed / SuperSpeedPlus capability → the
fastest speed it *could* link at), `rx_lanes`/`tx_lanes`, `version` (bcdUSB);
each root port's `over_current_count`, `state` and ACPI `physical_location`
(panel / horizontal / vertical position — wrong on the XPS, where both sockets
say "left", so only a hint for unlabelled sockets, never trusted over labels).

## Machine specifics: one generic core, per-host data, vendor modules

The script grew fixes for two machines: `daisy` (Lenovo ThinkPad, AMD, has
`peer` links, Lenovo PD firmware) and `xps` (Dell XPS 9320, Intel, no `peer`
links, Thunderbolt). plugii keeps all of that without host checks:

- **Discovered, not hard-coded:** battery and mains by power_supply `type`
  (not `BAT0`/`AC`), temperature sensor from a list of known hwmon names
  (`coretemp`, `k10temp`, `zenpower`, …), RAPL via powercap, `peer` links when
  firmware has them, else `peer:` labels.
- **Per-host data stays in the labels file** (stowed per host from dotfiles).
- **Known-benign kernel log patterns** are a table tagged by driver (amdgpu
  `LTTPR`/`REG_WAIT` first).
- **Vendor modules** picked by DMI `sys_vendor`: `vendor/lenovo.rs`,
  `vendor/dell.rs` — firmware catalogs (milestone 8) and vendor-specific fix
  advice attached to generic findings (e.g. Lenovo EC reset procedure).
- **Personal integrations are optional:** niri output mode/VRR only when niri
  runs; the VRR-over-HDMI warning is worded generically.
- **No hostnames in code.** `daisy` / `xps` appear only as fixture names. Each
  past incident (daisy's dead USB-C port 2026-09-27, the XPS dock throttling
  2026-10-16) becomes a fixture for the health rule it produced.
- ThinkPad captures and test runs: the user opens a Claude session on daisy;
  batch those asks into one checklist.

## TUI

Tabs: **Ports** | **Charts** | **Events**.

- Ports: one card per physical socket, USB 3 / USB 2 halves, device tree with
  speed badges (green USB 3, yellow USB 2, red hub stuck at USB 2), flash on
  change, ghosts for unplugged devices (~30 s). Side panels: USB-C/PD (all
  PDOs, active one highlighted), Power, Health.
- Detail view (Enter): strings, serial, driver, bcdUSB, bMaxPower,
  capable-vs-linked speed ("10G capable, linked at 5G → cable/hub limiting").
- Label wizard (`L`): name the selected unlabeled socket, write it to the labels file.
- Charts: power timeline (PD contract W, battery W, CPU package W) with
  vertical markers at plug events; CPU GHz + temp on the same time axis;
  per-port URB/s sparklines; per-socket link-speed timeline strips;
  charger comparison table (max offered, negotiated, CPU GHz/W allowed, resets).
- Keys — vim-first, arrows always work too (the user navigates with vim keys):
  - Move: `j`/`k` (↓/↑), `h`/`l` (←/→: collapse/expand a hub, or previous/next
    pane in a row), `gg` / `G` top/bottom, `Ctrl-d` / `Ctrl-u` half page,
    counts (`5j`).
  - Panes and tabs: `Ctrl-w h/j/k/l` move focus between panes, `gt` / `gT`
    next/previous tab, `1`/`2`/`3` jump to a tab, `Tab` cycles too.
  - `⏎` / `o` details, `Esc` back, `/` filter devices, `n` / `N` next/previous
    match, `L` label socket, `p` pause, `c` clear events, `?` help (lists every
    binding), `q` / `:q` quit.
  - No key does something different from what vim users expect: `l` never
    labels and `h` never opens help.

## Testing

Fixture sysfs trees captured from real machines (`plugii capture`) plus
hand-made fault fixtures: hub stuck at USB 2, UCSI missing, PROCHOT throttle,
unauthorized Thunderbolt device. insta snapshots for report and `--json`, a
unit test per health rule, TUI rendering through `TestBackend`.
Local gates: fmt, clippy `-D warnings`, test. GitHub Actions come with the
release milestone.

## Milestones

Each lands on a local branch, gets reviewed, and fast-forwards local `main`.
No PRs or pushes until the user calls it reliable enough to release.

1. ✅ Scaffold: cargo, MIT LICENSE, README, labels parser, Snapshot + History model.
2. Readers: usb, typec/PD, thunderbolt + `capture` + fixtures. *(parallel with 3)*
3. Readers: drm, power, cpu, kernel log. *(parallel with 2)*
4. Health rules + text report (parity with script, exit codes), `-H`, `--json`.
5. TUI core: layout, live updates (udev + tick), Events tab, Charts tab with power/CPU timeline.
6. TUI extras: detail view, capable-vs-linked, label wizard, help, URB sparklines, speed timeline, charger table.
7. `--record` / `replay`.
8. Firmware section (`--firmware`), with the vendor modules.
9. Release: GitHub Actions (CI + release binaries), docs, GitHub Pages site,
   first tagged release.
