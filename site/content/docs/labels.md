---
title: Labels
description: Name sockets, USB-C ports, firmware and devices, from the TUI with L or by hand, and pair USB 2 / USB 3 halves where firmware doesn't.
weight: 6
---

{{< callout type="info" icon="light-bulb" >}}
**Why labels?** The kernel can't tell which socket on the chassis a root port
is. It knows `0000:00:14.0-1`, not "the USB-C port by the caps lock key".
ACPI has a location hint, but it's often wrong (on the Dell XPS 9320 both
sockets say "left"), so plugii only shows it in a socket's details and
never uses it over a label.
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
`XDG_CONFIG_HOME` is unset (or not an absolute path). That's the only place
plugii looks. No file means no labels, which is fine: sockets show as
`unlabeled` with their key. `plugii labels` prints which file it read and
what it names:

```text
labels from /home/you/.config/plugii/labels
sockets:
  0000:05:00.4-1  USB-A (right)
  0000:05:00.4-2  USB-A (left)
  0000:06:00.3-1  USB-C (hinge)
  0000:06:00.4-1  USB-C (caps lock)  (USB-C: port0)
firmware (ESRT):
  a77b326c-bbf9-4867-af3b-a01f96f274f5  USB-C PD firmware
```

## What you can name

```text {filename="~/.config/plugii/labels"}
# A socket: its USB 2 root port's <pci-address>-<port>
0000:06:00.4-1  USB-C (caps lock)

# Its UCSI port, with the same label as the socket
typec:port0  USB-C (caps lock)

# Every device with this vendor:product id
device:05e3:0610  USB switch (desk)

# Firmware that fwupd only calls "UEFI Device Firmware"
esrt:a77b326c-bbf9-4867-af3b-a01f96f274f5  USB-C PD firmware

# The USB 3 half of a socket, where firmware has no peer link
peer:0000:00:14.0-1  0000:00:0d.0-1
```

The [labels file reference](../reference/labels-file/) has the exact
format. How to find each key:

### Sockets

Plug something into the socket and run `plugii`: the `unlabeled` block that
gains a device has the key next to it. Or press <kbd>L</kbd> on that socket
in the TUI.

### USB-C ports

Plug something into the socket and see which `portN` gains a partner in the
USB-C section; give it the socket's label. The same label is what ties the
UCSI port to the socket, so a partner that carries no USB data (a monitor in
DisplayPort alt mode, a bare charger) still shows on the socket, and
findings name the port by it.

### Devices

`device:<vid>:<pid>` names every device with those ids, in place of its
product string: in the report's trees, the TUI, the detail view and the
events. Useful for hubs and adapters with generic names (`USB2.1 Hub`). A
labelled hub coming or going is one event instead of one per device:

```text
− USB switch (desk): 6 devices left
```

### Firmware

`esrt:<guid>` gives capsule firmware a readable name in the
[Firmware](../firmware/) section. The GUID is in `fwupdmgr get-devices`. A
label containing `PD` on a Lenovo machine is also compared with Lenovo's
newest PD firmware.

### Peers

The USB 2 and USB 3 halves of a socket are paired by the kernel's `peer`
link when the firmware describes it. Where it doesn't (the Dell XPS 9320's
Thunderbolt ports, for one), the report says `USB 3 half: unknown`; pair
them by hand. Plug a USB 3 device in: the USB 3 key is the `<pci-address>-<port>`
of the SuperSpeed root port it appears under (in `/sys/bus/usb/devices`,
or with `lsusb -t`), which on Intel matches the USB 2 one by port number.

## Label from the TUI

You don't have to edit the file. In `plugii watch`:

{{% steps %}}

### Pick the row

On the **Ports** tab (<kbd>1</kbd>), move to the socket or the device with
<kbd>j</kbd> / <kbd>k</kbd>. A device row names its vid:pid; any other row
of a card (its title, a half, the USB-C line) names the socket.

### Name it

Press <kbd>L</kbd>. The prompt says what you're naming and starts with the
current label:

```text
label device 0b95:1790 (AX88179B): _
```

### Save or cancel

<kbd>⏎</kbd> saves, <kbd>Esc</kbd> or <kbd>Ctrl-c</kbd> cancels. An
empty name removes the label.

{{% /steps %}}

plugii writes `~/.config/plugii/labels` (creating it and its directory if
needed), and the new name shows at once. The last line for that key is
replaced in place, else a line is appended; comments, blank lines and the
order of everything else are kept, and the file is replaced atomically. A
symlinked labels file (dotfiles) is followed and its target written.
Relabelling a socket also renames the `typec:` line that carried its old
label, so the USB-C port stays tied to it. The status line says what was
written, or why it couldn't be.

> [!CAUTION]
> In `plugii replay`, <kbd>L</kbd> only says that labels can't be edited in
> a replay: a recording shows the labels as they were when it was made.
