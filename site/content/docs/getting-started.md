---
title: Getting started
description: Install plugii, run the report and the TUI, and read the exit codes.
weight: 2
---

plugii runs on Linux. It reads sysfs, listens to udev and asks `journalctl -k`
for the kernel log, all as your user: no root, no daemon.

## Install

Each [GitHub release](https://github.com/rdlu/plugii/releases) has binaries
for x86_64 and aarch64 Linux (glibc 2.35 or newer: Ubuntu 22.04, Debian 12,
Fedora 36 and later, Arch). They need libudev, which every systemd
distribution has. These install methods work from the first release,
0.1.0; until it's out, build from source.

{{< tabs >}}
{{< tab name="binary" >}}

Download the tarball for your machine and its checksum, check it, and put
the binary on your `PATH`:

```sh
version=0.1.0
target=$(uname -m)-unknown-linux-gnu   # x86_64 or aarch64
base=https://github.com/rdlu/plugii/releases/download/v$version
curl -fLO "$base/plugii-$version-$target.tar.gz"
curl -fLO "$base/plugii-$version-$target.tar.gz.sha256"
sha256sum -c "plugii-$version-$target.tar.gz.sha256"
tar -xzf "plugii-$version-$target.tar.gz"
install -Dm755 "plugii-$version-$target/plugii" ~/.local/bin/plugii
```

The tarball also has man pages (`man/`) and bash, zsh and fish completions
(`completions/`).

{{< /tab >}}
{{< tab name="mise" >}}

mise's `github` backend installs the release binary for your
architecture:

```sh
mise use -g github:rdlu/plugii
```

{{< /tab >}}
{{< tab name="from source" >}}

With a Rust toolchain (1.88 or newer) and libudev's headers (`libudev-dev`
and `pkg-config` on Debian and Ubuntu, `systemd-devel` on Fedora; Arch has
them already), cargo builds it into `~/.cargo/bin`:

```sh
cargo install --locked --git https://github.com/rdlu/plugii
```

Or from a clone, where the `install` task of the repository's `mise.toml`
runs the same `cargo install`:

```sh
git clone https://github.com/rdlu/plugii
cd plugii
mise run install
```

{{< /tab >}}
{{< /tabs >}}

Optional, picked up when present:

| Tool | What plugii uses it for |
| --- | --- |
| `journalctl` (systemd) | The kernel log section: USB errors and link resets this boot. |
| `fwupdmgr` | With `--firmware`: labelled firmware versions, failed and pending updates. |
| `niri` | The mode and VRR state of each lit display. |

## First run

```sh
plugii
```

The report lists every socket with what's plugged in and at what speed,
then USB-C, Thunderbolt, displays, power and the kernel log, and ends in a
health verdict. [Reading the report](../reading-the-report/) walks through one.
Sockets without a name show as `unlabeled` with their key (for example
`0000:00:14.0-1`); give them names in the [labels file](../labels/).

The other ways to run it:

| Command | What it does |
| --- | --- |
| `plugii -H` | the health verdict only |
| `plugii --json` | the snapshot and the findings as JSON |
| `plugii --firmware` | also check BIOS and PD firmware ([Firmware](../firmware/)); combines with `-H` and `--json` |
| `plugii watch`, `plugii -w` | the live TUI ([The TUI](../tui/)) |
| `plugii watch --record f.jsonl` | the TUI, recording ([Record and replay](../record-replay/)) |
| `plugii replay f.jsonl` | play a recording back |
| `plugii labels` | which labels file plugii reads and what it names |
| `plugii capture <dir>` | copy the sysfs files plugii reads, as a test fixture |

With both `-H` and `--json`, JSON wins. The [CLI reference](../reference/cli/)
has every flag.

In the TUI, press <kbd>?</kbd> for the key list and <kbd>q</kbd> to quit;
the [keymap](../reference/keymap/) has them all.

## Exit codes

The report, `-H` and `--json` set the exit status from the health findings,
so `plugii -H` fits in scripts and status bars:

| Code | Meaning |
| --- | --- |
| `0` | Every finding is ✓. For the other commands: it worked. |
| `1` | At least one finding is ⚠ or ✗, or plugii itself failed (an unreadable labels file, a recording that already exists, a file `replay` can't read). |
| `2` | The command line is wrong: an unknown flag, or flags that conflict (`--refresh` without `--firmware`, `-w` with `-H`). |

`--help` and `--version` exit 0.

```sh
plugii -H >/dev/null || notify-send "USB: something needs a look"
```
