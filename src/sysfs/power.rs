//! AC, battery and USB-C power supplies from `sys/class/power_supply`.
//!
//! Supplies are found by their `type`, not their name: `AC` / `ACAD` /
//! `ADP1` are all `Mains`, and the system battery isn't always `BAT0`.

use std::path::{Path, PathBuf};

use super::typec::{self, UcsiSupply};
use super::{Root, entries, read as attr, read_parse};
use crate::model::{Battery, Power, UsbCSource};

pub fn read(root: &Root) -> Power {
    let supplies = entries(root.path("sys/class/power_supply"), |_| true);
    let of_type = |kind: &str| -> Vec<&PathBuf> {
        supplies
            .iter()
            .filter(|dir| attr(dir.join("type")).as_deref() == Some(kind))
            .collect()
    };

    let mains = of_type("Mains");
    let ac_online = (!mains.is_empty()).then(|| {
        mains
            .iter()
            .any(|dir| attr(dir.join("online")).as_deref() == Some("1"))
    });

    // Peripheral batteries (mice, controllers) have scope Device.
    let battery = of_type("Battery")
        .into_iter()
        .find(|dir| attr(dir.join("scope")).as_deref() != Some("Device"))
        .map(|dir| battery(dir));

    let usb_c_sources = typec::ucsi_supplies(root)
        .iter()
        .filter(|s| s.online)
        .map(usb_c_source)
        .collect();

    Power {
        ac_online,
        battery,
        usb_c_sources,
    }
}

fn battery(dir: &Path) -> Battery {
    let status = attr(dir.join("status"));
    Battery {
        capacity_pct: read_parse(dir.join("capacity")),
        charge_limit_pct: read_parse(dir.join("charge_control_end_threshold")),
        flow_mw: flow_mw(dir, status.as_deref()),
        status,
    }
}

/// Power into the battery in mW: `power_now` (µW) where the driver has it,
/// else `current_now` (µA) × `voltage_now` (µV). Drivers disagree on the
/// sign, so like the script the magnitude is read and `status` sets the
/// sign; any status but Charging / Discharging gives `None`.
fn flow_mw(dir: &Path, status: Option<&str>) -> Option<i64> {
    let magnitude_uw = match read_parse::<i64>(dir.join("power_now")) {
        Some(uw) => uw.unsigned_abs(),
        None => {
            let ua = read_parse::<i64>(dir.join("current_now"))?.unsigned_abs();
            let uv = read_parse::<u64>(dir.join("voltage_now"))?;
            ua.checked_mul(uv)? / 1_000_000
        }
    };
    let mw = i64::try_from(magnitude_uw / 1000).ok()?;
    match status? {
        "Charging" => Some(mw),
        "Discharging" => Some(-mw),
        _ => None,
    }
}

/// An online UCSI supply, under the typec port [`typec::ucsi_supplies`]
/// pairs it with: the same pairing the typec reader uses.
fn usb_c_source(supply: &UcsiSupply) -> UsbCSource {
    let dir = &supply.dir;
    UsbCSource {
        typec_port: supply.port.clone(),
        voltage_mv: read_parse::<u32>(dir.join("voltage_now")).map(|uv| uv / 1000),
        current_ma: read_parse::<u32>(dir.join("current_max")).map(|ua| ua / 1000),
    }
}
