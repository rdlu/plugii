---
title: Record and replay
description: Record a plugii watch session to JSON lines, play it back at up to x60, and the file format.
weight: 7
---

Plug-in problems are often intermittent: the dock that links at USB 2 one
time in five, the charger that throttles after ten minutes. Record the
session while you try things, then play it back, or send it to someone.

## Record

```sh
plugii watch --record session.jsonl
```

The TUI runs as usual and every snapshot goes to the file, even while the
view is paused; the status line shows `● REC session.jsonl`.

| Flag | Effect |
| --- | --- |
| `--record <FILE>` | Record to `FILE`. A name ending in `.gz` is gzip-compressed. |
| `--redact` | Replace device serial numbers, Thunderbolt unique IDs and serial numbers in kernel log lines with `REDACTED`, for recordings you share. |
| `--force` | Overwrite `FILE` if it exists. Without it, an existing file stops plugii before the TUI starts. |

```sh
plugii watch --record ~/dock-$(date +%F).jsonl.gz --redact
```

Each line is flushed as it's written (a gzip sync flush for `.gz`), so if
plugii or the machine dies, the file still plays up to its last line. If a
write fails, recording stops and the status line says why; the TUI goes on.

## Replay

```sh
plugii replay session.jsonl.gz
```

The recording plays in the same TUI, through the same code as live: the
events, flashes, ghosts and charts come out as they did, with the recorded
times. The status line shows where you are:

```text
▶ replay 00:01:12 / 00:04:30 ×10  session.jsonl.gz
```

| Key | In a replay |
| --- | --- |
| <kbd>p</kbd> | pause / resume playback (`⏸` in yellow while paused) |
| <kbd>+</kbd> / <kbd>-</kbd> | faster / slower: ×1 ×2 ×4 ×10 ×30 ×60 |
| <kbd>]</kbd> / <kbd>[</kbd> | jump to the next / previous plug or unplug |
| <kbd>gg</kbd> / <kbd>G</kbd> | still top / bottom of the list, as live |
| <kbd>L</kbd> | says labels can't be edited in a replay |

`--speed N` starts at one of those speeds (`plugii replay f.jsonl --speed
10`). At the end, playback pauses with `end of recording`; jumping back with
<kbd>[</kbd> rebuilds the state from the start, so charts and events stay
right. Every other key works as live: tabs, details, the filter.

A replay shows the labels as they were recorded, and no vendor advice (the
recording may come from another machine). Replay takes plain and `.gz`
files; a cut-off last line or gzip stream (a crash) is skipped, with a
warning in the status line.

The demos on this site are replays too: `examples/demo_recording.rs` builds
a session from the test fixtures (fixed timestamps, redacted) into
`site/demos/daisy-session.jsonl.gz`, and VHS films `plugii replay` playing
it. [Download it](/demos/daisy-session.jsonl.gz) (5 KB) to try the keys:

```sh
plugii replay daisy-session.jsonl.gz --speed 4
```

## The file format

JSON lines. The first line is a header:

```json
{"format":"plugii-record","version":1,"plugii":"0.1.0","host":"daisy","started_at_ms":1760616000000}
```

`format` must be `plugii-record`; `version` is the format version (this
plugii reads `1` and refuses others with a message); `plugii` is the
version that wrote it; `started_at_ms` the first snapshot's time in Unix
milliseconds.

Every line after it has a `t_ms` time and one of:

- **A keyframe**, `{"t_ms":…,"snapshot":{…}}`: a whole snapshot, the same
  object as `plugii --json`'s `snapshot`. Written for the first snapshot,
  whenever anything but the fast-changing fields changes (a device, a link
  speed, a PD contract, a kernel log line), and at least every 10 minutes.
- **A tick**, `{"t_ms":…,"tick":{…}}`: only what changes every second,
  applied onto the last keyframe:

```json
{"t_ms":1760616018000,"tick":{
  "cpu":{"util_pct":22,"avg_khz":1635146,"max_khz":5100000,"min_khz":400000,"package_w":7.73,"package_temp_c":52.5},
  "power":{"ac_online":true,"battery":{"capacity_pct":80,"status":"Not charging","charge_limit_pct":80,"flow_mw":0},
           "usb_c_sources":[{"typec_port":"port0","voltage_mv":null,"current_ma":2750}]},
  "urbs":{"6-1":64,"6-1.2":46,"7-1":68,"7-1.2":5816}}}
```

(One line in the file; wrapped here.) `cpu` is the CPU sample (or `null`),
`power` the whole power state, and `urbs` each device's URB counter by
sysfs name, left out when no device has one. Since keyframes are rare, a
minute of a quiet session is a few kilobytes; the [demo](../../)'s 76 s
recording is 5 KB gzipped.

To pull numbers out of a recording:

```sh
zcat session.jsonl.gz | tail -n +2 | jq -c 'select(.tick) | [.t_ms, .tick.cpu.avg_khz]'
```
