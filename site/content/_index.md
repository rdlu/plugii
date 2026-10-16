---
title: plugii
description: A live USB port monitor for Linux. Which socket, what link speed, USB-C / PD power, a health verdict, and a vim-key TUI.
layout: hextra-home
---

{{< hextra/hero-badge link="https://github.com/rdlu/plugii" >}}
  <div class="hx:w-2 hx:h-2 hx:rounded-full hx:bg-primary-400"></div>
  <span>Linux · Rust · open source</span>
  {{< icon name="arrow-circle-right" attributes="height=14" >}}
{{< /hextra/hero-badge >}}

<div class="plugii-hero">
<div class="hx:mt-6 hx:mb-6">
{{< hextra/hero-headline >}}
  Know what every USB port&nbsp;<br class="hx:sm:block hx:hidden" />is really doing
{{< /hextra/hero-headline >}}
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
{{< hextra/hero-button text="GitHub" link="https://github.com/rdlu/plugii" style="background: transparent; color: inherit; box-shadow: inset 0 0 0 1px currentColor" >}}
</div>



<div class="hx:mt-10"></div>

{{< hextra/hero-section >}}What it shows{{< /hextra/hero-section >}}

<div class="hx:mt-6"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Every socket, both halves"
    icon="desktop-computer"
    subtitle="One card per physical socket, with the USB 2 and USB 3 halves side by side. A hub stuck at USB 2 is red; a device linked below what it can do gets a dim ↓."
  >}}
  {{< hextra/feature-card
    title="Capable vs. linked speed"
    icon="lightning-bolt"
    subtitle="plugii reads each device's descriptors for the fastest speed it *could* link at: “10G capable, linked at 5G → cable or hub limiting”."
  >}}
  {{< hextra/feature-card
    title="USB-C and Power Delivery"
    icon="puzzle"
    subtitle="Partners, cables and alt modes, every offered PDO with the negotiated contract highlighted, lit DisplayPort / HDMI connectors and Thunderbolt / USB4 authorization."
  >}}
  {{< hextra/feature-card
    title="A health verdict"
    icon="badge-check"
    subtitle="The one-shot report ends in a verdict and exits 1 if anything is wrong, so it fits in scripts. `-H` prints only the verdict; `--json` gives the whole snapshot."
  >}}
  {{< hextra/feature-card
    title="A live TUI, vim keys first"
    icon="terminal"
    subtitle="`plugii watch` updates as you swap hubs, cables and chargers: Ports, Charts and Events tabs, power and CPU timelines, URB/s sparklines, and ghosts of unplugged devices."
  >}}
  {{< hextra/feature-card
    title="Record and replay"
    icon="clock"
    subtitle="`--record` writes snapshots as JSON lines (`.gz` and `--redact` too); `plugii replay` plays them back at up to ×60 and jumps between plug events."
  >}}
{{< /hextra/feature-grid >}}

<div class="hx:mt-16"></div>

{{< hextra/hero-section >}}Next{{< /hextra/hero-section >}}

<div class="hx:mt-6"></div>

{{< cards >}}
  {{< card link="/docs/getting-started/" title="Getting started" subtitle="Install plugii and read your first report." icon="arrow-right" >}}
  {{< card link="/docs/labels/" title="Labels file" subtitle="Name your sockets, USB-C ports, firmware and devices." icon="tag" >}}
  {{< card link="/docs/reference/keymap/" title="TUI keymap" subtitle="Every key in plugii watch and replay." icon="key" >}}
{{< /cards >}}
