//! USB-C ports as UCSI registers them: partners, cables, alternate modes, the
//! partner's PD source/sink capabilities and the negotiated contract.
//!
//! The contract comes from UCSI's per-connector power supply,
//! `ucsi-source-psy-<ucsi-device><connector>` (`ucsi-source-psy-USBC000:001`
//! for connector 1 of `USBC000:00`), whose `online` means "this connector is
//! sinking power".

use std::fs;
use std::path::{Path, PathBuf};

use super::{Root, entries, read as attr, read_parse};
use crate::labels::Labels;
use crate::model::{Partner, PdContract, Pdo, TypecPort, TypecState};

pub fn read(root: &Root, labels: &Labels) -> TypecState {
    let mut ports: Vec<(u32, PathBuf)> =
        entries(root.path("sys/class/typec"), |n| port_index(n).is_some())
            .into_iter()
            .filter_map(|dir| Some((port_index(dir.file_name()?.to_str()?)?, dir)))
            .collect();
    ports.sort_by_key(|(i, _)| *i);

    let supplies = supplies(root);
    let mut out: Vec<TypecPort> = ports
        .iter()
        .map(|(_, dir)| read_port(dir, labels))
        .collect();
    let assigned = assign_supplies(&ports, &out, &supplies);
    for (port, psy) in out.iter_mut().zip(assigned) {
        let Some(psy) = psy else { continue };
        port.charging = psy.online;
        port.pd_contract = psy.contract().map(|mut c| {
            c.source_pdo = port
                .partner
                .as_ref()
                .and_then(|p| active_pdo(&p.source_pdos, c.voltage_mv));
            c
        });
    }
    TypecState {
        ucsi_present: !out.is_empty(),
        ports: out,
    }
}

/// `port3` → 3; `port0-partner`, `port0.1` → `None`.
fn port_index(name: &str) -> Option<u32> {
    let n = name.strip_prefix("port")?;
    n.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| n.parse().ok())?
}

fn read_port(dir: &Path, labels: &Labels) -> TypecPort {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partner_dir = dir.join(format!("{name}-partner"));
    let cable_dir = dir.join(format!("{name}-cable"));
    TypecPort {
        label: labels.typec_label(&name).map(str::to_owned),
        partner: partner_dir.is_dir().then(|| read_partner(&partner_dir)),
        cable_type: cable_dir
            .is_dir()
            .then(|| attr(cable_dir.join("type")))
            .flatten(),
        power_role: attr(dir.join("power_role")).and_then(|r| selected(&r)),
        charging: false,
        pd_contract: None,
        name,
    }
}

/// The bracketed choice in `source [sink]`; a bare single value as is.
fn selected(choices: &str) -> Option<String> {
    if let Some((_, rest)) = choices.split_once('[') {
        return Some(rest.split_once(']')?.0.to_owned());
    }
    let mut words = choices.split_whitespace();
    let only = words.next()?;
    words.next().is_none().then(|| only.to_owned())
}

fn read_partner(dir: &Path) -> Partner {
    let pd = dir.join("usb_power_delivery");
    Partner {
        supports_pd: attr(dir.join("supports_usb_power_delivery")).and_then(|s| match s.as_str() {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        }),
        // UCSI reports 0.0 when it doesn't know the partner's revision.
        pd_revision: attr(dir.join("usb_power_delivery_revision")).filter(|r| r != "0.0"),
        alt_modes: read_parse(dir.join("number_of_alternate_modes")).unwrap_or(0),
        source_pdos: pdos(&pd.join("source-capabilities")),
        sink_pdos: pdos(&pd.join("sink-capabilities")),
    }
}

/// `<n>:<kind>` capability directories, by n.
fn pdos(dir: &Path) -> Vec<Pdo> {
    let index = |name: &str| -> Option<u32> { name.split_once(':')?.0.parse().ok() };
    let mut found: Vec<(u32, PathBuf)> = entries(dir, |n| index(n).is_some())
        .into_iter()
        .filter_map(|p| Some((index(p.file_name()?.to_str()?)?, p)))
        .collect();
    found.sort_by_key(|(n, _)| *n);
    found.iter().map(|(_, p)| pdo(p)).collect()
}

