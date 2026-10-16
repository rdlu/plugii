//! What one scan of the machine found: plain data the sysfs readers fill and
//! health, the report, the TUI and `--json` read. No behaviour beyond small
//! derived values (watts, speed labels).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unix time in milliseconds.
    pub taken_at_ms: u64,
    pub host: String,
    pub sockets: Vec<Socket>,
    pub typec: TypecState,
    /// `None` where there is no Thunderbolt/USB4 bus.
    pub thunderbolt: Option<Thunderbolt>,
    /// Lit external connectors (eDP and writeback left out).
    pub displays: Vec<Display>,
    pub power: Power,
    pub cpu: Option<CpuSample>,
    pub kernel_log: KernelLog,
}

/// One physical hotplug socket, seen from its USB 2 root port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Socket {
    /// `<pci-address>-<port>` of the USB 2 root port, e.g. `0000:00:14.0-1`.
    pub key: String,
    pub label: Option<String>,
    pub usb2: Half,
    pub usb3: Usb3Side,
    /// The UCSI port (`port0`) labelled the same as this socket.
    pub typec_port: Option<String>,
}

/// One root port: `<bus>-<port>` in sysfs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Half {
    pub bus: u32,
    pub port: u32,
    /// What is plugged into the root port (hubs carry the rest of the tree);
    /// `None` when the half is idle.
    pub device: Option<UsbDevice>,
    /// The root port's `over_current_count`.
    #[serde(default)]
    pub over_current_count: Option<u32>,
    /// The root port's `state`: `configured`, `suspended`, `not attached`, …
    #[serde(default)]
    pub port_state: Option<String>,
    /// Where ACPI says the port is. Only a hint for unlabelled sockets, never
    /// trusted over labels: it is wrong on some machines (the XPS 9320 calls
    /// both sockets "left").
    #[serde(default)]
    pub physical_location: Option<PhysicalLocation>,
}

/// ACPI `_PLD` as the kernel exposes it under `physical_location/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalLocation {
    /// `top`, `bottom`, `left`, `right`, `front`, `back`, `unknown`.
    pub panel: Option<String>,
    /// `left`, `center`, `right`.
    pub horizontal: Option<String>,
    /// `upper`, `center`, `lower`.
    pub vertical: Option<String>,
    pub dock: bool,
    pub lid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// A scan holds a handful of sockets; boxing the half would only add noise.
#[allow(clippy::large_enum_variant)]
pub enum Usb3Side {
    /// Paired by the kernel's `peer` link or a `peer:` label.
    Paired(Half),
    /// The machine has no `peer` links and no `peer:` label pairs this socket,
    /// so whether it has a USB 3 half isn't known.
    Unknown,
    /// Other ports have `peer` links and this one doesn't: USB 2 only.
    Usb2Only,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsbDevice {
    /// The sysfs device name, e.g. `3-1.2`.
    pub sysfs_name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub speed: Speed,
    /// The fastest speed the device supports, from its BOS descriptor (or
    /// bcdUSB for USB 2 devices); `None` when it can't be told.
    #[serde(default)]
    pub capable_speed: Option<Speed>,
    /// SuperSpeed lanes in use (`rx_lanes` / `tx_lanes`); 2 on a Gen 2x2 link.
    #[serde(default)]
    pub rx_lanes: Option<u8>,
    #[serde(default)]
    pub tx_lanes: Option<u8>,
    /// bcdUSB, e.g. `0x0320` for USB 3.2.
    pub bcd_usb: Option<u16>,
    /// bDeviceClass; 9 is a hub.
    pub device_class: u8,
    /// bMaxPower in mA.
    pub max_power_ma: Option<u32>,
    pub driver: Option<String>,
    /// URBs submitted so far, for activity rates.
    pub urbnum: Option<u64>,
    pub children: Vec<UsbDevice>,
}

/// Negotiated link speed, as sysfs `speed` reports it in Mb/s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speed {
    Low,
    Full,
    High,
    Super,
    SuperPlus,
    SuperPlus2x2,
    /// Any other whole number of Mb/s.
    Other(u32),
}

