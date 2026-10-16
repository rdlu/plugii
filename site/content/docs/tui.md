---
title: The TUI
description: plugii watch, the live view - Ports, Charts and Events tabs, the detail view, capable vs. linked speed, ghosts, the filter and the charger table.
weight: 5
---

`plugii watch` (or `plugii -w`) is the report kept live. It rescans the
machine on every udev event from the `usb`, `typec`, `power_supply`, `drm`
and `thunderbolt` subsystems (a plug comes as a burst, so it waits for
150 ms of quiet, at most 500 ms) and once a second otherwise, for the CPU,
power and URB counters. The kernel log is read on plug events and every
10 seconds.

The keys are vim-first and the arrow keys always work too: the
[keymap](../reference/keymap/) lists every one, and <kbd>?</kbd> shows them
in the TUI.

## The top line

```text
 Ports │ Charts │ Events   daisy                                     updated 12:00:00
```

The tabs (<kbd>1</kbd> <kbd>2</kbd> <kbd>3</kbd>, <kbd>g</kbd><kbd>t</kbd> /
<kbd>g</kbd><kbd>T</kbd>, <kbd>Tab</kbd>), the host name, and on the right
the status: when the last scan was taken, or ` PAUSED ` after <kbd>p</kbd>
(updates are held, not lost: they all apply on resume). Keys typed so far
(`g`, `^W`, `:q`), the active filter (`/hub`), `● REC file` while
recording, and the label wizard's outcome show there too.

## Ports

{{< snapshot name="tui__ports_daisy_dock_120x40" title="plugii watch: a ThinkPad with a dock on the hinge port" >}}

**Sockets**, on the left, is the report's USB ports section as a list you
move through: a card per socket (`─ label ──── key ─`), its USB 3 and USB 2
halves, the device tree under each, and the USB-C partner. Speed badges are
coloured like the TUI's terminal palette shows them: **green** for a
SuperSpeed link (5G and up), **yellow** below it, **red** for a hub stuck
at USB 2. Hubs fold: <kbd>h</kbd> / <kbd>l</kbd> on a hub collapse and
expand it (`▸`, with `(+N)` devices hidden).

On the right, three panels that scroll:

- **USB-C / PD**: each UCSI port and its partner, PD, charging, cable,
  every source PDO with the active one highlighted, and the contract.
- **Power**: AC and which USB-C port feeds it, the battery (with its flow
  in watts), and the CPU line.
- **Health**: the [findings](../health/), with your vendor's advice.

<kbd>Ctrl-w</kbd> <kbd>h</kbd>/<kbd>j</kbd>/<kbd>k</kbd>/<kbd>l</kbd> move
the focus between panes; <kbd>h</kbd> / <kbd>l</kbd> do too when the row
isn't a hub. Below 100 columns the panels go under the sockets.

### Flash

A socket whose state just changed (something plugged, unplugged or relinked)
is highlighted for 2 seconds, so you see where a plug landed.

### Ghosts

An unplugged device stays in its socket's tree for **30 seconds**, dim and
struck through, its top row marked `(gone)`: you can still see what was
there and open its details. A ghost goes as soon as the same device (same
sysfs name and vid:pid) comes back.

### The filter

<kbd>/</kbd> opens a prompt; what you type filters the tree as you type, by
vid:pid, product, manufacturer or label, ignoring case. The socket titles
stay, the matching devices and the hubs above them stay, the matches in
bold. <kbd>⏎</kbd> applies it, <kbd>Esc</kbd> cancels it (and clears the
filter); <kbd>n</kbd> / <kbd>N</kbd> jump to the next and previous match.
On the Events tab, the same prompt filters event lines.

## The detail view

<kbd>⏎</kbd> or <kbd>o</kbd> opens the selected device's details in a
popup (<kbd>j</kbd>/<kbd>k</kbd> scroll it, <kbd>Esc</kbd> or <kbd>q</kbd>
close it):

