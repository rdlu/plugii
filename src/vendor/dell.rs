//! Dell: the PC update catalog, which gets BIOS releases before LVFS does.
//!
//! `CatalogIndexPC.cab` maps each system ID (DMI `product_sku`) to a
//! per-model cab whose folder changes with every release. Both cabs hold one
//! UTF-16LE, CRLF XML file. The per-model catalog's BIOS components aren't in
//! version order; the highest `vendorVersion` is the latest.

use std::cmp::Ordering;
use std::io::{Cursor, Read};
use std::time::Duration;

use crate::model::{BiosCheck, Catalog, DellBios, DellCatalog, Dmi, Finding, FwupdState, Level};

use super::{Cache, Fetch, Gap, child_text, parse_xml, version_cmp};

/// Per request, as the script's `curl --max-time 15`.
pub const TIMEOUT: Duration = Duration::from_secs(15);

pub const BASE_URL: &str = "https://downloads.dell.com";
pub const INDEX_URL: &str = "https://downloads.dell.com/catalog/CatalogIndexPC.cab";

/// Dell's download page for a package.
pub fn driver_url(package_id: &str) -> String {
    format!(
        "https://www.dell.com/support/home/us/en/19/Drivers/DriversDetails?driverId={package_id}"
    )
}

/// What the catalog lists for this machine, from the cache or the network.
pub fn catalog(dmi: &Dmi, fetch: &dyn Fetch, cache: &Cache) -> Catalog {
    let Some(system_id) = dmi
        .product_sku
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Catalog::NotChecked;
    };
    let key = format!("dell-{system_id}");
    let bios = match cache.load::<DellBios>(&key) {
        Some(bios) => bios,
        None => match fetch_latest(system_id, fetch) {
            Ok(bios) => {
                let _ = cache.store(&key, &bios);
                bios
            }
            Err(gap) => return gap.catalog("Dell", system_id),
        },
    };
    let bios_check = compare_bios(dmi, &bios);
    Catalog::Dell(DellCatalog {
        system_id: system_id.to_owned(),
        bios,
        bios_check,
    })
}

/// Fetches the index, finds this system ID's catalog, fetches it and takes
/// its newest BIOS.
pub fn fetch_latest(system_id: &str, fetch: &dyn Fetch) -> Result<DellBios, Gap> {
    let index = fetch
        .get(INDEX_URL, TIMEOUT)
        .map_err(|_| Gap::Unreachable)?;
    let index = extract_xml(&index).map_err(Gap::Unreadable)?;
    let path = model_cab_path(&index, system_id)
        .map_err(Gap::Unreadable)?
        .ok_or(Gap::NotListed)?;
    let model = fetch
        .get(
            &format!("{BASE_URL}/{}", path.trim_start_matches('/')),
            TIMEOUT,
        )
        .map_err(|_| Gap::Unreachable)?;
    let model = extract_xml(&model).map_err(Gap::Unreadable)?;
    latest_bios(&model)
        .map_err(Gap::Unreadable)?
        .ok_or(Gap::NotListed)
}

/// The first `.xml` file in a cab, as text.
pub fn extract_xml(cab_bytes: &[u8]) -> Result<String, String> {
    let mut cabinet =
        cab::Cabinet::new(Cursor::new(cab_bytes)).map_err(|e| format!("not a cab: {e}"))?;
    let name = cabinet
        .folder_entries()
        .flat_map(|folder| folder.file_entries())
        .map(|file| file.name().to_owned())
        .find(|name| name.to_ascii_lowercase().ends_with(".xml"))
        .ok_or("no XML file in the cab")?;
    let mut bytes = Vec::new();
    cabinet
        .read_file(&name)
        .and_then(|mut r| r.read_to_end(&mut bytes))
        .map_err(|e| format!("can't extract {name}: {e}"))?;
    Ok(decode_xml(&bytes))
}

