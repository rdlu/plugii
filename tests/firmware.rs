//! `--firmware` pieces against the trimmed vendor catalogs and the fwupd
//! captures: catalog parsers, the cache, fwupd parsing, offline behaviour
//! and the vendor findings. Nothing here touches the network.

mod common;

use std::fs;
use std::time::{Duration, SystemTime};

use common::fetch::{self, Canned, dell_cab, fixtures, lenovo, read};
use plugii::firmware::FwupdInput;
use plugii::fwupd;
use plugii::labels::{self, Labels};
use plugii::model::{
    BiosCheck, Catalog, DellBios, Dmi, Fwupd, FwupdState, LenovoLatest, Level, PendingUpdate,
};
use plugii::sysfs::Root;
use plugii::vendor::{self, Cache, Gap, dell as dell_mod, lenovo as lenovo_mod};
use tempfile::TempDir;

fn text(rel: &str) -> String {
    String::from_utf8(read(rel)).unwrap()
}

fn sysfs(name: &str) -> Root {
    Root::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sysfs")
            .join(name),
    )
}

fn daisy_labels() -> Labels {
    labels::read(&fixtures().join("../labels/daisy")).unwrap()
}

#[test]
fn lenovo_catalog_bios_and_pd_packages() {
    let (bios, pd) = lenovo_mod::parse_catalog(&text("lenovo/21M4_Win11.xml")).unwrap();
    let bios = bios.unwrap();
    assert_eq!(bios.package, "r2kuj40w");
    assert_eq!(bios.prefix, "r2k");
    assert_eq!(bios.build, 40);
    assert_eq!(
        bios.url,
        "https://download.lenovo.com/pccbbs/mobiles/r2kuj40w_2_.xml"
    );
    // The USB-C dock firmware and the other packages don't look like PD.
    assert_eq!(
        pd,
        ["https://download.lenovo.com/pccbbs/mobiles/r2kpd16w_2_.xml"]
    );
}

#[test]
fn lenovo_pd_package_version_is_hex() {
    let url = "https://download.lenovo.com/pccbbs/mobiles/r2kpd16w_2_.xml";
    let pd = lenovo_mod::parse_pd_package(&text("lenovo/r2kpd16w_2_.xml"), url).unwrap();
    assert_eq!(pd.package, "r2kpd16w");
    assert_eq!(pd.version_hex, "105");
    assert_eq!(pd.release_date.as_deref(), Some("2026-03-17"));
    assert_eq!(lenovo_mod::pd_up_to_date(0x105, &pd), Some(true));
}

#[test]
fn lenovo_catalog_fetches_catalog_then_pd_package() {
    let fetch = lenovo();
    let latest = lenovo_mod::fetch_latest("21M4", &fetch).unwrap();
    assert_eq!(latest.bios.unwrap().build, 40);
    assert_eq!(latest.pd.unwrap().version_hex, "105");
    assert_eq!(
        fetch.asked(),
        [
            "https://download.lenovo.com/catalog/21M4_Win11.xml",
            "https://download.lenovo.com/pccbbs/mobiles/r2kpd16w_2_.xml"
        ]
    );
}

#[test]
fn dell_cab_extracts_utf16_xml() {
    let cab = dell_cab("CatalogIndexPC.xml", "dell/CatalogIndexPC.xml");
    let xml = dell_mod::extract_xml(&cab).unwrap();
    assert_eq!(xml, text("dell/CatalogIndexPC.xml"));
    assert!(!xml.contains('\r'));
    // A cab without an XML file in it.
    let err = dell_mod::extract_xml(&fetch::cab("readme.txt", b"hi")).unwrap_err();
    assert_eq!(err, "no XML file in the cab");
}

#[test]
fn dell_index_finds_the_model_catalog_by_system_id() {
    let index = text("dell/CatalogIndexPC.xml");
    assert_eq!(
        dell_mod::model_cab_path(&index, "0AF3").unwrap().as_deref(),
        Some("FOLDER15136825M/1/XPS_Notebook_0AF3.cab")
    );
    assert_eq!(
        dell_mod::model_cab_path(&index, "0AF4").unwrap().as_deref(),
        Some("FOLDER15041822M/1/Inspiron_0AF4.cab")
    );
    // Neither a missing ID nor a prefix of one matches.
    assert_eq!(dell_mod::model_cab_path(&index, "0AF").unwrap(), None);
    assert_eq!(dell_mod::model_cab_path(&index, "FFFF").unwrap(), None);
}

