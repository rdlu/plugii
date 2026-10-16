//! Thunderbolt / USB4: domains, their security level and host controller,
//! and each attached device router with its authorization and link speed.

use std::path::Path;

use super::{Root, entries, read as attr, read_parse};
use crate::model::{TbDevice, TbDomain, TbLink, Thunderbolt};

/// `None` where the machine has no Thunderbolt/USB4 domain.
pub fn read(root: &Root) -> Option<Thunderbolt> {
    let tb = root.path("sys/bus/thunderbolt/devices");
    let mut domains: Vec<TbDomain> = entries(&tb, |n| domain_index(n).is_some())
        .into_iter()
        .filter_map(|dir| {
            let index = domain_index(dir.file_name()?.to_str()?)?;
            // The host router (<domain>-0) says which controller this is.
            let host = tb.join(format!("{index}-0"));
            Some(TbDomain {
                index,
                security: attr(dir.join("security")),
                iommu_dma_protection: attr(dir.join("iommu_dma_protection")).as_deref()
                    == Some("1"),
                controller_vendor: attr(host.join("vendor_name")),
                controller_device: attr(host.join("device_name")),
                generation: read_parse(host.join("generation")),
            })
        })
        .collect();
    if domains.is_empty() {
        return None;
    }
    domains.sort_by_key(|d| d.index);
    let devices = entries(&tb, is_device_router)
        .into_iter()
        .filter(|dir| dir.join("authorized").exists())
        .filter_map(|dir| read_device(&dir))
        .collect();
    Some(Thunderbolt { domains, devices })
}

/// `domain1` → 1.
fn domain_index(name: &str) -> Option<u32> {
    let n = name.strip_prefix("domain")?;
    n.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| n.parse().ok())?
}

/// Device routers are `<domain>-<route>`; `<domain>-0` is the host, and names
/// with `.` or `:` are XDomain services and retimers, not devices.
fn is_device_router(name: &str) -> bool {
    let Some((domain, route)) = name.split_once('-') else {
        return false;
    };
    !domain.is_empty()
        && domain.bytes().all(|b| b.is_ascii_digit())
        && route.starts_with(|c: char| c.is_ascii_hexdigit())
        && route != "0"
        && !route.contains(['.', ':'])
}

fn read_device(dir: &Path) -> Option<TbDevice> {
    let route = dir.file_name()?.to_str()?.to_owned();
    // The script treats anything but 0 as authorized (1, or 2 for secure).
    let authorized = attr(dir.join("authorized")).as_deref() != Some("0");
    Some(TbDevice {
        route,
        unique_id: attr(dir.join("unique_id")),
        vendor: attr(dir.join("vendor_name")),
        name: attr(dir.join("device_name")),
        authorized,
        generation: read_parse(dir.join("generation")),
        link: link(dir),
        boltd_stored: None,
    })
}

fn link(dir: &Path) -> Option<TbLink> {
    Some(TbLink {
        rx_gbps: gbps(&attr(dir.join("rx_speed"))?)?,
        rx_lanes: read_parse(dir.join("rx_lanes"))?,
        tx_gbps: gbps(&attr(dir.join("tx_speed"))?)?,
        tx_lanes: read_parse(dir.join("tx_lanes"))?,
    })
}

/// `20.0 Gb/s` → 20.0.
fn gbps(text: &str) -> Option<f32> {
    text.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_names() {
        assert!(is_device_router("0-1"));
        assert!(is_device_router("1-301"));
        assert!(is_device_router("0-a01"));
        assert!(!is_device_router("0-0"));
        assert!(!is_device_router("0-1.1"));
        assert!(!is_device_router("0-0:1.1"));
        assert!(!is_device_router("domain0"));
        assert!(!is_device_router("x-1"));
        assert!(!is_device_router("0-"));
    }

    #[test]
    fn speeds() {
        assert_eq!(gbps("20.0 Gb/s"), Some(20.0));
        assert_eq!(gbps("10.0 Gb/s"), Some(10.0));
        assert_eq!(gbps("fast"), None);
    }
}
