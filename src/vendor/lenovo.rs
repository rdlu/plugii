//! Lenovo: the per-model update catalog (the only source for BIOS and USB-C
//! PD firmware on models LVFS doesn't carry), and the EC reset procedure.
//!
//! `https://download.lenovo.com/catalog/<MTM>_Win11.xml` lists one package
//! descriptor per update. The BIOS package's file name carries the BIOS
//! family and build (`r2kuj40w` → `R2KET40W`); the PD firmware package is
//! one of the `<xxx>pd<NN>w_…xml` ones, and its `<Version hex2dec="True">`
//! is the version the ESRT reports, in hex.

use std::time::Duration;

use crate::health::EC_RESET;
use crate::model::{
    BiosCheck, Catalog, Dmi, Finding, FwupdState, LenovoBios, LenovoCatalog, LenovoLatest,
    LenovoPd, Level,
};

use super::{Cache, Fetch, Gap, child_text, parse_xml};

/// Per request, as the script's `curl --max-time 8`.
pub const TIMEOUT: Duration = Duration::from_secs(8);

/// How Lenovo's EC is reset, added after "EC reset" in findings.
pub const EC_RESET_PROCEDURE: &str = "(power off, unplug all, hold power 15 s)";

pub fn catalog_url(mtm: &str) -> String {
    format!("https://download.lenovo.com/catalog/{mtm}_Win11.xml")
}

/// The machine type: the first 4 characters of `product_name` (`21M4000RBO`
/// → `21M4`).
pub fn mtm(dmi: &Dmi) -> Option<String> {
    let name = dmi.product_name.as_deref()?.trim();
    let mtm: String = name.chars().take(4).collect();
    (mtm.chars().count() == 4 && mtm.chars().all(|c| c.is_ascii_alphanumeric())).then_some(mtm)
}

/// What the catalog lists for this machine, from the cache or the network.
pub fn catalog(dmi: &Dmi, fetch: &dyn Fetch, cache: &Cache) -> Catalog {
    let Some(mtm) = mtm(dmi) else {
        return Catalog::NotChecked;
    };
    let key = format!("lenovo-{mtm}");
    let latest = match cache.load::<LenovoLatest>(&key) {
        Some(latest) => latest,
        None => match fetch_latest(&mtm, fetch) {
            Ok(latest) => {
                // A cache that can't be written only costs a fetch next time.
                let _ = cache.store(&key, &latest);
                latest
            }
            Err(gap) => return gap.catalog("Lenovo", &mtm),
        },
    };
    let bios_check = latest.bios.as_ref().map(|b| compare_bios(dmi, b));
    Catalog::Lenovo(LenovoCatalog {
        mtm,
        latest,
        bios_check,
    })
}

/// Fetches the catalog and the PD package descriptors. A catalog with
/// neither a BIOS nor a PD package is "not listed".
pub fn fetch_latest(mtm: &str, fetch: &dyn Fetch) -> Result<LenovoLatest, Gap> {
    let bytes = fetch
        .get(&catalog_url(mtm), TIMEOUT)
        .map_err(|_| Gap::Unreachable)?;
    let text = String::from_utf8_lossy(&bytes);
    let (bios, pd_urls) = parse_catalog(&text).map_err(Gap::Unreadable)?;
    // Several packages can look like PD firmware by name; the one whose
    // description says so is it.
    let pd = pd_urls.iter().find_map(|url| {
        let bytes = fetch.get(url, TIMEOUT).ok()?;
        parse_pd_package(&String::from_utf8_lossy(&bytes), url)
    });
    if bios.is_none() && pd.is_none() {
        return Err(Gap::NotListed);
    }
    Ok(LenovoLatest { bios, pd })
}

/// The catalog's first BIOS package, and every package descriptor URL that
/// looks like PD firmware.
pub fn parse_catalog(xml: &str) -> Result<(Option<LenovoBios>, Vec<String>), String> {
    let doc = parse_xml(xml)?;
    let packages = || {
        doc.descendants()
            .filter(|n| n.has_tag_name("package"))
            .filter_map(|p| {
                let location = child_text(p, "location")?;
                Some((location, child_text(p, "category").unwrap_or("")))
            })
    };
    let bios = packages()
        .find(|(_, category)| category.starts_with("BIOS UEFI"))
        .and_then(|(url, _)| bios_package(url));
    let pd = packages()
        .filter(|(url, _)| is_pd_package(file_name(url)))
        .map(|(url, _)| url.to_owned())
        .collect();
    Ok((bios, pd))
}