#[test]
fn dell_latest_bios_is_the_highest_version_not_the_last() {
    // 2.31.0, Dell Command Update 5.7.1 (not a BIOS), 2.32.0, 2.29.0, 2.30.0.
    let bios = dell_mod::latest_bios(&text("dell/XPS_Notebook_0AF3.xml"))
        .unwrap()
        .unwrap();
    assert_eq!(
        bios,
        DellBios {
            version: "2.32.0".into(),
            release_date: Some("October 07, 2026".into()),
            package_id: "94TXT".into(),
            criticality: Some("Recommended".into()),
        }
    );
    let bios = dell_mod::latest_bios(&text("dell/XPS_Notebook_0AF3-2.30.0.xml"))
        .unwrap()
        .unwrap();
    assert_eq!(bios.version, "2.30.0");
    assert_eq!(bios.criticality.as_deref(), Some("Urgent"));
}

#[test]
fn dell_catalog_through_both_cabs() {
    let fetch = fetch::dell("XPS_Notebook_0AF3.xml");
    let bios = dell_mod::fetch_latest("0AF3", &fetch).unwrap();
    assert_eq!(bios.version, "2.32.0");
    // A system ID the index doesn't list.
    assert_eq!(dell_mod::fetch_latest("FFFF", &fetch), Err(Gap::NotListed));
    assert_eq!(
        Gap::NotListed.catalog("Dell", "FFFF"),
        Catalog::NotListed {
            vendor: "Dell".into(),
            model: "FFFF".into()
        }
    );
    // A broken index.
    let broken = Canned::default().with(dell_mod::INDEX_URL, b"<html>".to_vec());
    assert!(matches!(
        dell_mod::fetch_latest("0AF3", &broken),
        Err(Gap::Unreadable(error)) if error.starts_with("not a cab")
    ));
    // The index, but not the model's catalog.
    let half = Canned::default().with(
        dell_mod::INDEX_URL,
        dell_cab("CatalogIndexPC.xml", "dell/CatalogIndexPC.xml"),
    );
    assert_eq!(dell_mod::fetch_latest("0AF3", &half), Err(Gap::Unreachable));
}

#[test]
fn offline_is_unreachable_not_an_error() {
    let offline = Canned::default();
    let fw = fetch::collect(
        &sysfs("daisy-charger"),
        &daisy_labels(),
        &offline,
        FwupdInput::Given(None),
    );
    assert_eq!(
        fw.catalog,
        Catalog::Unreachable {
            vendor: "Lenovo".into(),
            model: "21M4".into()
        }
    );
    assert_eq!(fw.fwupd, Fwupd::NotInstalled);
    let fw = fetch::collect(
        &sysfs("xps"),
        &Labels::default(),
        &offline,
        FwupdInput::Given(None),
    );
    assert_eq!(
        fw.catalog,
        Catalog::Unreachable {
            vendor: "Dell".into(),
            model: "0AF3".into()
        }
    );
}

#[test]
fn dmi_from_the_fixtures() {
    let fw = fetch::collect(
        &sysfs("daisy-charger"),
        &Labels::default(),
        &lenovo(),
        fetch::fwupd_empty(),
    );
    assert_eq!(
        fw.dmi,
        Dmi {
            sys_vendor: Some("LENOVO".into()),
            product_name: Some("21M4000RBO".into()),
            product_sku: Some("LENOVO_MT_21M4_BU_Think_FM_ThinkPad E14 Gen 6".into()),
            product_family: Some("ThinkPad E14 Gen 6".into()),
            bios_version: Some("R2KET40W (1.29 )".into()),
            bios_release: Some("1.29".into()),
            bios_date: Some("05/19/2026".into()),
            ec_firmware_release: Some("1.29".into()),
        }
    );
    assert_eq!(fw.dmi.bios_id(), Some("R2KET40W"));
    let Catalog::Lenovo(c) = &fw.catalog else {
        panic!("{:?}", fw.catalog)
    };
    assert_eq!(c.bios_check, Some(BiosCheck::UpToDate));
    // No sysfs DMI (a hand-made fixture): no vendor, nothing fetched.
    let none = Canned::default();
    let fw = fetch::collect(
        &sysfs("prochot"),
        &Labels::default(),
        &none,
        fetch::fwupd_empty(),
    );
    assert_eq!(fw.dmi, Dmi::default());
    assert_eq!(fw.catalog, Catalog::NotChecked);
    assert!(none.asked().is_empty());
}

