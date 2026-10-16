//! Builds small sysfs trees for scenarios no captured machine shows, laid out
//! like the kernel's: real directories under `sys/devices/...`, relative
//! symlinks from `sys/bus/*` and `sys/class/*`.

#![allow(dead_code)]

pub mod fetch;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Component, Path, PathBuf};

use plugii::sysfs::Root;
use tempfile::TempDir;

pub struct Sysfs {
    dir: TempDir,
    /// Bus number → the root hub's real directory (relative).
    hubs: RefCell<HashMap<u32, PathBuf>>,
}

/// One USB device's attributes.
pub struct Dev<'a> {
    pub ids: (u16, u16),
    pub speed: &'a str,
    pub version: &'a str,
    pub class: u8,
    pub product: &'a str,
    pub bos: Option<&'a [u8]>,
    pub driver: &'a str,
}

impl Sysfs {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            hubs: RefCell::default(),
        }
    }

    pub fn root(&self) -> Root {
        Root::new(self.dir.path())
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Writes `rel` with `text` plus a newline, as sysfs prints it.
    pub fn file(&self, rel: impl AsRef<Path>, text: &str) {
        self.bytes(rel, format!("{text}\n").as_bytes());
    }

    pub fn bytes(&self, rel: impl AsRef<Path>, bytes: &[u8]) {
        let path = self.dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// A symlink at `rel` pointing at `target` (both relative to the root),
    /// written as a relative link like the kernel's.
    pub fn link(&self, rel: impl AsRef<Path>, target: impl AsRef<Path>) {
        let rel = rel.as_ref();
        let at = self.dir.path().join(rel);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        symlink(relative(rel.parent().unwrap(), target.as_ref()), at).unwrap();
    }

    pub fn dir(&self, rel: impl AsRef<Path>) {
        fs::create_dir_all(self.dir.path().join(rel)).unwrap();
    }

    /// Root hub `usb<bus>` on PCI controller `pci` with `ports` as
    /// (port, connect_type).
    pub fn root_hub(&self, pci: &str, bus: u32, speed: &str, ports: &[(u32, &str)]) {
        let hub = PathBuf::from(format!("sys/devices/pci0000:00/{pci}/usb{bus}"));
        self.file(hub.join("speed"), speed);
        self.file(hub.join("idVendor"), "1d6b");
        self.link(format!("sys/bus/usb/devices/usb{bus}"), &hub);
        let iface = hub.join(format!("{bus}-0:1.0"));
        self.dir(&iface);
        self.link(format!("sys/bus/usb/devices/{bus}-0:1.0"), &iface);
        for (port, connect_type) in ports {
            let p = iface.join(format!("usb{bus}-port{port}"));
            self.file(p.join("connect_type"), connect_type);
            self.file(p.join("over_current_count"), "0");
            self.file(p.join("state"), "not attached");
        }
        self.hubs.borrow_mut().insert(bus, hub);
    }

    /// The real directory of root port `usb<bus>-port<port>`.
    pub fn root_port(&self, bus: u32, port: u32) -> PathBuf {
        self.hubs.borrow()[&bus].join(format!("{bus}-0:1.0/usb{bus}-port{port}"))
    }

    /// The kernel's `peer` link between two root ports.
    pub fn peer(&self, a: (u32, u32), b: (u32, u32)) {
        let (pa, pb) = (self.root_port(a.0, a.1), self.root_port(b.0, b.1));
        self.link(pa.join("peer"), &pb);
        self.link(pb.join("peer"), &pa);
    }

    /// Device `name` (`2-1`, `2-1.4`) below its parent, with one interface
    /// bound to `dev.driver`.
    pub fn device(&self, name: &str, dev: Dev) {
        let (bus, path) = name.split_once('-').unwrap();
        let mut real = self.hubs.borrow()[&bus.parse::<u32>().unwrap()].clone();
        let mut so_far = format!("{bus}-");
        for (i, part) in path.split('.').enumerate() {
            if i > 0 {
                so_far.push('.');
            }
            so_far.push_str(part);
            real = real.join(&so_far);
        }
        self.file(real.join("idVendor"), &format!("{:04x}", dev.ids.0));
        self.file(real.join("idProduct"), &format!("{:04x}", dev.ids.1));
        self.file(real.join("speed"), dev.speed);
        self.file(real.join("version"), dev.version);
        self.file(real.join("bDeviceClass"), &format!("{:02x}", dev.class));
        self.file(real.join("bMaxPower"), "100mA");
        self.file(real.join("product"), dev.product);
        self.file(real.join("urbnum"), "42");
        self.file(real.join("rx_lanes"), "1");
        self.file(real.join("tx_lanes"), "1");
        if let Some(bos) = dev.bos {
            self.bytes(real.join("bos_descriptors"), bos);
        }
        self.link(real.join("driver"), "sys/bus/usb/drivers/usb");
        let iface = real.join(format!("{name}:1.0"));
        self.dir(&iface);
        self.link(
            iface.join("driver"),
            format!("sys/bus/usb/drivers/{}", dev.driver),
        );
        self.link(format!("sys/bus/usb/devices/{name}"), &real);
        self.link(format!("sys/bus/usb/devices/{name}:1.0"), &iface);
    }
}

fn relative(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for part in &to[common..] {
        out.push(part);
    }
    out
}

/// A BOS descriptor with only the SuperSpeed (5G) capability.
pub const BOS_5G: &[u8] = &[
    0x05, 0x0f, 0x16, 0x00, 0x02, 0x07, 0x10, 0x02, 0x06, 0x00, 0x00, 0x00, 0x0a, 0x10, 0x03, 0x00,
    0x0e, 0x00, 0x01, 0x08, 0xbe, 0x00,
];

/// A BOS descriptor with SuperSpeed and SuperSpeedPlus (10G) capabilities.
pub const BOS_10G: &[u8] = &[
    0x05, 0x0f, 0x2a, 0x00, 0x03, 0x07, 0x10, 0x02, 0x06, 0x00, 0x00, 0x00, 0x0a, 0x10, 0x03, 0x00,
    0x0e, 0x00, 0x01, 0x04, 0xe7, 0x00, 0x14, 0x10, 0x0a, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x11,
    0x00, 0x00, 0x30, 0x40, 0x0a, 0x00, 0xb0, 0x40, 0x0a, 0x00,
];
