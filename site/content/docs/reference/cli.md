---
title: Command line
linkTitle: CLI
description: Every plugii command and flag, from clap's own --help output.
weight: 12
---

The blocks below are `plugii … --help` exactly as clap prints it: a test
renders each one into an insta snapshot, and this page shows those
snapshots, so it changes when the code does. [Getting started](../../getting-started/)
has the exit codes.

## plugii

The report. With no flags, the full report and health verdict; `-H` the
verdict only; `--json` the snapshot and findings as JSON (and wins over
`-H`); `--firmware` adds the firmware checks to any of them.

{{< snapshot name="cli__help_plugii" title="$ plugii --help" >}}

`--refresh` needs `--firmware`; `-w` can't combine with the report flags;
flags and a command can't be mixed. Each of those is a usage error (exit 2).

## plugii watch

The live TUI; see [The TUI](../../tui/) and, for the recording flags,
[Record and replay](../../record-replay/).

{{< snapshot name="cli__help_watch" title="$ plugii watch --help" >}}

## plugii replay

{{< snapshot name="cli__help_replay" title="$ plugii replay --help" >}}

Any other `--speed` is a usage error.

## plugii labels

Prints the labels file's path and what it names, by kind; see
[Labels](../../labels/).

{{< snapshot name="cli__help_labels" title="$ plugii labels --help" >}}

## plugii capture

Copies the sysfs and procfs files plugii reads into `DIR` (which must not
exist yet), laid out as under `/`, so plugii can read the copy as if it were
the machine; a `CAPTURE.txt` notes the date, kernel and model. Serial
numbers and unique IDs are replaced with `REDACTED`, since captures end up
in a public repository: contributors use it to turn a machine's odd state
into a fixture under `tests/fixtures/sysfs/`.

{{< snapshot name="cli__help_capture" title="$ plugii capture --help" >}}
