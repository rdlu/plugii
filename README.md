# plugii

A live USB port monitor for Linux. For every physical socket on the machine it
shows what is plugged in and at what link speed, with the USB 2 and USB 3
halves side by side, so a port that has silently fallen back to USB 2 stands
out. It also shows the USB-C / Power Delivery state (partners, offered and
negotiated power), lit DisplayPort / HDMI connectors, Thunderbolt / USB4
authorization, and what the power source lets the CPU do. A one-shot CLI report
ends in a health verdict; a ratatui TUI updates live while you swap hubs,
cables, chargers and devices.

**Status: early development.** Only the labels file is read so far; `plugii`
prints a summary of it.

## Planned commands

```text
plugii                        full report + health; exit 1 if anything is wrong
plugii -H                     health verdict only
plugii --json                 the snapshot as JSON
plugii --firmware             also check BIOS / PD firmware versions (network)
plugii watch                  live TUI (also: plugii -w)
plugii watch --record f.jsonl record snapshots while watching
plugii replay f.jsonl         play a recording back
plugii capture <dir>          copy the sysfs files plugii reads, as a test fixture
```

## Labels

The kernel can't tell which socket on the chassis a root port is, so names
come from a labels file: `$XDG_CONFIG_HOME/plugii/labels` (default
`~/.config/plugii/labels`), falling back to `~/.config/usb-ports/labels`. One
entry per line; blank lines and `#` comments are ignored, and the last line for
a key wins.

```text
# <pci-address>-<port>  <label>
0000:06:00.4-1  USB-C (caps lock)

# typec:<portN>  <label>  (a UCSI port; same label as its socket)
typec:port0  USB-C (caps lock)

# esrt:<guid>  <label>  (firmware fwupd lists only as "UEFI Device Firmware")
esrt:a77b326c-...  USB-C PD firmware

# peer:<usb2-key>  <usb3-key>  (pair the halves where firmware has no peer link)
peer:0000:00:14.0-1  0000:00:0d.0-1
```

- **Sockets:** find the key by plugging something in and reading the unlabeled
  line it appears on. The key is the USB 2 root port's
  `<pci-address>-<port>`.
- **USB-C ports:** plug something into the socket, see which `portN` gains a
  partner, and give it the socket's label. That ties the UCSI port to the
  socket, so a device that carries no USB data (a monitor in DP alt mode)
  still shows on it.
- **Firmware:** an ESRT GUID gets a readable name the same way.
- **Peers:** the USB 2 and USB 3 halves of a socket are paired by the kernel's
  `peer` link when firmware describes it. Where it doesn't (the Dell XPS
  9320's Thunderbolt ports, for one), pair them by hand: plug a USB 3 device
  in; the USB 3 key is its SuperSpeed root port's `<pci-address>-<port>`,
  which on Intel matches the USB 2 one by port number.

## License

MIT, see [LICENSE](LICENSE).
