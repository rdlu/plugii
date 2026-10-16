//! Canned vendor catalogs for the firmware tests: a fetcher that serves the
//! fixtures under `tests/fixtures/firmware/` and never touches the network.

use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use plugii::firmware::{self, FwupdInput};
use plugii::fwupd;
use plugii::labels::Labels;
use plugii::model::Firmware;
use plugii::sysfs::Root;
use plugii::vendor::{Cache, Fetch, dell, lenovo};

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/firmware")
}

pub fn read(rel: &str) -> Vec<u8> {
    fs::read(fixtures().join(rel)).unwrap()
}

/// URL → body; anything else fails as if offline. Records what was asked.
#[derive(Default)]
pub struct Canned {
    pub bodies: HashMap<String, Vec<u8>>,
    pub asked: Mutex<Vec<String>>,
}

impl Canned {
    pub fn with(mut self, url: &str, body: Vec<u8>) -> Self {
        self.bodies.insert(url.to_owned(), body);
        self
    }

    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Fetch for Canned {
    fn get(&self, url: &str, _timeout: Duration) -> Result<Vec<u8>, String> {
        self.asked.lock().unwrap().push(url.to_owned());
        self.bodies
            .get(url)
            .cloned()
            .ok_or_else(|| format!("{url}: connection refused"))
    }
}

/// Lenovo's 21M4 catalog and its PD package.
pub fn lenovo() -> Canned {
    Canned::default()
        .with(&lenovo::catalog_url("21M4"), read("lenovo/21M4_Win11.xml"))
        .with(
            "https://download.lenovo.com/pccbbs/mobiles/r2kpd16w_2_.xml",
            read("lenovo/r2kpd16w_2_.xml"),
        )
}

/// Dell's index and the XPS 9320's catalog from `model` (a fixture under
/// `dell/`), both packed as Dell packs them.
pub fn dell(model: &str) -> Canned {
    Canned::default()
        .with(
            dell::INDEX_URL,
            dell_cab("CatalogIndexPC.xml", "dell/CatalogIndexPC.xml"),
        )
        .with(
            "https://downloads.dell.com/FOLDER15136825M/1/XPS_Notebook_0AF3.cab",
            dell_cab("XPS_Notebook_0AF3.xml", &format!("dell/{model}")),
        )
}

/// A fixture XML as Dell ships it: UTF-16LE with a byte order mark and CRLF
/// line ends, alone in an MS-ZIP cab.
pub fn dell_cab(name: &str, rel: &str) -> Vec<u8> {
    let text = String::from_utf8(read(rel)).unwrap();
    let text = format!("\u{feff}{}", text.replace('\n', "\r\n"));
    let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    cab(name, &bytes)
}

pub fn cab(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut builder = cab::CabinetBuilder::new();
    builder
        .add_folder(cab::CompressionType::MsZip)
        .add_file(name);
    let mut writer = builder.build(Cursor::new(Vec::new())).unwrap();
    while let Some(mut file) = writer.next_file().unwrap() {
        file.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// fwupdmgr's output from `fwupd/<name>-devices.json` and
/// `fwupd/<name>-updates.json`.
pub fn fwupd(name: &str) -> FwupdInput {
    let read = |kind: &str| String::from_utf8(read(&format!("fwupd/{name}-{kind}.json"))).unwrap();
    FwupdInput::Given(Some(fwupd::Output {
        devices: read("devices"),
        updates: read("updates"),
    }))
}

/// fwupdmgr installed, with no devices and nothing to update.
pub fn fwupd_empty() -> FwupdInput {
    FwupdInput::Given(Some(fwupd::Output {
        devices: r#"{"Devices": []}"#.into(),
        updates: r#"{"Devices": []}"#.into(),
    }))
}

/// `--firmware` for a fixture machine, without a cache.
pub fn collect(root: &Root, labels: &Labels, fetch: &dyn Fetch, fwupd: FwupdInput) -> Firmware {
    firmware::collect(root, labels, fetch, &Cache::none(), fwupd)
}