fn file_name(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// `…/r2kuj40w_2_.xml` → prefix `r2k`, build 40: three letters or digits,
/// two letters, two digits, `w_`.
pub fn bios_package(url: &str) -> Option<LenovoBios> {
    let name = file_name(url).as_bytes();
    let alnum = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let ok = name.len() > 9
        && name[..3].iter().all(alnum)
        && name[3..5].iter().all(u8::is_ascii_lowercase)
        && name[5..7].iter().all(u8::is_ascii_digit)
        && &name[7..9] == b"w_";
    if !ok {
        return None;
    }
    let name = file_name(url);
    Some(LenovoBios {
        package: name[..8].to_owned(),
        prefix: name[..3].to_owned(),
        build: name[5..7].parse().ok()?,
        url: url.to_owned(),
    })
}

/// `r2kpd16w_2_.xml`: three letters or digits, `pd`, two digits, `w_`, and
/// an `.xml` descriptor.
fn is_pd_package(name: &str) -> bool {
    let b = name.as_bytes();
    let alnum = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    b.len() > 9
        && b[..3].iter().all(alnum)
        && &b[3..5] == b"pd"
        && b[5..7].iter().all(u8::is_ascii_digit)
        && &b[7..9] == b"w_"
        && name.ends_with(".xml")
}

/// A package descriptor that says "PD Firmware": its hex version
/// (`<Version hex2dec="True">105^</Version>` → `105`) and release date.
pub fn parse_pd_package(xml: &str, url: &str) -> Option<LenovoPd> {
    if !xml.contains("PD Firmware") {
        return None;
    }
    let doc = parse_xml(xml).ok()?;
    let version = doc.descendants().find(|n| {
        n.has_tag_name("Version")
            && n.attribute("hex2dec")
                .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    })?;
    let hex: String = version
        .text()?
        .trim()
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    if hex.is_empty() {
        return None;
    }
    let root = doc.root_element();
    let package = root
        .attribute("id")
        .map(str::to_owned)
        .unwrap_or_else(|| file_name(url).chars().take(8).collect());
    Some(LenovoPd {
        package,
        version_hex: hex.to_ascii_lowercase(),
        release_date: child_text(root, "ReleaseDate").map(str::to_owned),
        url: url.to_owned(),
    })
}

/// The running BIOS's build: the two digits before the final `W` of its id
/// (`R2KET40W` → 40).
pub fn bios_build(bios_id: &str) -> Option<u32> {
    let digits = bios_id.strip_suffix('W')?;
    let n = digits.len();
    if n < 2 || !digits.as_bytes()[n - 2..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    digits[n - 2..].parse().ok()
}

/// The running BIOS against the catalog's: another family (the id doesn't
/// start with the package prefix) or an unreadable build is not compared.
pub fn compare_bios(dmi: &Dmi, latest: &LenovoBios) -> BiosCheck {
    let Some(id) = dmi.bios_id() else {
        return BiosCheck::NotCompared;
    };
    let Some(build) = bios_build(id) else {
        return BiosCheck::NotCompared;
    };
    if !id.to_ascii_lowercase().starts_with(&latest.prefix) {
        return BiosCheck::NotCompared;
    }
    if build >= latest.build {
        BiosCheck::UpToDate
    } else {
        BiosCheck::UpdateAvailable
    }
}

/// The installed PD firmware (an ESRT version) against the catalog's hex
/// one; `None` when either isn't a number.
pub fn pd_up_to_date(installed: u64, latest: &LenovoPd) -> Option<bool> {
    let latest = u64::from_str_radix(&latest.version_hex, 16).ok()?;
    Some(installed >= latest)
}

/// `R2KET40W, 1.29`: the BIOS id and its release.
fn bios_name(dmi: &Dmi) -> String {
    let id = dmi.bios_id().unwrap_or("?");
    match &dmi.bios_release {
        Some(rel) => format!("{id}, {rel}"),
        None => id.to_owned(),
    }
}

/// The BIOS health finding; none when the catalog lists no BIOS or it's
/// another family.
pub fn bios_finding(dmi: &Dmi, c: &LenovoCatalog) -> Option<Finding> {
    let bios = c.latest.bios.as_ref()?;
    let (level, message) = match c.bios_check? {
        BiosCheck::UpToDate => (Level::Ok, format!("BIOS up to date ({})", bios_name(dmi))),
        BiosCheck::UpdateAvailable => (
            Level::Warn,
            format!(
                "BIOS update available: {} → build {:02}. Install from Windows ({}.exe); \
                 Lenovo's bootable ISO flasher doesn't work on every model",
                dmi.bios_id().unwrap_or("?"),
                bios.build,
                bios.package
            ),
        ),
        BiosCheck::NotCompared => return None,
    };
    Some(Finding { level, message })
}

/// Each labelled ESRT device whose label says "PD" against the catalog's PD
/// firmware.
pub fn pd_findings(c: &LenovoCatalog, state: &FwupdState) -> Vec<Finding> {
    let Some(pd) = &c.latest.pd else {
        return Vec::new();
    };
    state
        .labelled
        .iter()
        .filter(|d| d.label.contains("PD"))
        .filter_map(|d| {
            let have = d.version_number()?;
            Some(if pd_up_to_date(have, pd)? {
                Finding {
                    level: Level::Ok,
                    message: format!("{} up to date (0x{have:x})", d.label),
                }
            } else {
                Finding {
                    level: Level::Warn,
                    message: format!(
                        "{} update available: 0x{have:x} → 0x{} (Windows-only installer)",
                        d.label, pd.version_hex
                    ),
                }
            })
        })
        .collect()
}

/// Lenovo's EC reset procedure after each "EC reset".
pub fn advise(findings: &mut [Finding]) {
    for f in findings {
        if f.message.ends_with(EC_RESET) {
            f.message.push(' ');
            f.message.push_str(EC_RESET_PROCEDURE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtm_is_the_first_four_characters() {
        let dmi = |name: &str| Dmi {
            product_name: Some(name.into()),
            ..Dmi::default()
        };
        assert_eq!(mtm(&dmi("21M4000RBO")).as_deref(), Some("21M4"));
        assert_eq!(mtm(&dmi("21M")), None);
        assert_eq!(mtm(&Dmi::default()), None);
    }

    #[test]
    fn bios_package_names() {
        let b = bios_package("https://download.lenovo.com/pccbbs/mobiles/r2kuj40w_2_.xml").unwrap();
        assert_eq!(
            (b.package.as_str(), b.prefix.as_str(), b.build),
            ("r2kuj40w", "r2k", 40)
        );
        assert_eq!(bios_package("https://x/fwnva70_2_.xml"), None);
        assert!(is_pd_package("r2kpd16w_2_.xml"));
        assert!(!is_pd_package("r2kpd16w_2_.exe"));
        assert!(!is_pd_package("usbcundkfw3098_w10w11_2_.xml"));
    }

    #[test]
    fn bios_builds() {
        assert_eq!(bios_build("R2KET40W"), Some(40));
        assert_eq!(bios_build("R2KET4XW"), None);
        assert_eq!(bios_build("2.30.0"), None);
    }

    #[test]
    fn bios_family_mismatch_is_not_compared() {
        let latest = bios_package("https://x/r2kuj41w_2_.xml").unwrap();
        let dmi = |v: &str| Dmi {
            bios_version: Some(v.into()),
            ..Dmi::default()
        };
        assert_eq!(
            compare_bios(&dmi("R2KET40W (1.29 )"), &latest),
            BiosCheck::UpdateAvailable
        );
        assert_eq!(
            compare_bios(&dmi("R2KET41W (1.30 )"), &latest),
            BiosCheck::UpToDate
        );
        assert_eq!(compare_bios(&dmi("R2KET42W"), &latest), BiosCheck::UpToDate);
        assert_eq!(
            compare_bios(&dmi("N3ZET40W (1.29 )"), &latest),
            BiosCheck::NotCompared
        );
        assert_eq!(
            compare_bios(&Dmi::default(), &latest),
            BiosCheck::NotCompared
        );
    }

    #[test]
    fn a_pd_lookalike_without_pd_firmware_is_skipped() {
        let xml = r#"<?xml version="1.0"?><Package id="r2kpd01w"><Title><Desc>Power utility</Desc></Title>
            <DetectInstall><_Firmware><Version hex2dec="True">999^</Version></_Firmware></DetectInstall></Package>"#;
        assert_eq!(parse_pd_package(xml, "https://x/r2kpd01w_2_.xml"), None);
    }

    #[test]
    fn pd_versions_compare_in_hex() {
        let pd = LenovoPd {
            package: "r2kpd16w".into(),
            version_hex: "105".into(),
            release_date: None,
            url: String::new(),
        };
        assert_eq!(pd_up_to_date(261, &pd), Some(true));
        assert_eq!(pd_up_to_date(260, &pd), Some(false));
    }

    #[test]
    fn ec_reset_gets_the_procedure() {
        let mut f = vec![
            Finding {
                level: Level::Fail,
                message: format!("hub stuck. Flip the plug; if that fails, {EC_RESET}"),
            },
            Finding {
                level: Level::Ok,
                message: "kernel log clean".into(),
            },
        ];
        advise(&mut f);
        assert_eq!(
            f[0].message,
            "hub stuck. Flip the plug; if that fails, EC reset (power off, unplug all, hold power 15 s)"
        );
        assert_eq!(f[1].message, "kernel log clean");
    }
}
