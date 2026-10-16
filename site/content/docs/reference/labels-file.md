---
title: Labels file format
linkTitle: Labels file
description: The exact format of ~/.config/plugii/labels - keys, values, comments and which line wins.
weight: 13
---

Path: `$XDG_CONFIG_HOME/plugii/labels`, else `~/.config/plugii/labels`. An
`XDG_CONFIG_HOME` that is empty or not absolute is ignored. A missing file
is no labels; a file that exists but can't be read is an error (exit 1).
How to use it: [Labels](../../labels/).

## Lines

One entry per line, read like bash's `read -r key text`:

- The **key** is the first word (split on spaces and tabs); the **value**
  is the rest of the line, trimmed.
- Blank lines and lines whose first word starts with `#` are skipped.
  There are no trailing comments: `#` after a key is part of the value.
- The **last** line for a key wins. A key with an empty value unsets it.

## Keys

| Key | Value | Names |
| --- | --- | --- |
| `<pci-address>-<port>` | label | A socket, by its USB 2 root port: `0000:06:00.4-1`. |
| `typec:<portN>` | label | A UCSI port (`port0`). Give it the same label as its socket to tie the two. |
| `device:<vid>:<pid>` | name | Every USB device with these ids. Exactly four hex digits each, either case; a malformed one is skipped. |
| `esrt:<guid>` | label | Capsule firmware, by its ESRT GUID as fwupd lists it. |
| `peer:<usb2-key>` | `<usb3-key>` | The USB 3 root port paired with a USB 2 socket key, where firmware has no `peer` link. Only the first word of the value is used. |

```text
# daisy, a ThinkPad E14 Gen 6
0000:06:00.3-1  USB-C (hinge)
0000:06:00.4-1  USB-C (caps lock)
0000:05:00.4-1  USB-A (right)
0000:05:00.4-2  USB-A (left)

typec:port0  USB-C (caps lock)
esrt:a77b326c-bbf9-4867-af3b-a01f96f274f5  USB-C PD firmware
device:05e3:0610  USB switch (desk)
```

## How labels are used

- A socket's label replaces `unlabeled` in the report and its key in the
  TUI, and names it in findings, events and charts.
- The UCSI port whose `typec:` label equals a socket's label is that
  socket's USB-C port. When several do, the first by port name wins.
- A `device:` name replaces the product string everywhere a device is
  shown, and a labelled hub with devices below it leaves or arrives as one
  event.
- A `peer:` line is used only when the kernel has no `peer` link for that
  port.

## How plugii writes it

The TUI's <kbd>L</kbd> edits one key at a time: the last line for the key
is replaced in place as `<key>  <value>` (keeping that line's ending), or
appended at the end if there is none; an empty value removes every line for
the key. Line breaks typed into a value become spaces. Comments, blank
lines, line endings and the order of the other lines are kept; the file's
permissions are kept, and the new text goes to a temporary file renamed
over the old one. If the path is a symlink, the target is written.
