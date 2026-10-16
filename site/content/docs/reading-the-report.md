---
title: Reading the report
description: A real plugii report from a Dell XPS 13 9320, section by section; what the halves, speeds, USB-C / UCSI, power, kernel log and health lines mean.
weight: 3
---

`plugii` with no arguments prints one report and exits. This is a real one,
from a Dell XPS 13 9320 with a USB-C hub (a webcam, a keyboard and a
billboard device behind it) on the left port, and a monitor plus a 47 W
charger on the right one.

{{< snapshot name="report__xps_report" title="$ plugii" >}}

> [!NOTE]
> The reports on this page are the snapshots plugii's test suite checks
> (machine captures under `tests/fixtures/`, collected offline), so they
> match what the current code prints. In a terminal the states and marks
> are coloured: green for linked and ✓, yellow for ⚠, red for ✗.

## USB ports

{{< snapshot name="report__xps_report" section="USB ports" >}}

One block per **physical socket**: every root port whose `connect_type` is
`hotplug`, visited from its USB 2 side. The title is the socket's label (or
`unlabeled`), then its **key**, the USB 2 root port's
`<pci-address>-<port>`. Labels come from the [labels file](../labels/).

Each socket has two **halves**. A USB-C or USB 3 Type-A socket is wired to
two root ports, one on a USB 2 bus and one on a USB 3 (SuperSpeed) bus, and
a device links on one or the other:

- **`USB 3 half: linked`**: something is on the SuperSpeed lanes; its tree
  follows. A USB 3 hub shows on both halves, since it is two hubs in one.
- **`USB 2 half: linked`**: something is on the USB 2 pair: a USB 2 device,
  or the USB 2 side of a hub.
- **`idle`**: nothing on that half.
- **`unknown`**: on machines whose firmware has no `peer` links at all,
  plugii can't find a socket's USB 3 half unless a `peer:` line pairs it.
  Where the firmware does have them, a socket without one is USB 2-only and
  gets no USB 3 line.

Under a half, each device is `vid:pid`, the **link speed**, and its name
(the `device:` label if there is one, else the product string), indented by
hub depth:

| Speed | Meaning |
| --- | --- |
| `20G (USB 3 Gen 2x2)` | SuperSpeed+ over two lanes |
| `10G (USB 3 Gen 2)` | SuperSpeed+ |
| `5G (USB 3 Gen 1)` | SuperSpeed |
| `480M (USB 2)` | High speed |
| `12M (USB 1 full)` | Full speed: keyboards, mice, receivers |
| `1.5M (USB 1 low)` | Low speed |

The `USB-C:` line appears when the labels tie a UCSI port to the socket (a
`typec:` line with the same label). With no USB data on either half, it
says what the partner probably is: `display in DP alt mode → DP-3`, or
`no USB data, no DisplayPort (charger only?)`.

Two warnings can follow a socket:

