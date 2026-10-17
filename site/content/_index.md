---
title: plugii
description: A live USB port monitor for Linux. Which socket, what link speed, USB-C / PD power, a health verdict, and a vim-key TUI.
layout: hextra-home
# The headline, typed round and round (layouts/_shortcodes/hero-typed.html):
# each line held for `hold` ms. `tagline` is what screen readers, reduced
# motion and no-JS get, and where a return visit picks up.
hero:
  - { text: "plugii: a live USB port monitor for Linux", hold: 4500 }
  - { text: "Know what every USB port is really doing", hold: 5000, tagline: true }
  - { text: "10G capable, linked at 5G. It's the cable.", hold: 2500 }
  - { text: "USB-C: the C is for Could be anything.", hold: 2500 }
---

{{< hextra/hero-badge link="https://github.com/rdlu/plugii" >}}
  <div class="hx:w-2 hx:h-2 hx:bg-primary-400"></div>
  <span>Linux · Rust · MIT</span>
  {{< icon name="arrow-circle-right" attributes="height=14" >}}
{{< /hextra/hero-badge >}}

<div class="plugii-hero">
<div class="hx:mt-6 hx:mb-6">
{{< hero-typed >}}
</div>

<div class="hx:mb-10">
{{< hextra/hero-subtitle >}}
  plugii is a live USB port monitor for Linux. For every physical socket it
  shows what is plugged in, at what link speed, and with what USB-C power,
  so a port that has quietly fallen back to USB&nbsp;2 stands out.
{{< /hextra/hero-subtitle >}}
</div>
</div>

<div class="plugii-actions">
{{< hextra/hero-button text="Get started" link="/docs/getting-started/" >}}
{{< hextra/hero-button text="GitHub" link="https://github.com/rdlu/plugii" >}}
</div>

{{< demo name="ports" title="plugii replay daisy-session.jsonl.gz --speed 2" speed="×2" alt="plugii replaying a recorded session on the Ports tab: a dock plugged into the hinge USB-C port comes up with its hub stuck at USB 2 and a red health finding, then links at 5G; a device's details popup, the filter, the help overlay, and the dock's devices left as ghosts after the unplug." caption="A recorded session, replayed: the dock's hub first comes up at USB 2 only, then at 5G. [More on the TUI](docs/tui/)." >}}

<div class="hx:mt-12"></div>

{{< hextra/hero-section >}}What it shows{{< /hextra/hero-section >}}

<div class="hx:mt-6"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Every socket, both halves"
    icon="view-grid"
    subtitle="One card per physical socket, with its USB 3 and USB 2 halves side by side and the device tree under each. A USB 3 hub that only came up on the USB 2 half shows in red."
  >}}
  {{< hextra/feature-card
    title="Capable vs. linked"
    icon="lightning-bolt"
    subtitle="plugii reads each device's BOS descriptor for the fastest speed it *could* link at: “10G capable, linked at 5G → cable or hub limiting”. In the tree, a dim `↓` marks it."
  >}}
  {{< hextra/feature-card
    title="USB-C and Power Delivery"
    icon="puzzle"
    subtitle="UCSI partners, cables and alt modes, every source PDO a charger offers with the negotiated one marked, lit DisplayPort / HDMI connectors and Thunderbolt / USB4 authorization."
  >}}
  {{< hextra/feature-card
    title="A health verdict"
    icon="badge-check"
    subtitle="The report ends in ✓ / ⚠ / ✗ findings with what to do, and exits 1 if anything needs a look. `-H` prints only the verdict; `--json` gives the whole snapshot."
  >}}
  {{< hextra/feature-card
    title="A live TUI, vim keys first"
    icon="terminal"
    subtitle="`plugii watch`: Ports, Charts and Events tabs, details on `⏎`, a `/` filter, ghosts of unplugged devices, power and CPU timelines, link-speed strips, URB/s and a charger table."
  >}}
  {{< hextra/feature-card
    title="Record and replay"
    icon="clock"
    subtitle="`watch --record` writes the session as JSON lines (`.gz` and `--redact` too); `plugii replay` plays it back in the same TUI at up to ×60 and jumps between plugs."
  >}}
  {{< hextra/feature-card
    title="Firmware checks"
    icon="chip"
    subtitle="`--firmware` compares the BIOS and USB-C PD firmware with Lenovo's and Dell's catalogs and asks fwupd for pending and failed updates. Cached for a day; offline is fine."
  >}}
  {{< hextra/feature-card
    title="Names for sockets"
    icon="tag"
    subtitle="The kernel knows `0000:06:00.4-1`, not “the port by the caps lock key”. A small labels file, or `L` in the TUI, names sockets, USB-C ports and devices."
  >}}
  {{< hextra/feature-card
    title="No root, no daemon"
    icon="shield-check"
    subtitle="It reads sysfs, udev and `journalctl -k` as your user; where RAPL energy is root-only, it leaves the package watts out. One binary; the TUI rescans on udev events and once a second."
  >}}
{{< /hextra/feature-grid >}}

<div class="hx:mt-16"></div>

{{< hextra/hero-section >}}Start here{{< /hextra/hero-section >}}

<div class="hx:mt-6"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Getting started"
    icon="arrow-right"
    link="docs/getting-started/"
    subtitle="Install plugii, run the report and the TUI, and read the exit codes."
  >}}
  {{< hextra/feature-card
    title="Reading the report"
    icon="document-text"
    link="docs/reading-the-report/"
    subtitle="A real report, section by section."
  >}}
  {{< hextra/feature-card
    title="Health checks"
    icon="badge-check"
    link="docs/health/"
    subtitle="Every finding plugii can give, and what to do about it."
  >}}
{{< /hextra/feature-grid >}}