impl Speed {
    /// Parses sysfs `speed` (`1.5`, `12`, `480`, `5000`, …).
    pub fn from_sysfs(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "1.5" => Self::Low,
            "12" => Self::Full,
            "480" => Self::High,
            "5000" => Self::Super,
            "10000" => Self::SuperPlus,
            "20000" => Self::SuperPlus2x2,
            other => Self::Other(other.parse().ok()?),
        })
    }

    pub fn mbps(self) -> f32 {
        match self {
            Self::Low => 1.5,
            Self::Full => 12.0,
            Self::High => 480.0,
            Self::Super => 5000.0,
            Self::SuperPlus => 10000.0,
            Self::SuperPlus2x2 => 20000.0,
            Self::Other(mbps) => mbps as f32,
        }
    }

    pub fn is_superspeed(self) -> bool {
        self.mbps() >= 5000.0
    }

    /// The compact form for one-line events: `480M`, `5G`, `10G`.
    pub fn short(self) -> String {
        match self {
            Self::Low => "1.5M".into(),
            Self::Full => "12M".into(),
            Self::High => "480M".into(),
            Self::Super => "5G".into(),
            Self::SuperPlus => "10G".into(),
            Self::SuperPlus2x2 => "20G".into(),
            Self::Other(mbps) if mbps >= 1000 && mbps % 1000 == 0 => {
                format!("{}G", mbps / 1000)
            }
            Self::Other(mbps) => format!("{mbps}M"),
        }
    }

    /// The `usb-ports` script's wording.
    pub fn label(self) -> String {
        match self {
            Self::Low => "1.5M (USB 1 low)".into(),
            Self::Full => "12M (USB 1 full)".into(),
            Self::High => "480M (USB 2)".into(),
            Self::Super => "5G (USB 3 Gen 1)".into(),
            Self::SuperPlus => "10G (USB 3 Gen 2)".into(),
            Self::SuperPlus2x2 => "20G (USB 3 Gen 2x2)".into(),
            Self::Other(mbps) => format!("{mbps}M"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypecState {
    /// Whether UCSI registered any ports; without it Linux can't see USB-C state.
    pub ucsi_present: bool,
    pub ports: Vec<TypecPort>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypecPort {
    /// `port0`, `port1`, …
    pub name: String,
    pub label: Option<String>,
    pub partner: Option<Partner>,
    /// The cable's `type` (`passive`, `active`) when UCSI reports a cable.
    pub cable_type: Option<String>,
    /// The port's power role as UCSI reports it, `source` or `sink`. Recorded
    /// only: it can be wrong (see `pd_inconsistent`), so `charging` doesn't
    /// use it.
    #[serde(default)]
    pub power_role: Option<String>,
    /// Power is flowing in through this port (its ucsi-source-psy is online).
    pub charging: bool,
    /// The partner's PDOs and the port's power role disagree with its supply:
    /// the partner advertises source PDOs and the port says sink while the
    /// supply is offline, or the supply is online while the partner
    /// advertises no source PDOs. UCSI keeps the PDOs from when the partner
    /// registered, and they go stale: on the XPS 9320 (2026-10-16) port0 kept
    /// the dock's old 5-20 V PDOs and a sink role while power came in on
    /// port1, whose partner listed no source PDOs and whose role said source.
    /// `charging` and `pd_contract` follow the supply regardless. Only the
    /// first case is stale data; some chargers never report their PDOs.
    #[serde(default)]
    pub pd_inconsistent: bool,
    pub pd_contract: Option<PdContract>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Partner {
    /// `supports_usb_power_delivery`; can say no while a contract is in place.
    pub supports_pd: Option<bool>,
    pub pd_revision: Option<String>,
    pub alt_modes: u32,
    pub source_pdos: Vec<Pdo>,
    pub sink_pdos: Vec<Pdo>,
}

/// The negotiated PD contract, from the port's ucsi-source-psy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdContract {
    pub voltage_mv: u32,
    pub current_ma: u32,
    /// 1-based position of the partner's source PDO this contract runs on,
    /// when exactly one fixed PDO has the contract's voltage.
    #[serde(default)]
    pub source_pdo: Option<u8>,
}

impl PdContract {
    pub fn watts(&self) -> f32 {
        watts(self.voltage_mv, self.current_ma)
    }
}

/// A power data object: one supply (or demand) a PD partner advertises.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pdo {
    Fixed {
        mv: u32,
        ma: u32,
    },
    Variable {
        min_mv: u32,
        max_mv: u32,
        ma: u32,
    },
    Battery {
        min_mv: u32,
        max_mv: u32,
        mw: u32,
    },
    Pps {
        min_mv: u32,
        max_mv: u32,
        ma: u32,
    },
    /// A kind this version doesn't decode, by its sysfs directory name.
    Other(String),
}

impl Pdo {
    pub fn max_watts(&self) -> Option<f32> {
        match *self {
            Self::Fixed { mv, ma } => Some(watts(mv, ma)),
            Self::Variable { max_mv, ma, .. } | Self::Pps { max_mv, ma, .. } => {
                Some(watts(max_mv, ma))
            }
            Self::Battery { mw, .. } => Some(mw as f32 / 1000.0),
            Self::Other(_) => None,
        }
    }
}

fn watts(mv: u32, ma: u32) -> f32 {
    (f64::from(mv) * f64::from(ma) / 1e6) as f32
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thunderbolt {
    pub domains: Vec<TbDomain>,
    /// Device routers; the host router and XDomain services are left out.
    pub devices: Vec<TbDevice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TbDomain {
    /// N in `domainN`.
    pub index: u32,
    /// `none`, `user`, `secure`, `dponly`, …
    pub security: Option<String>,
    pub iommu_dma_protection: bool,
    /// The host router's vendor and device names.
    pub controller_vendor: Option<String>,
    pub controller_device: Option<String>,
    pub generation: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TbDevice {
    /// `<domain>-<route>`, e.g. `0-1`.
    pub route: String,
    pub unique_id: Option<String>,
    pub vendor: Option<String>,
    pub name: Option<String>,
    /// Without authorization there are no PCIe tunnels.
    pub authorized: bool,
    pub generation: Option<u32>,
    pub link: Option<TbLink>,
    /// boltd's enrollment (auto-authorized next time); `None` without boltd.
    pub boltd_stored: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TbLink {
    pub rx_gbps: f32,
    pub rx_lanes: u8,
    pub tx_gbps: f32,
    pub tx_lanes: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Display {
    /// DRM connector name without the card prefix, e.g. `DP-2`, `HDMI-A-1`.
    pub connector: String,
    pub kind: DisplayKind,
    /// Current mode as the compositor drives it, e.g. `2560x1440@59.951`.
    pub mode: Option<String>,
    pub vrr: Option<Vrr>,
}

/// `Dp` on a laptop is USB-C DP alt mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayKind {
    Dp,
    Hdmi,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vrr {
    On,
    Off,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Power {
    pub ac_online: Option<bool>,
    pub battery: Option<Battery>,
    /// Online ucsi-source-psy supplies: power arriving over USB-C.
    pub usb_c_sources: Vec<UsbCSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Battery {
    pub capacity_pct: Option<u8>,
    /// sysfs `status`: `Charging`, `Discharging`, `Not charging`, `Full`.
    pub status: Option<String>,
    pub charge_limit_pct: Option<u8>,
    /// Power into the battery in mW: positive charging, negative discharging.
    pub flow_mw: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsbCSource {
    pub typec_port: String,
    /// `voltage_now`; `None` when unreadable or 0 (not reported).
    pub voltage_mv: Option<u32>,
    /// `current_max`, else the magnitude of `current_now`; `None` when
    /// neither is readable and non-zero.
    pub current_ma: Option<u32>,
}

impl UsbCSource {
    /// `None` unless both voltage and current are known and non-zero.
    pub fn watts(&self) -> Option<f32> {
        let mv = self.voltage_mv.filter(|&v| v > 0)?;
        let ma = self.current_ma.filter(|&a| a > 0)?;
        Some(watts(mv, ma))
    }
}

/// CPU state over a short sampling window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuSample {
    pub util_pct: u8,
    pub avg_khz: u32,
    pub max_khz: u32,
    /// `cpuinfo_min_freq`: the floor a throttled CPU gets pinned to.
    pub min_khz: Option<u32>,
    /// RAPL package power; `None` where `energy_uj` is root-only.
    pub package_w: Option<f32>,
    pub package_temp_c: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelLog {
    /// USB, USB-C and link-training error lines this boot.
    pub errors: Vec<LogLine>,
    /// Known-harmless warnings (amdgpu LTTPR / REG_WAIT).
    pub benign_count: u32,
    /// USB link resets this boot, per socket label or port.
    pub link_resets: Vec<LinkResets>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    /// `None` when the line's timestamp couldn't be parsed.
    pub secs_since_boot: Option<u64>,
    pub message: String,
    /// Label of the socket the message names, when known.
    pub socket: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkResets {
    /// A socket label, `port <bus>-<port>`, or `unknown port`.
    pub place: String,
    pub count: u32,
}

/// One health verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub level: Level,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Ok,
    Warn,
    Fail,
}