fn pdo(dir: &Path) -> Pdo {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kind = name.split_once(':').map_or("", |(_, k)| k);
    // Source PDOs say maximum_*, sink PDOs operational_* (except PPS).
    let value = |names: &[&str], unit: &str| {
        names
            .iter()
            .find_map(|n| with_unit(&attr(dir.join(n))?, unit))
    };
    let mv = |n: &str| value(&[n], "mV");
    let ma = || value(&["maximum_current", "operational_current"], "mA");
    let decoded = match kind {
        "fixed_supply" => (|| {
            Some(Pdo::Fixed {
                mv: mv("voltage")?,
                ma: ma()?,
            })
        })(),
        "variable_supply" => (|| {
            Some(Pdo::Variable {
                min_mv: mv("minimum_voltage")?,
                max_mv: mv("maximum_voltage")?,
                ma: ma()?,
            })
        })(),
        "battery" => (|| {
            Some(Pdo::Battery {
                min_mv: mv("minimum_voltage")?,
                max_mv: mv("maximum_voltage")?,
                mw: value(&["maximum_power", "operational_power"], "mW")?,
            })
        })(),
        "programmable_supply" => (|| {
            Some(Pdo::Pps {
                min_mv: mv("minimum_voltage")?,
                max_mv: mv("maximum_voltage")?,
                ma: ma()?,
            })
        })(),
        _ => None,
    };
    decoded.unwrap_or(Pdo::Other(name))
}

/// `20000mV` → 20000 (the unit is optional).
fn with_unit(text: &str, unit: &str) -> Option<u32> {
    let text = text.trim();
    text.strip_suffix(unit).unwrap_or(text).trim().parse().ok()
}

/// The 1-based position of the one fixed source PDO at the contract voltage.
fn active_pdo(source: &[Pdo], voltage_mv: u32) -> Option<u8> {
    let mut matches = source
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p, Pdo::Fixed { mv, .. } if *mv == voltage_mv));
    let (i, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    u8::try_from(i + 1).ok()
}

/// A `ucsi-source-psy-*` power supply.
#[derive(Debug, Clone)]
struct Supply {
    /// The UCSI device it belongs to (`USBC000:00`) when it can be told.
    ucsi: Option<String>,
    /// The 1-based UCSI connector number.
    connector: u32,
    online: bool,
    voltage_uv: Option<u64>,
    current_max_ua: Option<u64>,
}

impl Supply {
    fn contract(&self) -> Option<PdContract> {
        if !self.online {
            return None;
        }
        let mv = u32::try_from(self.voltage_uv? / 1000).ok()?;
        let ma = u32::try_from(self.current_max_ua? / 1000).ok()?;
        (mv > 0 && ma > 0).then_some(PdContract {
            voltage_mv: mv,
            current_ma: ma,
            source_pdo: None,
        })
    }
}

const PSY_PREFIX: &str = "ucsi-source-psy-";

fn supplies(root: &Root) -> Vec<Supply> {
    let mut out: Vec<Supply> = entries(root.path("sys/class/power_supply"), |n| {
        n.starts_with(PSY_PREFIX)
    })
    .into_iter()
    .filter_map(|dir| {
        let name = dir.file_name()?.to_str()?.to_owned();
        let ucsi = parent_device(&dir);
        Some(Supply {
            connector: connector_number(&name, ucsi.as_deref())?,
            ucsi,
            online: attr(dir.join("online")).as_deref() == Some("1"),
            voltage_uv: read_parse(dir.join("voltage_now")),
            current_max_ua: read_parse(dir.join("current_max")),
        })
    })
    .collect();
    out.sort_by_key(|s| s.connector);
    out
}

/// The connector number at the end of a UCSI supply's name. With the UCSI
/// device name known it is whatever follows it; otherwise the device name is
/// taken to be ACPI style, `<HID>:<2-digit instance>`.
fn connector_number(name: &str, ucsi: Option<&str>) -> Option<u32> {
    let rest = name.strip_prefix(PSY_PREFIX)?;
    if let Some(n) = ucsi.and_then(|u| rest.strip_prefix(u)) {
        return n.parse().ok();
    }
    let (_, tail) = rest.rsplit_once(':')?;
    tail.get(2..)?.parse().ok()
}

/// The device a class device hangs off: `…/USBC000:00/power_supply/<psy>` and
/// `…/USBC000:00/typec/port0` both give `USBC000:00`.
fn parent_device(class_dev: &Path) -> Option<String> {
    let real = fs::canonicalize(class_dev).ok()?;
    Some(real.parent()?.parent()?.file_name()?.to_str()?.to_owned())
}

