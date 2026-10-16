---
title: Getting started
description: Install plugii, run the report and the TUI, and read the exit codes.
weight: 2
---

plugii runs on Linux and reads everything from sysfs; the report and the TUI
don't need root. It is built from source for now, so you need a Rust toolchain.

## Install

Clone the repository, then install the `plugii` binary into `~/.cargo/bin`:

{{< tabs >}}
{{< tab name="cargo" >}}

```sh
git clone https://github.com/rdlu/plugii
cd plugii
cargo install --locked --path .
```

{{< /tab >}}
{{< tab name="mise" >}}

The repository's `mise.toml` has an `install` task that runs the same `cargo install`:

```sh
git clone https://github.com/rdlu/plugii
cd plugii
mise run install
```

{{< /tab >}}
{{< /tabs >}}

## First run

```sh
plugii       # full report + health verdict
plugii -H    # health verdict only
plugii -w    # the live TUI (same as plugii watch)
```

The report lists every socket with what is plugged in and the link speed,
then USB-C / PD, power, and the health findings. Unnamed sockets show their
key (for example `0000:00:14.0-1`): give them names in the
[labels file](/docs/labels/).

In the TUI, press <kbd>?</kbd> for help and <kbd>q</kbd> to quit. The
[keymap](/docs/reference/keymap/) lists every key.

Other commands:

| Command | What it does |
| --- | --- |
| `plugii --json` | the snapshot and findings as JSON |
| `plugii labels` | list what the labels file names |
| `plugii capture <dir>` | copy the sysfs files plugii reads, as a test fixture |
| `plugii watch --record f.jsonl` | record snapshots while watching (`f.jsonl.gz` compresses) |
| `plugii replay f.jsonl` | play a recording back |

> [!NOTE]
> **Planned:** `plugii --firmware` (BIOS and PD firmware versions, from the
> network) isn't done yet.

## Exit codes

The report, `-H` and `--json` set the exit status from the health findings,
so `plugii -H` works in scripts and status bars.

| Code | Meaning |
| --- | --- |
| `0` | Every finding is OK. |
| `1` | At least one finding is a warning or a failure, or plugii itself failed (for example, an unreadable labels file). |
| `2` | The command line is wrong (an unknown flag, or flags that conflict). |
