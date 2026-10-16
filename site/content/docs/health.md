---
title: Health checks
description: Every finding plugii's health verdict can give, grouped by area, with what it means and what to do.
weight: 4
---

The report ends in a **Health** section: a line per check, in a fixed order
(USB sockets, USB-C, Thunderbolt, displays, power, CPU, kernel log, then
firmware with `--firmware`). Each line has a level:

| Mark | Level | JSON `level` | Exit status |
| --- | --- | --- | --- |
| `✓` | fine | `ok` | 0 if every line is ✓ |
| `⚠` | worth a look | `warn` | 1 |
| `✗` | broken | `fail` | 1 |

Findings name sockets and USB-C ports by their [labels](../labels/), so
`USB-C (left)` below stands for whatever yours are called. A check that
has nothing to look at says nothing: no Thunderbolt line on a machine
without Thunderbolt, no CPU line when the CPU is idle.

Here is a ThinkPad with a few things wrong, run with `--firmware`:

{{< snapshot name="report__daisy_firmware_faults_health" title="$ plugii -H --firmware" >}}

## USB sockets

#### ✗ `<socket>: hub stuck at USB 2`

> `USB-C (hinge): hub stuck at USB 2 — the port's SuperSpeed lanes are
> down, so no USB 3 and no DP alt mode either. Flip the plug; if that
> fails, EC reset`

A hub is linked on the socket's USB 2 half while its USB 3 half is idle.
USB 2-only hubs are rare now, so this is a USB 3 hub (or a dock) whose
SuperSpeed link never came up. Whatever needs those lanes is gone too,
including DisplayPort alt mode.

