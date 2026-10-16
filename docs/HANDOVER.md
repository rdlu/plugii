# Handover: xps → daisy (2026-10-16)

The project moves from the work laptop (`xps`, Dell XPS 9320) to the ThinkPad
(`daisy`), where development continues. This file holds everything the first
session on daisy needs that isn't in the code or `docs/PLAN.md`. Read it with
`docs/PLAN.md` and `CLAUDE.md`, then fold anything still useful into your own
memory and delete this file.

## Where things are

- Remote: `git@github.com:rdlu/plugii.git` (public, MIT). **Work on `dev`.**
  GitHub `main` still holds only the plan commit, on purpose.
- Done: milestones 1–4 (scaffold, every sysfs reader, `plugii capture`,
  collect + health + text report, `-H`, `--json`, `plugii labels`).
- Next: the daisy checks below, then milestone 5 (TUI core).

## How the user wants to work

- **No PRs for now.** Each milestone goes on a local branch, gets reviewed,
  fast-forwards local `main`, then `git push origin main:dev`. PRs, GitHub
  Actions, docs and a GitHub Pages site come with the first release, when the
  user calls it "truly reliable" (milestone 9). This overrides the user's
  global "branch and PR always" rule for this repo only.
- SSH only for git, never HTTPS (the security key blinks; retry up to 3×).
- Delegate implementation to subagents (Opus); plan and review on the main
  session; a review pass between milestones.
- **Vim-first navigation** is the user's favourite: the TUI keymap in
  `docs/PLAN.md` is a requirement, not a nice-to-have.
- Fixtures are committed to a public repo: `plugii capture` redacts serials,
  `unique_id`, `eppid`, DMI serial/uuid. Check every new capture for
  per-unit identifiers before committing.

## Decisions made on xps that aren't obvious from the code

1. **UCSI supply ↔ port mapping**: supply `ucsi-source-psy-*:00N` ↔ ACPI
   connector `CR0N` (typec port `firmware_node/path`) ↔ `port(N-1)`. Never
   remap by `power_role`: the Dell's UCSI reported `[source]` on a port that
   was sinking 20 V. Charging = the mapped supply is online.
2. **Partner PDOs can be stale.** UCSI keeps the offers from when the partner
   registered. On xps the dock kept "offering" 5–20 V after it stopped
   supplying power (proven by an unplug test: with the monitor unplugged, the
   laptop went to battery although the dock "offered" 20 V). Health warns
   about that case. The opposite case (supply online, partner reports no
   offers, e.g. the dock passing through a charger) is benign: it shows
   "offers not reported" and doesn't warn.
3. **A USB 3 hub's USB 2 half carries the same BOS**, so it reads "capable
   5G, linked 480M" even when healthy. The capable-vs-linked view (milestone
   6) must skip a hub's USB 2 half when its USB 3 half is linked.
4. `capable_speed` for full-speed USB 2 devices is `None`, not High (keyboards
   and YubiKeys claim bcdUSB 2.00 but only do 12M). 20G Gen 2x2 reads as 10G
   capable (lane count isn't in BOS).
5. `driver` is the interfaces' drivers joined; the device's own link is
   always `usb`.

## The user's hardware (for reading reports and captures)

- xps sockets: `USB-C (left)` = `0000:00:14.0-1` (USB 2) / `0000:00:0d.0-1`
  (USB 3) / `port0`; `USB-C (right)` = `0000:00:14.0-3` / `0000:00:0d.0-3` /
  `port1`. No kernel peer links; paired by `peer:` labels.
- A dock (Genesys hubs) with **no power brick** of its own; behind it a
  **USB switch with a button** shared with another computer (when switched
  away, its hub and devices vanish: not a fault).
- A monitor with a 100 W brick that **passes only 47 W** to the laptop
  (offers 5V 1A · 9V 1A · 12V 1.5A · 15V 1.8A · 20V 2.35A). The XPS ships
  with a 60 W charger; through the dock that charger arrives as 20 V × 2.63 A
  ≈ 53 W (the dock keeps ~7 W).
- daisy: Lenovo ThinkPad, AMD, has kernel `peer` links, Lenovo PD firmware via
  an `esrt:` label. The script notes daisy's USB-C port died on 2026-09-27
  (hub stuck at USB 2, no DP alt mode; fixed by an EC reset).

## Feature requests captured but not built yet

- **Device labels** (milestone 6): `device:<vid>:<pid>  <name>` lines in the
  labels file name a device; tree, details and the event log use the name;
  when a labelled hub leaves, the event collapses to "− USB switch (desk): 6
  devices left".
- **Charging-power hint**: show "best offer N W" now; a configurable
  low-power threshold later (it depends on the laptop, so not hard-coded).
- Charger comparison table, power timeline with plug markers, etc.: see
  `docs/PLAN.md` → TUI.

## First session on daisy: checklist

1. `git clone -b dev git@github.com:rdlu/plugii.git ~/Projects/plugii`
   (or wherever the user wants it), `cargo test`, and
   `cargo test --test live -- --ignored --nocapture`.
2. `cargo run` (full report) and `cargo run -- -H`: compare with the old
   script (`usb-ports`) side by side. Every difference is either a bug or a
   documented deviation (see the milestone 4 notes in git log).
3. Check things xps couldn't exercise: kernel `peer` links (sockets should
   pair without `peer:` labels; ports without a peer show USB 2-only),
   `k10temp` Tctl, RAPL on AMD, amdgpu benign log lines, the `esrt:` label.
4. Capture fixtures: `cargo run -- capture tests/fixtures/sysfs/daisy`
   (normal setup), plus variants (nothing plugged in; charger direct; dock).
   Review the redaction, commit, and add report snapshots for them like the
   xps ones in `tests/report.rs`.
5. Then milestone 5 (TUI core) per `docs/PLAN.md`.