fn latest() -> LenovoLatest {
    lenovo_mod::fetch_latest("21M4", &lenovo()).unwrap()
}

#[test]
fn cache_fresh_stale_and_refresh() {
    let dir = TempDir::new().unwrap();
    let cache = Cache::new(Some(dir.path().join("plugii")), false);
    assert_eq!(cache.load::<LenovoLatest>("lenovo-21M4"), None);
    cache.store("lenovo-21M4", &latest()).unwrap();
    assert_eq!(cache.load("lenovo-21M4"), Some(latest()));

    // --refresh ignores it.
    let refresh = Cache::new(Some(dir.path().join("plugii")), true);
    assert_eq!(refresh.load::<LenovoLatest>("lenovo-21M4"), None);

    // A day old is stale.
    let path = cache.path("lenovo-21M4").unwrap();
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(24 * 3600 + 60))
        .unwrap();
    assert_eq!(cache.load::<LenovoLatest>("lenovo-21M4"), None);
    file.set_modified(SystemTime::now() - Duration::from_secs(23 * 3600))
        .unwrap();
    assert_eq!(cache.load("lenovo-21M4"), Some(latest()));

    // Garbage reads as nothing cached.
    fs::write(&path, "{").unwrap();
    assert_eq!(cache.load::<LenovoLatest>("lenovo-21M4"), None);
}

