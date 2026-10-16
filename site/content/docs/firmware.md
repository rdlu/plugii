---
title: Firmware
description: plugii --firmware - BIOS, EC and USB-C PD firmware versions against Lenovo's and Dell's catalogs and fwupd, the day-long cache, offline behaviour and esrt labels.
weight: 8
---

Many USB-C problems are fixed by firmware: the BIOS, the embedded
controller, the PD controller. `--firmware` adds a **Firmware** section to
the report and its findings to Health:

```sh
plugii --firmware            # report with a Firmware section
plugii -H --firmware         # health only
plugii --json --firmware     # the snapshot's "firmware" object too
plugii --firmware --refresh  # ignore the cached catalog
```

It is opt-in because it uses the network. The firmware checks run in a
thread alongside the scan, so the report waits only for the slower of the
two.

{{< snapshot name="report__daisy_charger_firmware_report" section="Firmware" title="Lenovo ThinkPad E14 Gen 6, up to date" >}}

{{< snapshot name="report__xps_firmware_update_not_on_lvfs_report" section="Firmware" title="Dell XPS 9320, a BIOS update Dell has and LVFS hasn't yet" >}}

## What it checks

### BIOS and EC

From DMI (`/sys/class/dmi/id`): the BIOS version, release and date, and the
EC firmware release. Always shown, on any machine.

### The vendor's catalog

Picked by DMI `sys_vendor`:

- **Lenovo** (`LENOVO`): the per-model update catalog,
  `https://download.lenovo.com/catalog/<MTM>_Win11.xml`, where the machine
  type (MTM) is the first four characters of the product name (`21M4` for a
  `21M4000RBO`). It gives the newest BIOS build and USB-C PD firmware. LVFS
  doesn't carry these for many ThinkPads, so this is the only source.
  - The BIOS is compared by build number within its family (`R2KET40W` is
    build 40 of `R2K`); a catalog BIOS of another family isn't compared.
  - The PD firmware is compared with each `esrt:` labelled firmware whose
    label contains `PD`: the ESRT version is a plain number, the catalog's
    is hex (`261` is `0x105`).
  - Updates install from Windows: the BIOS from its `.exe` (Lenovo's
    bootable ISO flasher doesn't work on every model), the PD firmware only
    with a Windows installer.
- **Dell** (`Dell Inc.`): `CatalogIndexPC.cab`, then the per-model catalog
  for the system ID (DMI `product_sku`), for the newest BIOS with its
  criticality and date. Dell's catalog gets BIOS releases before LVFS does,
  so the advice depends on whether fwupd has the update yet: `fwupdmgr
  update` if it does, else F12 → BIOS Flash Update with the `.exe` on the
  EFI system partition, or wait.

Other vendors get the DMI and fwupd parts. Adding one is a module under
`src/vendor/`.

### fwupd

When `fwupdmgr` is installed, plugii runs `fwupdmgr get-devices --json`
and `fwupdmgr get-updates --json` (fwupd's cached metadata; this doesn't
download anything):

- the version of each device an [`esrt:` label](../labels/#firmware) names
  (shown as `USB-C PD firmware: 261 (0x105)`),
- updates that failed (✗),
- pending updates (⚠, `fwupdmgr update`). The list is as new as the last
  `fwupdmgr refresh`.

Without fwupd, the section says `fwupd not installed — skipping capsule
firmware checks`.

## Naming firmware with `esrt:`

fwupd lists some capsule firmware only as "UEFI Device Firmware". Find its
GUID in `fwupdmgr get-devices`, then name it:

```text {filename="~/.config/plugii/labels"}
# 0x105 = Lenovo PD firmware 1.0.1.5
esrt:a77b326c-bbf9-4867-af3b-a01f96f274f5  USB-C PD firmware
```

The name is what the Firmware section and the findings show, and on Lenovo
machines a name containing `PD` is compared with the catalog's PD firmware.

## The cache

Catalog answers are cached for a day, one JSON file per vendor and model,
in `$XDG_CACHE_HOME/plugii/` (default `~/.cache/plugii/`): `lenovo-21M4.json`,
`dell-0AF3.json`. Within the day, `--firmware` needs no network for the
catalog. `--refresh` fetches again (and stores the new answer); it requires
`--firmware`.

## Offline

Offline isn't an error. The report says so and goes on, and the verdict
doesn't change because of it:

{{< snapshot name="report__daisy_firmware_offline_report" section="Firmware" title="$ plugii --firmware   (no network)" >}}

A catalog that downloads but can't be read says `can't read Lenovo's
catalog for 21M4 (…)`; a model the catalog doesn't list says `Lenovo's
catalog lists nothing for 21M4`. Requests time out after 8 s (Lenovo) or
15 s (Dell); fwupdmgr gets 20 s, and a fwupdmgr that fails reads as no
devices and no updates.

The findings are listed under [Health checks](../health/#firmware).