```text
┌ 0b95:1790 AX88179B ──────────────────────────────────────┐
│ vendor:product     0b95:1790                             │
│ label              —                                     │
│ manufacturer       ASIX                                  │
│ product            AX88179B                              │
│ serial             REDACTED                              │
│ driver             cdc_ncm                               │
│ bcdUSB             3.20                                  │
│ device class       00 (per interface)                    │
│ bMaxPower          184 mA                                │
│ lanes              rx 1 · tx 1                           │
│ urbnum             486                                   │
│                                                          │
│ sysfs name         7-1.2                                 │
│ socket             USB-C (hinge) (0000:06:00.3-1)        │
│ half               USB 3 (bus 7, root port 1)            │
│ path               7-1 › 7-1.2                           │
│ via                05e3:0626 USB3.1 Hub                  │
│                                                          │
│ linked at          5G (USB 3 Gen 1)                      │
│ capable of         5G (USB 3 Gen 1)                      │
│ capable vs linked  linked at its best speed (5G)         │
└──────────────────────────────── Esc/q close · j/k scroll ┘
```

On a socket's title or half rows it shows the socket instead: both halves
with their bus and root port, the device on each, the port state, the
over-current count, ACPI's physical location (a hint only: labels win over
it, and on some machines every socket says "left"), and the USB-C port.

### Capable vs. linked

A device's BOS descriptor says the fastest speed it supports (SuperSpeed,
SuperSpeedPlus, the lane count). plugii compares it with the speed it
linked at:

| The detail view says | Meaning |
| --- | --- |
| `linked at its best speed (5G)` | Nothing holds it back. |
| `10G capable, linked at 5G → cable or hub limiting` | Something in between is slower: a 5G hub, a USB 2 cable, a port that can't do more. The tree marks the badge with a dim `↓`. |
| `USB 2 half of a USB 3 hub (its USB 3 half is linked)` | Normal: a USB 3 hub's USB 2 side carries the same descriptor and reads "capable 5G, linked 480M" while healthy. |
| `can't be told: …` | A slow USB 2 device without a BOS descriptor; it may be full-speed only. |

## Charts

The Charts tab puts power and CPU on one time axis, up to the last 10
minutes, with a vertical marker at every plug and unplug:

- **Power (W)**: the USB-C PD contract, the battery flow (negative while
  discharging) and the CPU package power.
- **CPU (GHz, °C)**: the average clock and the package temperature.

Under them, on the same time axis or as a running window:

- **Link speed**: a strip per socket, a cell per moment, by the faster
  half's link: `·` nothing, `▄` USB 2, `█` 5G or more, `▒` a hub stuck at
  USB 2. A dock that comes up on its USB 2 half first and then links at 5G
  draws `▒▒▒████`.
- **URB/s**: USB request blocks per second per socket, as a sparkline with
  the newest on the right and the current rate: whether a device behind a
  port is actually moving data.
- **Chargers this session**: a row per charger you've plugged in this
  session (the same charger replugged keeps its row), the one charging now
  marked `●`:

```text
  Charger      Port              Negotiated     CPU max Pkg max Resets Charging
  not reported USB-C (caps lock) drawing 2.75 A 4.2 GHz 28 W    3      29s
● not reported USB-C (caps lock) drawing 3.25 A 2.1 GHz 10 W    0      34s
```

Its best offer, what was negotiated, the highest CPU clock and package
watts seen while it charged, the link resets that happened meanwhile, and
for how long. It answers "which of my chargers lets this laptop run at full
speed?": plug each in, run a build, compare. On a short terminal the lower
panes shrink and scroll; <kbd>Ctrl-w</kbd> <kbd>j</kbd> pages to them.

## Events

{{< snapshot name="tui__events_tab" >}}

Every change between two scans, with its time:

| Sign | Kind | Examples |
| --- | --- | --- |
| `+` | plugged in, appeared | a device with its speed and socket, a charger, a Thunderbolt device, a display |
| `−` | unplugged, gone | the same going away; a labelled hub leaving is one line: `− USB switch (desk): 6 devices left` |
| `~` | changed | link speed, PD contract, charging, AC, Thunderbolt authorization |
| `!` | went wrong | a new kernel error line, link resets on a socket |

Battery percentage and the current a supply draws move all the time, so they
aren't events. The list follows the newest event until you move up, keeps
the last 1000, and <kbd>c</kbd> clears it.

## Labels

<kbd>L</kbd> on the Ports tab names the selected socket or device and writes
it to the labels file: see [Labels](../labels/#label-from-the-tui).

## In a replay

`plugii replay` plays a recording in this same TUI, with the recorded times
on the events and charts: see [Record and replay](../record-replay/).