#[test]
fn cache_writes_atomically() {
    let dir = TempDir::new().unwrap();
    let cache = Cache::new(Some(dir.path().join("plugii")), false);
    let path = cache.path("dell-0AF3").unwrap();
    let bios = |v: &str| DellBios {
        version: v.into(),
        release_date: None,
        package_id: "94TXT".into(),
        criticality: None,
    };
    cache.store("dell-0AF3", &bios("2.31.0")).unwrap();
    // A reader holding the old file keeps reading the old contents: the new
    // one is renamed over it, not written into it.
    let old = fs::File::open(&path).unwrap();
    cache.store("dell-0AF3", &bios("2.32.0")).unwrap();
    let old: DellBios = serde_json::from_reader(old).unwrap();
    assert_eq!(old.version, "2.31.0");
    assert_eq!(cache.load("dell-0AF3"), Some(bios("2.32.0")));
    // No temporary file left behind.
    let names: Vec<_> = fs::read_dir(dir.path().join("plugii"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["dell-0AF3.json"]);
    // No cache directory: nothing stored, no error.
    Cache::none().store("dell-0AF3", &bios("1")).unwrap();
}

#[test]
fn a_cached_catalog_is_not_fetched_again() {
    let dir = TempDir::new().unwrap();
    let cache = Cache::new(Some(dir.path().to_owned()), false);
    let dmi = vendor::read_dmi(&sysfs("daisy-charger"));
    let first = lenovo();
    let a = vendor::catalog(&dmi, &first, &cache);
    assert_eq!(first.asked().len(), 2);
    assert!(dir.path().join("lenovo-21M4.json").exists());
    let second = Canned::default();
    assert_eq!(vendor::catalog(&dmi, &second, &cache), a);
    assert!(second.asked().is_empty());
    // Offline results aren't cached.
    let dmi = vendor::read_dmi(&sysfs("xps"));
    vendor::catalog(&dmi, &second, &cache);
    assert!(!dir.path().join("dell-0AF3.json").exists());
}

#[test]
fn fwupd_labels_esrt_devices_by_guid() {
    let FwupdInput::Given(Some(out)) = fetch::fwupd("daisy") else {
        unreachable!()
    };
    let state = fwupd::parse(&out, &daisy_labels());
    assert_eq!(state.labelled.len(), 1);
    let pd = &state.labelled[0];
    assert_eq!(pd.guid, "a77b326c-bbf9-4867-af3b-a01f96f274f5");
    assert_eq!(pd.label, "USB-C PD firmware");
    assert_eq!(pd.version, "261");
    assert_eq!(pd.version_number(), Some(0x105));
    // UpdateState 2 is success; nothing pending.
    assert!(state.failed.is_empty());
    assert!(state.pending.is_empty());
    // Without labels, nothing is labelled.
    assert!(fwupd::parse(&out, &Labels::default()).labelled.is_empty());
}

#[test]
fn fwupd_failed_and_pending_updates() {
    let FwupdInput::Given(Some(out)) = fetch::fwupd("faults") else {
        unreachable!()
    };
    let state = fwupd::parse(&out, &daisy_labels());
    assert_eq!(state.failed, ["System Firmware"]);
    assert_eq!(
        state.pending,
        [PendingUpdate {
            name: "Integrated Camera".into(),
            version: "59.14".into(),
            release: "59.16".into(),
        }]
    );
    assert_eq!(state.labelled[0].version, "260");
}

fn dell_findings(fwupd: Option<FwupdState>) -> Vec<(Level, String)> {
    let fw = fetch::collect(
        &sysfs("xps"),
        &Labels::default(),
        &fetch::dell("XPS_Notebook_0AF3.xml"),
        FwupdInput::Given(None),
    );
    let Catalog::Dell(c) = &fw.catalog else {
        panic!("{:?}", fw.catalog)
    };
    assert_eq!(c.bios_check, BiosCheck::UpdateAvailable);
    dell_mod::update_finding(&fw.dmi, c, fwupd.as_ref())
        .into_iter()
        .map(|f| (f.level, f.message))
        .collect()
}

#[test]
fn dell_update_wording_follows_fwupd() {
    let base = "BIOS update available: 2.30.0 → 2.32.0 (Recommended, October 07, 2026)";
    assert_eq!(
        dell_findings(None),
        [(
            Level::Warn,
            format!("{base} — flash from F12 → BIOS Flash Update with the .exe on the ESP")
        )]
    );
    assert_eq!(
        dell_findings(Some(FwupdState::default())),
        [(
            Level::Warn,
            format!(
                "{base} — not on LVFS yet; flash from F12 → BIOS Flash Update with the .exe \
                 on the ESP, or wait for fwupdmgr"
            )
        )]
    );
    let lvfs = FwupdState {
        pending: vec![PendingUpdate {
            name: "System Firmware".into(),
            version: "2.30.0".into(),
            release: "2.32.0".into(),
        }],
        ..FwupdState::default()
    };
    assert_eq!(
        dell_findings(Some(lvfs)),
        [(
            Level::Warn,
            format!("{base} — fwupd has it too: fwupdmgr update")
        )]
    );
}

#[test]
fn lenovo_bios_update_and_family_mismatch() {
    let catalog = |bios: &str| {
        let xml = text("lenovo/21M4_Win11.xml").replace("r2kuj40w", bios);
        Canned::default().with(&lenovo_mod::catalog_url("21M4"), xml.into_bytes())
    };
    let fw = |fetch: &Canned| {
        fetch::collect(
            &sysfs("daisy-charger"),
            &daisy_labels(),
            fetch,
            fetch::fwupd("daisy"),
        )
    };
    let newer = fw(&catalog("r2kuj41w"));
    let Catalog::Lenovo(c) = &newer.catalog else {
        panic!()
    };
    assert_eq!(c.bios_check, Some(BiosCheck::UpdateAvailable));
    // The PD package wasn't served: no PD verdict, no PD finding.
    assert_eq!(c.latest.pd, None);
    let f = lenovo_mod::bios_finding(&newer.dmi, c).unwrap();
    assert_eq!(f.level, Level::Warn);
    assert_eq!(
        f.message,
        "BIOS update available: R2KET40W → build 41. Install from Windows (r2kuj41w.exe); \
         Lenovo's bootable ISO flasher doesn't work on every model"
    );

    let other = fw(&catalog("n3zuj45w"));
    let Catalog::Lenovo(c) = &other.catalog else {
        panic!()
    };
    assert_eq!(c.bios_check, Some(BiosCheck::NotCompared));
    assert_eq!(lenovo_mod::bios_finding(&other.dmi, c), None);
}

#[test]
fn firmware_json_round_trips() {
    let fw = fetch::collect(
        &sysfs("daisy-charger"),
        &daisy_labels(),
        &lenovo(),
        fetch::fwupd("faults"),
    );
    let json = serde_json::to_string(&fw).unwrap();
    assert_eq!(
        serde_json::from_str::<plugii::model::Firmware>(&json).unwrap(),
        fw
    );
    for gap in [
        Catalog::Unreachable {
            vendor: "Dell".into(),
            model: "0AF3".into(),
        },
        Catalog::NotChecked,
    ] {
        let json = serde_json::to_string(&gap).unwrap();
        assert_eq!(serde_json::from_str::<Catalog>(&json).unwrap(), gap);
    }
}