- `⚠ hub linked at USB 2 only — its USB 3 half is missing`: a hub (class
  09) on the USB 2 half while the USB 3 half is idle. A USB 3 hub's
  SuperSpeed link didn't come up, and with it no DisplayPort alt mode
  either. See [Health checks](../health/#usb-sockets).
- `⚠ over-current reported on this socket (N×)`: the root ports counted
  over-current events.

The report shows the linked speed. The TUI's detail view adds the speed a
device is **capable** of, from its BOS descriptor, and whether something
holds it back: see [capable vs. linked](../tui/#capable-vs-linked).

## USB-C (UCSI)

{{< snapshot name="report__xps_report" section="USB-C (UCSI)" >}}

Linux learns about USB-C through UCSI, the firmware interface to the USB-C
controller. Each port is `portN`, with its label when a `typec:` line gives
one, then:

- `partner` or `empty`: whether something is plugged in.
- `pd=yes|no`: whether the partner speaks USB Power Delivery.
- `alt-modes=N`: alternate modes the partner offers (DisplayPort, on a
  monitor or a dock).
- `power-in=charging|no`: whether power comes in through this port.
- `cable=passive|active`, when UCSI reports the cable.

`offers:` lists the partner's **source PDOs**, what a charger offers: fixed
`20V 2.35A`, variable `5-20V 3A`, `PPS 3.3-21V 3A`, or battery `…W`. The one
in use is marked `▶` with its watts. `offers: not reported` means power
comes in but the partner listed no PDOs: some chargers and docks don't, and
Lenovo's UCSI never does. That isn't an error.

If UCSI didn't start at all, the section says `no USB-C ports registered`
and Health has a ✗ with the fix.

## Thunderbolt / USB4

{{< snapshot name="report__xps_report" section="Thunderbolt / USB4" >}}

Only on machines with a Thunderbolt or USB4 controller. A line per
**domain** (controller) with its security level (`user`: devices need
authorizing) and whether IOMMU DMA protection is on; then a line per
attached device: its route, vendor and name, `authorized` or `not
authorized`, generation and link speed (`rx 20.0 Gb/s×2`). A device that
isn't authorized gets no PCIe tunnels; Health says how to authorize it.

## Display connectors

{{< snapshot name="report__xps_report" section="Display connectors" >}}

Lit external connectors from DRM: `DP-N connected (USB-C alt mode)` is a
monitor over USB-C, `HDMI-A-N connected` one on HDMI. When niri is running,
each line adds the mode and the VRR state.

## Power

{{< snapshot name="report__xps_report" section="Power" >}}

`AC` is the mains supply (`online`, `offline`, or `no mains supply` on a
machine without one), then the USB-C port the power arrives through, as
`port1 (USB-C (right)) 20V 2.35A ≈ 47W`. Some UCSI firmware (Lenovo's)
reports the current but no voltage: then it says `— power level not
reported (drawing 3.25 A)`. Then the battery: percentage, status, and the
charge limit when one is set.

When plugii can sample the CPU, a second line follows. This one is from a
laptop on battery:

{{< snapshot name="report__amd_battery_report_with_kernel_faults" section="Power" >}}

How busy the CPU is, the average clock with the maximum and the **floor**
(the lowest clock the CPU allows), RAPL package power, package temperature,
and the battery's flow (`−` discharging, `+` charging). The throttling check
reads this line: a busy CPU held near its floor on AC means the embedded
controller doesn't trust the power source. When the CPU is mostly idle
there's nothing to judge, so the report asks you to load it (a build, for
example) and run plugii again.

## Kernel log (this boot)

{{< snapshot name="report__xps_report" section="Kernel log (this boot)" >}}

plugii runs `journalctl -k -b` and keeps the lines that name a real USB,
USB-C or link-training failure: UCSI failures and errors, over-current,
`device descriptor read`, `not accepting address`, `unable to enumerate`,
`cannot enable`, failed link training, `HC died`, xHCI errors. Lines inside
a kernel `WARNING:` stack trace don't count. Known-benign warnings (amdgpu's
`LTTPR` and `REG_WAIT`) are only counted.

With errors, the section shows the last five with their time since boot,
the socket they happened on (`← USB-C (left)`) when the device can be
placed, and `(at boot)` in the first minute; then the **link resets**
(`reset SuperSpeed` / `reset high-speed` lines) per socket or port:

{{< snapshot name="report__amd_battery_report_with_kernel_faults" section="Kernel log (this boot)" >}}

## Firmware

Only with `--firmware`: BIOS and EC versions from DMI, the vendor catalog's
verdict, labelled firmware and fwupd's updates. See [Firmware](../firmware/).

## Health

{{< snapshot name="report__xps_report" section="Health" >}}

The verdict: a line per check, `✓` fine, `⚠` worth a look, `✗` broken,
each with what to do. It ends in `→ all good`, or in `→ something needs a
look` when any line is ⚠ or ✗, and then plugii exits 1. `plugii -H` prints
only this section. [Health checks](../health/) lists every finding.

## As JSON

`plugii --json` prints `{"snapshot": …, "findings": […]}`: everything the
report is made from, and each finding as `{"level": "ok" | "warn" | "fail",
"message": …}`. The exit status is the report's.

```sh
plugii --json | jq -r '.findings[] | select(.level != "ok") | .message'
```