/// Which supply belongs to which port.
///
/// UCSI numbers connectors from 1 and typec ports from 0, so `portN` should be
/// connector N+1 of the same UCSI device. That numbering isn't guaranteed (the
/// port index comes from a global allocator), and it has been seen to be wrong
/// after UCSI re-registered its ports: the XPS 9320 had `port0` sinking from
/// the dock while connector 2's supply was the online one. A supply is online
/// exactly when its connector sinks power, so when the numbering contradicts
/// the ports' own power roles and the sinking ports and online supplies pair
/// up one to one, they are paired instead.
fn assign_supplies<'a>(
    ports: &[(u32, PathBuf)],
    read: &[TypecPort],
    supplies: &'a [Supply],
) -> Vec<Option<&'a Supply>> {
    let numbered: Vec<Option<&Supply>> = ports
        .iter()
        .map(|(index, dir)| {
            let ucsi = parent_device(dir);
            supplies.iter().find(|s| {
                s.connector == index + 1 && (s.ucsi.is_none() || ucsi.is_none() || s.ucsi == ucsi)
            })
        })
        .collect();
    let sinking: Vec<bool> = read
        .iter()
        .map(|p| p.partner.is_some() && p.power_role.as_deref() == Some("sink"))
        .collect();
    let roles_known = read
        .iter()
        .all(|p| p.partner.is_none() || p.power_role.is_some());
    let consistent = numbered
        .iter()
        .zip(&sinking)
        .all(|(s, sink)| s.is_none_or(|s| s.online == *sink));
    if consistent || !roles_known {
        return numbered;
    }
    let online: Vec<&Supply> = supplies.iter().filter(|s| s.online).collect();
    if online.len() != sinking.iter().filter(|s| **s).count() {
        return numbered;
    }
    let mut online = online.into_iter();
    let offline: Vec<&Supply> = supplies.iter().filter(|s| !s.online).collect();
    let mut offline = offline.into_iter();
    sinking
        .iter()
        .map(|sink| if *sink { online.next() } else { offline.next() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_names() {
        assert_eq!(port_index("port0"), Some(0));
        assert_eq!(port_index("port12"), Some(12));
        assert_eq!(port_index("port0-partner"), None);
        assert_eq!(port_index("port0.1"), None);
        assert_eq!(port_index("port"), None);
    }

    #[test]
    fn selected_role() {
        assert_eq!(selected("source [sink]").as_deref(), Some("sink"));
        assert_eq!(selected("[source] sink").as_deref(), Some("source"));
        assert_eq!(selected("sink").as_deref(), Some("sink"));
        assert_eq!(selected("source sink"), None);
        assert_eq!(selected(""), None);
    }

    #[test]
    fn supply_connector_numbers() {
        let n = |name, ucsi| connector_number(name, ucsi);
        assert_eq!(
            n("ucsi-source-psy-USBC000:001", Some("USBC000:00")),
            Some(1)
        );
        assert_eq!(
            n("ucsi-source-psy-USBC000:0012", Some("USBC000:00")),
            Some(12)
        );
        assert_eq!(n("ucsi-source-psy-USBC000:002", None), Some(2));
        assert_eq!(n("ucsi-source-psy-AMDI0042:003", None), Some(3));
        assert_eq!(
            n(
                "ucsi-source-psy-cros_ec_ucsi.4.auto1",
                Some("cros_ec_ucsi.4.auto")
            ),
            Some(1)
        );
        assert_eq!(n("ucsi-source-psy-USBC000:00", None), None);
        assert_eq!(n("BAT0", None), None);
    }

    #[test]
    fn units() {
        assert_eq!(with_unit("20000mV", "mV"), Some(20000));
        assert_eq!(with_unit("2880mA", "mA"), Some(2880));
        assert_eq!(with_unit("45000", "mW"), Some(45000));
        assert_eq!(with_unit("x mA", "mA"), None);
    }

    #[test]
    fn active_pdo_needs_one_fixed_match() {
        let fixed = |mv| Pdo::Fixed { mv, ma: 3000 };
        let source = [fixed(5000), fixed(9000), fixed(20000)];
        assert_eq!(active_pdo(&source, 20000), Some(3));
        assert_eq!(active_pdo(&source, 15000), None);
        assert_eq!(active_pdo(&[fixed(5000), fixed(5000)], 5000), None);
        let pps = Pdo::Pps {
            min_mv: 3300,
            max_mv: 21000,
            ma: 3000,
        };
        assert_eq!(active_pdo(&[pps, fixed(9000)], 9000), Some(2));
    }
}