/// UTF-16LE (with or without its byte order mark) or UTF-8 bytes as text,
/// without carriage returns.
pub fn decode_xml(bytes: &[u8]) -> String {
    let utf16 = bytes.starts_with(&[0xff, 0xfe]) || (bytes.len() > 1 && bytes[1] == 0);
    let text = if utf16 {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.trim_start_matches('\u{feff}').replace('\r', "")
}

/// The per-model cab's path for `system_id`: the `GroupManifest` with a
/// `Model systemID="<id>"`, its `ManifestInformation path`.
pub fn model_cab_path(index_xml: &str, system_id: &str) -> Result<Option<String>, String> {
    let doc = parse_xml(index_xml)?;
    let group = doc
        .descendants()
        .filter(|n| n.has_tag_name("GroupManifest"))
        .find(|g| {
            g.descendants()
                .any(|m| m.has_tag_name("Model") && m.attribute("systemID") == Some(system_id))
        });
    Ok(group.and_then(|g| {
        g.descendants()
            .filter_map(|n| n.attribute("path"))
            .find(|p| p.to_ascii_lowercase().ends_with(".cab"))
            .map(str::to_owned)
    }))
}

/// The BIOS component with the highest `vendorVersion`.
pub fn latest_bios(model_xml: &str) -> Result<Option<DellBios>, String> {
    let doc = parse_xml(model_xml)?;
    let bios = doc
        .descendants()
        .filter(|n| n.has_tag_name("SoftwareComponent"))
        .filter(|c| {
            c.children()
                .any(|t| t.has_tag_name("ComponentType") && t.attribute("value") == Some("BIOS"))
        })
        .filter_map(|c| {
            let version = c.attribute("vendorVersion").filter(|v| !v.is_empty())?;
            let criticality = c
                .children()
                .find(|n| n.has_tag_name("Criticality"))
                .and_then(|n| child_text(n, "Display"))
                .map(str::to_owned);
            Some(DellBios {
                version: version.to_owned(),
                release_date: c.attribute("releaseDate").map(str::to_owned),
                package_id: c.attribute("packageID").unwrap_or_default().to_owned(),
                criticality,
            })
        })
        .max_by(|a, b| version_cmp(&a.version, &b.version));
    Ok(bios)
}

/// The running BIOS (DMI `bios_version`) against the catalog's.
pub fn compare_bios(dmi: &Dmi, latest: &DellBios) -> BiosCheck {
    match dmi.bios_id() {
        None => BiosCheck::NotCompared,
        Some(id) if version_cmp(id, &latest.version) == Ordering::Less => {
            BiosCheck::UpdateAvailable
        }
        Some(_) => BiosCheck::UpToDate,
    }
}

/// `Recommended, October 07, 2026`, or as much of it as the catalog gives.
pub fn release_note(bios: &DellBios) -> String {
    [bios.criticality.as_deref(), bios.release_date.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
}

/// "BIOS up to date"; a newer BIOS waits for [`update_finding`], which
/// knows whether fwupd has it too.
pub fn bios_finding(dmi: &Dmi, c: &DellCatalog) -> Option<Finding> {
    (c.bios_check == BiosCheck::UpToDate).then(|| Finding {
        level: Level::Ok,
        message: format!("BIOS up to date ({})", dmi.bios_id().unwrap_or("?")),
    })
}

/// A newer BIOS in Dell's catalog. Dell's catalog runs ahead of LVFS
/// (2.31.0 wasn't there after a refresh), so the advice depends on whether
/// fwupd offers a System Firmware update; `state` is `None` without fwupd.
pub fn update_finding(dmi: &Dmi, c: &DellCatalog, state: Option<&FwupdState>) -> Option<Finding> {
    if c.bios_check != BiosCheck::UpdateAvailable {
        return None;
    }
    let mut message = format!(
        "BIOS update available: {} → {}",
        dmi.bios_id().unwrap_or("?"),
        c.bios.version
    );
    let note = release_note(&c.bios);
    if !note.is_empty() {
        message.push_str(&format!(" ({note})"));
    }
    message.push_str(match state {
        None => " — flash from F12 → BIOS Flash Update with the .exe on the ESP",
        Some(s) if s.pending.iter().any(|p| p.name == "System Firmware") => {
            " — fwupd has it too: fwupdmgr update"
        }
        Some(_) => {
            " — not on LVFS yet; flash from F12 → BIOS Flash Update with the .exe on the \
             ESP, or wait for fwupdmgr"
        }
    });
    Some(Finding {
        level: Level::Warn,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16le_with_crlf() {
        let text = "\u{feff}<a>\r\nb</a>\r\n";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_xml(&bytes), "<a>\nb</a>\n");
        assert_eq!(decode_xml(b"<a>x</a>"), "<a>x</a>");
    }

    #[test]
    fn running_bios_against_the_catalog() {
        let latest = DellBios {
            version: "2.32.0".into(),
            release_date: None,
            package_id: "94TXT".into(),
            criticality: None,
        };
        let dmi = |v: &str| Dmi {
            bios_version: Some(v.into()),
            ..Dmi::default()
        };
        assert_eq!(
            compare_bios(&dmi("2.30.0"), &latest),
            BiosCheck::UpdateAvailable
        );
        assert_eq!(compare_bios(&dmi("2.32.0"), &latest), BiosCheck::UpToDate);
        assert_eq!(compare_bios(&dmi("2.33.0"), &latest), BiosCheck::UpToDate);
        assert_eq!(
            compare_bios(&Dmi::default(), &latest),
            BiosCheck::NotCompared
        );
    }

    #[test]
    fn not_a_cab() {
        assert!(extract_xml(b"<html>").unwrap_err().starts_with("not a cab"));
    }
}
