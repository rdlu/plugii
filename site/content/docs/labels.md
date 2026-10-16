---
title: Labels file
description: Name sockets, USB-C ports, firmware and devices, and pair USB 2 / USB 3 halves by hand.
weight: 3
---

{{< callout type="info" icon="light-bulb" >}}
**Why labels?** The kernel can't tell which socket on the chassis a root port
is. It knows `0000:00:14.0-1`, not "the USB-C port by the caps lock key". ACPI
has a location hint, but it is often wrong (on the Dell XPS 9320 both sockets
say "left"), so plugii uses it only for unlabelled sockets and never over a
label.
{{< /callout >}}

## Where the file lives

{{< filetree/container >}}
  {{< filetree/folder name="$XDG_CONFIG_HOME/ (default ~/.config/)" >}}
    {{< filetree/folder name="plugii" >}}
      {{< filetree/file name="labels" >}}
    {{< /filetree/folder >}}
  {{< /filetree/folder >}}
{{< /filetree/container >}}

`$XDG_CONFIG_HOME/plugii/labels`, which is `~/.config/plugii/labels` when
`XDG_CONFIG_HOME` is unset. Run `plugii labels` to see which file plugii
reads and what it names.

## Format

One entry per line. Blank lines and `#` comments are ignored, and the last
line for a key wins.

```text {filename="~/.config/plugii/labels"}
# <pci-address>-<port>  <label>
0000:06:00.4-1  USB-C (caps lock)

# typec:<portN>  <label>  (a UCSI port; same label as its socket)
typec:port0  USB-C (caps lock)

# esrt:<guid>  <label>  (firmware fwupd lists only as "UEFI Device Firmware")
esrt:a77b326c-...  USB-C PD firmware

# peer:<usb2-key>  <usb3-key>  (pair the halves where firmware has no peer link)
peer:0000:00:14.0-1  0000:00:0d.0-1

# device:<vid>:<pid>  <name>  (every device with these ids)
device:05e3:0610  USB switch (desk)
```

## Kinds of line

### Sockets

Key: **`<pci-address>-<port>`**

Plug something in and read the unlabelled line it appears on. The key is the
USB 2 root port's `<pci-address>-<port>`.

### USB-C ports

Key: **`typec:portN`**

Plug something into the socket, see which `portN` gains a partner, and give
it the socket's label. That ties the UCSI port to the socket, so a device
that carries no USB data (a monitor in DisplayPort alt mode) still shows on it.

### Firmware

Key: **`esrt:<guid>`**

An ESRT GUID gets a readable name the same way, for firmware that fwupd lists
only as "UEFI Device Firmware".

### Peers

Key: **`peer:<usb2-key> <usb3-key>`**

The USB 2 and USB 3 halves of a socket are paired by the kernel's `peer` link
when firmware describes it. Where it doesn't (the Dell XPS 9320's Thunderbolt
ports, for one), pair them by hand: plug a USB 3 device in; the USB 3 key is
its SuperSpeed root port's `<pci-address>-<port>`, which on Intel matches the
USB 2 one by port number.

### Devices

Key: **`device:<vid>:<pid>`**

A name for a vid:pid, shown in place of the product string in the report and
the TUI. A labelled hub coming or going is one event:

```text
− USB switch (desk): 6 devices left
```

## Label from the TUI

You don't have to edit the file by hand. In `plugii watch`:

{{% steps %}}

### Pick the row

Open the **Ports** tab (<kbd>1</kbd>) and move to the socket or device with
<kbd>j</kbd> / <kbd>k</kbd>.

### Name it

Press <kbd>L</kbd> and type the name.

### Save or cancel

Press <kbd>⏎</kbd> to save it, or <kbd>Esc</kbd> to cancel. An empty name
removes the key's lines.

{{% /steps %}}

plugii writes the change to `~/.config/plugii/labels`, creating it if needed.
The last line for that key is replaced in place, else a line is appended;
comments and order are kept. Relabelling a socket renames the `typec:` line
that carried its old label too.

> [!CAUTION]
> In `plugii replay`, <kbd>L</kbd> only tells you labels can't be edited in a
> replay.