**What to do:** flip the USB-C plug, or replug it. If the lanes stay down
with every cable and device, the port's controller is stuck: reset the
embedded controller (on Lenovo machines plugii says how; see
[vendor advice](#vendor-advice)).

#### ✗ `<socket>: over-current on its root port (N×)`

The socket's root ports (either half) counted over-current events: a device
or a cable drew too much current, or shorted.

**What to do:** unplug what's there and check the cable. The count is the
kernel's since boot.

#### ✓ `USB 3 links up (SuperSpeed lanes working)`

At least one socket has a device on its USB 3 half, and no socket has a hub
stuck at USB 2. With nothing on any USB 3 half, plugii can't tell and says
nothing.

## USB-C

#### ✗ `USB-C status driver (UCSI) didn't init`

> `USB-C status driver (UCSI) didn't init — Linux can't see USB-C state.
> Video still works; fix: pkexec sh -c 'modprobe -r ucsi_acpi && modprobe
> ucsi_acpi', or reboot`

No UCSI ports are registered, so Linux knows nothing about partners,
chargers or alt modes. Video and data still work; plugii just can't see
the USB-C side.

**What to do:** reload the driver with the command in the message, or reboot.

#### ✓ `USB-C status driver (UCSI) up: N port(s)`

UCSI is running.

#### ⚠ `USB-C status for <port> looks stale`

> `USB-C status for USB-C (left) looks stale (charging state and advertised
> power disagree); replug the cable to refresh`

The partner still lists source PDOs, but the port's supply is offline and
UCSI calls the port a sink: leftovers from an earlier state of the partner.
Seen on an XPS 9320 after its cables moved: one port kept a dock's old
offers until both cables were replugged.

**What to do:** replug the cable to make the controller report again.

## Thunderbolt / USB4

#### ⚠ `Thunderbolt device(s) not authorized, so no PCIe tunnels`

> `Thunderbolt device(s) not authorized, so no PCIe tunnels: WD19TB
> (0-1) — boltctl authorize <uuid> (or enroll to make it stick)`

With security level `user`, a device has to be authorized before its PCIe
tunnels (a dock's Ethernet, its USB controller) come up.

**What to do:** `boltctl authorize <uuid>` for this session, or `boltctl
enroll <uuid>` to have boltd authorize it every time.

#### ✓ `Thunderbolt/USB4: N domain(s), security …, N device(s) attached`

Every attached device is authorized.

## Displays

#### ✓ `external display on USB-C (DP alt mode working)` / `external display on HDMI`

A DisplayPort connector is lit (on a laptop, that's USB-C alt mode), or an
HDMI one.

#### ⚠ `VRR is always-on over HDMI`

> `VRR is always-on over HDMI (HDMI-A-1) — some monitors flicker with it;
> if this one does, set 'variable-refresh-rate on-demand=true' on its niri
> output`

Only when niri runs and reports VRR on for an HDMI output. Some monitors
flicker with variable refresh always on.

**What to do:** nothing if the picture is fine; otherwise make VRR
on-demand in niri's output config.

#### ⚠ `USB-C device on <port> but no DisplayPort output`

> `USB-C device on USB-C (caps lock) but no DisplayPort output — if it's a
> monitor, alt mode failed: check the cable on another machine, then EC
> reset`

A USB-C partner that isn't charging the laptop, while no DisplayPort
connector is lit. If that partner is a monitor, DisplayPort alt mode never
came up. This is how a dead USB-C port first showed on a ThinkPad.

**What to do:** if it's not a monitor (a phone, a drive), ignore it. If it
is, try the cable and monitor on another machine; if they work there, reset
the embedded controller.

## Power

#### ✓ `charging power arriving via USB-C <port> …`

> `charging power arriving via USB-C port1 (USB-C (right)) 20V 2.35A ≈ 47W
> (best offer 47 W)`

AC is online through a USB-C port: the negotiated voltage and current, and
the best the charger offers. When the supply reports no voltage, it says
`power level not reported (drawing 3.25 A)`; when the charger lists no
offers, `charger's offers not reported`. Neither is a problem.

#### ✓ `on AC power (via a port UCSI doesn't report …)`

AC is online, but no USB-C supply is: a barrel charger, or a hub's
charging pass-through that UCSI doesn't see.

## CPU throttling

These need a CPU sample with clocks, and a CPU at least **50 %** busy: an
idle CPU says nothing about what the power source allows. A busy CPU whose
average clock is within **115 %** of its minimum is throttled.

#### ✗ `CPU pinned at its minimum clock while N% busy on AC`

> `CPU pinned at its minimum clock while 100% busy on AC (4 W package) —
> the EC is throttling (BD PROCHOT), usually a charger, cable or dock it
> doesn't trust. Plug the charger straight into the laptop, or try another
> charger`

The embedded controller is holding the CPU down with BD PROCHOT. On a
laptop on AC this is usually the power source: a charger too weak for it,
or a dock or cable in between that the EC doesn't trust.

**What to do:** plug the charger into the laptop directly, or try another
charger. The [Charts tab's charger table](../tui/#charts) compares the
chargers you've tried, with the clock and package watts each allowed.

#### ✗ `CPU pinned at its minimum clock while N% busy on battery`

The same, on battery: BD PROCHOT, or a low-battery limit.

**What to do:** check the power profile and the battery's health.

#### ✓ `CPU not power-throttled (N% busy at X GHz, N W package)`

Busy, and clocking above its floor.

## Kernel log

#### ✗ `N USB/USB-C/display error line(s) in the kernel log`

At least one line this boot names a real failure (see [Reading the
report](../reading-the-report/#kernel-log-this-boot) for the patterns).

**What to do:** read the lines in the report's Kernel log section, or all
of them with `journalctl -k -b`. The socket each happened on is shown when
plugii can place it. Lines from boot can be old news: the report marks
those `(at boot)`.

#### ✓ `kernel log clean`

No such lines this boot. Known-benign driver warnings don't count.

#### ⚠ `N USB link resets this boot on …`

> `8 USB link resets this boot on port 1-1 ×2, port 2-1 ×3, … — flaky
> cable/dock or a marginal SuperSpeed link; reseat or swap the cable`

Five or more link resets since boot, by socket. A few are normal (plugging
in a hub, suspend); many mean the link keeps dropping.

**What to do:** reseat the cable, or swap it; then watch the count in the
TUI.

## Firmware

Only with `--firmware`; see [Firmware](../firmware/) for where each
version comes from.

#### ✓ / ⚠ `BIOS up to date` / `BIOS update available` (Lenovo)

> `BIOS update available: R2KET40W → build 41. Install from Windows
> (r2kuj41w.exe); Lenovo's bootable ISO flasher doesn't work on every model`

The running BIOS build against the newest in Lenovo's catalog for the
machine type. A catalog BIOS of another family isn't compared.

#### ✓ / ⚠ `<label> up to date` / `<label> update available` (Lenovo PD firmware)

> `USB-C PD firmware update available: 0x104 → 0x105 (Windows-only installer)`

For each `esrt:` labelled firmware whose label contains `PD`: its version
from fwupd against the PD firmware in Lenovo's catalog. Lenovo ships it
only as a Windows installer.

#### ✓ / ⚠ `BIOS up to date` / `BIOS update available` (Dell)

> `BIOS update available: 2.30.0 → 2.32.0 (Recommended, October 07, 2026) —
> not on LVFS yet; flash from F12 → BIOS Flash Update with the .exe on the
> ESP, or wait for fwupdmgr`

The running BIOS against the newest in Dell's catalog for the system ID.
Dell's catalog gets releases before LVFS, so the advice depends on fwupd:
`fwupd has it too: fwupdmgr update` when fwupd offers a System Firmware
update, the message above when it doesn't, and only the F12 route when
fwupd isn't installed.

#### ✗ `fwupd recorded a failed firmware update: <device>`

fwupd marks the device's last update as failed.

**What to do:** `fwupdmgr get-history` for the details; retry or report it.

#### ⚠ `fwupd has N firmware update(s): <name> <version> → <release> — fwupdmgr update`

fwupd's cached metadata offers updates.

**What to do:** `fwupdmgr update`. The list is only as new as the last
`fwupdmgr refresh`.

## Vendor advice

Findings are generic; a vendor module, picked by DMI `sys_vendor`, can add
its own advice. Findings that end in `EC reset` (a hub stuck at USB 2, a
monitor with no DisplayPort) get how to do it on **Lenovo** machines:

> `… Flip the plug; if that fails, EC reset (power off, unplug all, hold
> power 15 s)`

That is: shut down, unplug everything including the charger, then hold the
power button for 15 seconds before starting it again. **Dell** machines get
no extra advice; on others, look up how your model resets its embedded
controller (often the same procedure, or a pinhole button).

The TUI shows the same findings in its Health panel, with the vendor's
advice for the machine it runs on; a replay, which may come from another
machine, shows them without.
