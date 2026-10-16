//! What the Charts tab's lower panes plot, built up one snapshot at a time:
//! URB/s and the linked speed per socket, and a row per charger seen this
//! session. Pure state; [`super::charts`] draws it.

use std::collections::{HashMap, VecDeque};

use crate::health::{hub_stuck_at_usb2, port_name, socket_name};
use crate::model::{Pdo, Snapshot, Socket, TypecPort, Usb3Side, UsbDevice};

use super::app::HISTORY_LEN;

/// URB rates kept per socket: two minutes at the watcher's 1 s tick.
pub const URB_RATES: usize = 120;

#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// In the order sockets were first seen.
    pub sockets: Vec<SocketStats>,
    /// In the order chargers were first seen.
    pub chargers: Vec<ChargerRow>,
    /// The row of the charger charging now.
    pub active: Option<usize>,
    /// The previous snapshot's time and link-reset counts per place.
    prev: Option<(u64, HashMap<String, u32>)>,
}

#[derive(Debug, Clone)]
pub struct SocketStats {
    pub key: String,
    /// Its label, else its key.
    pub name: String,
    /// URB/s between consecutive snapshots, oldest first.
    pub rates: VecDeque<f64>,
    /// The socket's link at each snapshot, oldest first.
    pub links: VecDeque<(u64, Link)>,
    /// The time and URB counters at the last snapshot, per device.
    urbs: Option<(u64, HashMap<DeviceId, u64>)>,
}

/// A device for URB counting: the same sysfs name with other ids is
/// another device, whose counter has nothing to do with the old one's.
type DeviceId = (String, u16, u16);

/// A socket's link, from the faster half's top device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    None,
    /// Linked below 5G: USB 2 (or 1).
    Usb2,
    /// 5G or more.
    SuperSpeed,
    /// A USB 3 hub that only came up on the USB 2 half.
    Stuck,
}

/// One charger: a USB-C port plus what the partner offers.
#[derive(Debug, Clone, PartialEq)]
pub struct ChargerId {
    /// The UCSI port name, `port0`.
    pub port: String,
    pub offers: Offers,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Offers {
    /// The partner's source PDOs, sorted by their maximum watts.
    Pdos(Vec<Pdo>),
    /// The partner lists no source PDOs (Lenovo's UCSI): the most current
    /// the supply reported while charging is the only hint to tell two
    /// such chargers apart.
    NotReported { max_ma: Option<u32> },
}

/// One charger's line in the comparison table. Replugging the same charger
/// (same port and offers, or same port and current when the offers aren't
/// reported) carries on its row rather than starting another: the table
/// compares chargers, not plug-ins.
#[derive(Debug, Clone, PartialEq)]
pub struct ChargerRow {
    pub id: ChargerId,
    /// The port's label, else its name.
    pub port_label: String,
    /// The partner's best offer, when it reports any.
    pub best_w: Option<f32>,
    /// What was negotiated at the last snapshot.
    pub negotiated: Negotiated,
    pub max_ghz: Option<f32>,
    pub max_package_w: Option<f32>,
    /// Link resets (any socket) that showed up while charging.
    pub link_resets: u32,
    /// Time spent charging from it, in ms.
    pub charging_ms: u64,
    /// Times charging started from it.
    pub sessions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Negotiated {
    /// A PD contract, or a supply reporting volts and amps.
    Watts(f32),
    /// The supply only reports its current.
    Drawing {
        ma: u32,
    },
    Unknown,
}

impl Stats {
    /// Takes the next snapshot; snapshots are expected in time order.
    pub fn observe(&mut self, snap: &Snapshot) {
        let t = snap.taken_at_ms;
        for socket in &snap.sockets {
            let stats = match self.sockets.iter().position(|s| s.key == socket.key) {
                Some(i) => &mut self.sockets[i],
                None => {
                    self.sockets.push(SocketStats::new(&socket.key));
                    self.sockets.last_mut().expect("just pushed")
                }
            };
            stats.observe(t, socket);
        }

        let resets: HashMap<String, u32> = snap
            .kernel_log
            .link_resets
            .iter()
            .map(|r| (r.place.clone(), r.count))
            .collect();
        let (dt, new_resets) = match &self.prev {
            Some((t0, before)) => (
                t.saturating_sub(*t0),
                resets
                    .iter()
                    .map(|(place, n)| n.saturating_sub(before.get(place).copied().unwrap_or(0)))
                    .sum(),
            ),
            None => (0, 0),
        };
        self.prev = Some((t, resets));
        self.charging(snap, dt, new_resets);
    }

    /// The charger table: continue the active row, resume a known charger's
    /// row, or start a new one.
    fn charging(&mut self, snap: &Snapshot, dt: u64, new_resets: u32) {
        let Some(port) = snap.typec.ports.iter().find(|p| p.charging) else {
            self.active = None;
            return;
        };
        let id = charger_id(snap, port);
        let continuing = self
            .active
            .filter(|&i| same_session(&self.chargers[i].id, &id));
        let i = match continuing {
            Some(i) => {
                let row = &mut self.chargers[i];
                row.charging_ms += dt;
                row.link_resets += new_resets;
                i
            }
            None => {
                let i = match self.chargers.iter().position(|r| r.id == id) {
                    Some(i) => i,
                    None => {
                        self.chargers.push(ChargerRow::new(id.clone()));
                        self.chargers.len() - 1
                    }
                };
                let row = &mut self.chargers[i];
                row.sessions += 1;
                // The interval that brought the charger in: plugging in is
                // when resets tend to happen.
                row.link_resets += new_resets;
                i
            }
        };
        self.active = Some(i);
        let row = &mut self.chargers[i];
        if let Offers::NotReported { max_ma } = &mut row.id.offers
            && let Offers::NotReported { max_ma: now } = id.offers
        {
            *max_ma = (*max_ma).max(now);
        }
        row.port_label = port_name(port).to_owned();
        row.best_w = match &row.id.offers {
            Offers::Pdos(pdos) => pdos.iter().filter_map(Pdo::max_watts).reduce(f32::max),
            Offers::NotReported { .. } => None,
        };
        row.negotiated = negotiated(snap, port);
        if let Some(cpu) = &snap.cpu {
            if cpu.avg_khz > 0 {
                let ghz = (f64::from(cpu.avg_khz) / 1e6) as f32;
                row.max_ghz = Some(row.max_ghz.map_or(ghz, |g| g.max(ghz)));
            }
            if let Some(w) = cpu.package_w {
                row.max_package_w = Some(row.max_package_w.map_or(w, |m| m.max(w)));
            }
        }
    }
}

impl SocketStats {
    fn new(key: &str) -> Self {
        Self {
            key: key.to_owned(),
            name: key.to_owned(),
            rates: VecDeque::with_capacity(URB_RATES),
            links: VecDeque::with_capacity(HISTORY_LEN),
            urbs: None,
        }
    }

    fn observe(&mut self, t: u64, socket: &Socket) {
        socket_name(socket).clone_into(&mut self.name);
        if self.links.len() == HISTORY_LEN {
            self.links.pop_front();
        }
        self.links.push_back((t, link(socket)));

        let mut now = HashMap::new();
        for half in halves(socket) {
            urb_counters(half, &mut now);
        }
        if let Some((t0, before)) = &self.urbs
            && t > *t0
        {
            let rate = urb_rate(before, &now, t - t0);
            if self.rates.len() == URB_RATES {
                self.rates.pop_front();
            }
            self.rates.push_back(rate);
        }
        self.urbs = Some((t, now));
    }

    /// The newest rate.
    pub fn rate(&self) -> Option<f64> {
        self.rates.back().copied()
    }

    /// The link at `t_ms`: the last state seen at or before it.
    pub fn link_at(&self, t_ms: u64) -> Option<Link> {
        let i = self.links.partition_point(|&(t, _)| t <= t_ms);
        i.checked_sub(1).map(|i| self.links[i].1)
    }
}

impl ChargerRow {
    fn new(id: ChargerId) -> Self {
        Self {
            port_label: id.port.clone(),
            id,
            best_w: None,
            negotiated: Negotiated::Unknown,
            max_ghz: None,
            max_package_w: None,
            link_resets: 0,
            charging_ms: 0,
            sessions: 0,
        }
    }
}

/// URBs per second between two sets of counters taken `dt_ms` apart.
/// Only devices in both count: one arriving or leaving adds nothing, and a
/// counter that went down (the device re-enumerated) adds nothing either.
pub fn urb_rate(before: &HashMap<DeviceId, u64>, now: &HashMap<DeviceId, u64>, dt_ms: u64) -> f64 {
    if dt_ms == 0 {
        return 0.0;
    }
    let urbs: u64 = now
        .iter()
        .filter_map(|(id, &n)| n.checked_sub(*before.get(id)?))
        .sum();
    urbs as f64 * 1000.0 / dt_ms as f64
}

fn halves(socket: &Socket) -> impl Iterator<Item = &UsbDevice> {
    let usb3 = match &socket.usb3 {
        Usb3Side::Paired(half) => half.device.as_ref(),
        _ => None,
    };
    usb3.into_iter().chain(socket.usb2.device.as_ref())
}

/// Every device's URB counter in a tree.
fn urb_counters(d: &UsbDevice, out: &mut HashMap<DeviceId, u64>) {
    if let Some(n) = d.urbnum {
        out.insert((d.sysfs_name.clone(), d.vendor_id, d.product_id), n);
    }
    for child in &d.children {
        urb_counters(child, out);
    }
}

/// The faster half's top device decides, unless a hub is stuck at USB 2.
/// One row per socket rather than per half: it is one plug, and the stuck
/// verdict needs both halves anyway.
pub fn link(socket: &Socket) -> Link {
    if hub_stuck_at_usb2(socket) {
        return Link::Stuck;
    }
    match halves(socket).map(|d| d.speed.mbps()).reduce(f32::max) {
        None => Link::None,
        Some(mbps) if mbps >= 5000.0 => Link::SuperSpeed,
        Some(_) => Link::Usb2,
    }
}

/// The charger on `port`: its sorted offers, or the current its supply
/// reports when it lists none.
pub fn charger_id(snap: &Snapshot, port: &TypecPort) -> ChargerId {
    let mut pdos = port
        .partner
        .as_ref()
        .map(|p| p.source_pdos.clone())
        .unwrap_or_default();
    let offers = if pdos.is_empty() {
        Offers::NotReported {
            max_ma: supply(snap, port).and_then(|s| s.current_ma),
        }
    } else {
        pdos.sort_by(|a, b| {
            let w = |p: &Pdo| p.max_watts().unwrap_or(0.0);
            w(a).total_cmp(&w(b))
        });
        Offers::Pdos(pdos)
    };
    ChargerId {
        port: port.name.clone(),
        offers,
    }
}

/// Whether `now` is still the charger `active` stands for. Unreported
/// offers keep a session going when only the current changes.
fn same_session(active: &ChargerId, now: &ChargerId) -> bool {
    active.port == now.port
        && match (&active.offers, &now.offers) {
            (Offers::NotReported { .. }, Offers::NotReported { .. }) => true,
            (a, b) => a == b,
        }
}

fn supply<'a>(snap: &'a Snapshot, port: &TypecPort) -> Option<&'a crate::model::UsbCSource> {
    snap.power
        .usb_c_sources
        .iter()
        .find(|s| s.typec_port == port.name)
}

fn negotiated(snap: &Snapshot, port: &TypecPort) -> Negotiated {
    if let Some(c) = port.pd_contract {
        return Negotiated::Watts(c.watts());
    }
    let Some(src) = supply(snap, port) else {
        return Negotiated::Unknown;
    };
    match (src.watts(), src.current_ma) {
        (Some(w), _) => Negotiated::Watts(w),
        (None, Some(ma)) => Negotiated::Drawing { ma },
        (None, None) => Negotiated::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        CpuSample, Half, KernelLog, LinkResets, Partner, PdContract, Power, Speed, TypecState,
        UsbCSource,
    };

    fn snap(t: u64) -> Snapshot {
        Snapshot {
            taken_at_ms: t,
            host: "test".into(),
            sockets: Vec::new(),
            typec: TypecState {
                ucsi_present: true,
                ports: Vec::new(),
            },
            thunderbolt: None,
            displays: Vec::new(),
            power: Power {
                ac_online: None,
                battery: None,
                usb_c_sources: Vec::new(),
            },
            cpu: None,
            kernel_log: KernelLog {
                errors: Vec::new(),
                benign_count: 0,
                link_resets: Vec::new(),
            },
        }
    }

    fn device(name: &str, speed: Speed, class: u8, urbs: u64) -> UsbDevice {
        UsbDevice {
            sysfs_name: name.into(),
            vendor_id: 0x1234,
            product_id: 0x5678,
            manufacturer: None,
            product: None,
            serial: None,
            label: None,
            speed,
            capable_speed: None,
            rx_lanes: None,
            tx_lanes: None,
            bcd_usb: None,
            device_class: class,
            max_power_ma: None,
            driver: None,
            urbnum: Some(urbs),
            children: Vec::new(),
        }
    }

    fn half(bus: u32, device: Option<UsbDevice>) -> Half {
        Half {
            bus,
            port: 1,
            device,
            over_current_count: None,
            port_state: None,
            physical_location: None,
        }
    }

    fn socket(usb3: Option<UsbDevice>, usb2: Option<UsbDevice>) -> Socket {
        Socket {
            key: "0000:00:14.0-1".into(),
            label: Some("left".into()),
            usb2: half(3, usb2),
            usb3: Usb3Side::Paired(half(2, usb3)),
            typec_port: None,
        }
    }

    fn with_socket(t: u64, s: Socket) -> Snapshot {
        let mut snap = snap(t);
        snap.sockets.push(s);
        snap
    }

    fn rates(stats: &Stats) -> Vec<f64> {
        stats.sockets[0].rates.iter().copied().collect()
    }

    #[test]
    fn urb_rate_is_the_counter_delta_per_second() {
        let mut stats = Stats::default();
        let hub = |urbs, child_urbs| {
            let mut hub = device("2-1", Speed::Super, 9, urbs);
            hub.children
                .push(device("2-1.1", Speed::Super, 0, child_urbs));
            hub
        };
        stats.observe(&with_socket(0, socket(Some(hub(10, 100)), None)));
        assert!(stats.sockets[0].rates.is_empty());
        stats.observe(&with_socket(1000, socket(Some(hub(12, 200)), None)));
        stats.observe(&with_socket(3000, socket(Some(hub(14, 400)), None)));
        assert_eq!(rates(&stats), [102.0, 101.0]);
        assert_eq!(stats.sockets[0].rate(), Some(101.0));
    }

    #[test]
    fn arrivals_and_departures_make_no_spikes() {
        let mut stats = Stats::default();
        let mouse = |urbs| device("3-1", Speed::Full, 0, urbs);
        let disk = |urbs| device("2-1", Speed::Super, 0, urbs);
        stats.observe(&with_socket(0, socket(None, Some(mouse(50)))));
        // A disk arrives with a big counter: it counts from the next scan.
        stats.observe(&with_socket(
            1000,
            socket(Some(disk(90_000)), Some(mouse(60))),
        ));
        stats.observe(&with_socket(
            2000,
            socket(Some(disk(90_500)), Some(mouse(70))),
        ));
        // It leaves: no negative rate.
        stats.observe(&with_socket(3000, socket(None, Some(mouse(80)))));
        // Everything leaves.
        stats.observe(&with_socket(4000, socket(None, None)));
        assert_eq!(rates(&stats), [10.0, 510.0, 10.0, 0.0]);
    }

    #[test]
    fn counter_reset_adds_nothing() {
        let mut stats = Stats::default();
        let d = |urbs| device("2-1", Speed::Super, 0, urbs);
        stats.observe(&with_socket(0, socket(Some(d(5000)), None)));
        // Re-enumerated under the same name between two scans.
        stats.observe(&with_socket(1000, socket(Some(d(12)), None)));
        stats.observe(&with_socket(2000, socket(Some(d(32)), None)));
        // Another device under the old name: a fresh counter, not a delta.
        let mut other = d(40);
        other.product_id = 0x9999;
        stats.observe(&with_socket(3000, socket(Some(other), None)));
        assert_eq!(rates(&stats), [0.0, 20.0, 0.0]);
    }

    #[test]
    fn same_time_twice_makes_no_rate() {
        let mut stats = Stats::default();
        let d = |urbs| device("2-1", Speed::Super, 0, urbs);
        stats.observe(&with_socket(1000, socket(Some(d(1)), None)));
        stats.observe(&with_socket(1000, socket(Some(d(500)), None)));
        assert!(rates(&stats).is_empty());
    }

    #[test]
    fn rates_keep_the_last_two_minutes() {
        let mut stats = Stats::default();
        for i in 0..200u64 {
            let d = device("2-1", Speed::Super, 0, i * i);
            stats.observe(&with_socket(i * 1000, socket(Some(d), None)));
        }
        let r = rates(&stats);
        assert_eq!(r.len(), URB_RATES);
        assert_eq!(r.last(), Some(&(199.0 * 199.0 - 198.0 * 198.0)));
    }

    #[test]
    fn links() {
        let hub2 = device("3-1", Speed::High, 9, 0);
        assert_eq!(link(&socket(None, None)), Link::None);
        assert_eq!(
            link(&socket(None, Some(device("3-1", Speed::High, 0, 0)))),
            Link::Usb2
        );
        assert_eq!(link(&socket(None, Some(hub2.clone()))), Link::Stuck);
        let hub3 = device("2-1", Speed::SuperPlus, 9, 0);
        assert_eq!(link(&socket(Some(hub3), Some(hub2))), Link::SuperSpeed);

        let mut stats = Stats::default();
        stats.observe(&with_socket(1000, socket(None, None)));
        stats.observe(&with_socket(
            2000,
            socket(None, Some(device("3-1", Speed::Full, 0, 0))),
        ));
        let s = &stats.sockets[0];
        assert_eq!(s.link_at(999), None);
        assert_eq!(s.link_at(1500), Some(Link::None));
        assert_eq!(s.link_at(2000), Some(Link::Usb2));
    }

    fn fixed(v: u32, a: u32) -> Pdo {
        Pdo::Fixed { mv: v, ma: a }
    }

    /// A snapshot charging on `port0` from a partner offering `pdos`, with
    /// the supply at `ma` (and 20 V when `pdos` isn't empty).
    fn charging(t: u64, pdos: &[Pdo], ma: u32) -> Snapshot {
        let mut s = snap(t);
        s.typec.ports.push(TypecPort {
            name: "port0".into(),
            label: Some("USB-C (left)".into()),
            partner: Some(Partner {
                supports_pd: Some(true),
                pd_revision: None,
                alt_modes: 0,
                source_pdos: pdos.to_vec(),
                sink_pdos: Vec::new(),
            }),
            cable_type: None,
            power_role: Some("sink".into()),
            charging: true,
            pd_inconsistent: false,
            pd_contract: (!pdos.is_empty()).then_some(PdContract {
                voltage_mv: 20_000,
                current_ma: ma,
                source_pdo: None,
            }),
        });
        s.power.usb_c_sources.push(UsbCSource {
            typec_port: "port0".into(),
            voltage_mv: (!pdos.is_empty()).then_some(20_000),
            current_ma: Some(ma),
        });
        s
    }

    fn unplugged(t: u64) -> Snapshot {
        let mut s = snap(t);
        s.typec.ports.push(TypecPort {
            name: "port0".into(),
            label: Some("USB-C (left)".into()),
            partner: None,
            cable_type: None,
            power_role: None,
            charging: false,
            pd_inconsistent: false,
            pd_contract: None,
        });
        s
    }

    #[test]
    fn charger_identity_sorts_offers() {
        let a = charging(0, &[fixed(20_000, 3250), fixed(5000, 3000)], 3250);
        let b = charging(0, &[fixed(5000, 3000), fixed(20_000, 3250)], 3250);
        let id = |s: &Snapshot| charger_id(s, &s.typec.ports[0]);
        assert_eq!(id(&a), id(&b));
        assert_eq!(
            id(&a).offers,
            Offers::Pdos(vec![fixed(5000, 3000), fixed(20_000, 3250)])
        );
        let lenovo = charging(0, &[], 3250);
        assert_eq!(
            id(&lenovo).offers,
            Offers::NotReported { max_ma: Some(3250) }
        );
    }

    #[test]
    fn a_replugged_charger_carries_on_its_row() {
        let pdos = [fixed(5000, 3000), fixed(20_000, 3250)];
        let mut stats = Stats::default();
        stats.observe(&unplugged(0));
        stats.observe(&charging(1000, &pdos, 3250));
        stats.observe(&charging(3000, &pdos, 3250));
        stats.observe(&unplugged(4000));
        assert_eq!(stats.active, None);
        stats.observe(&charging(5000, &pdos, 3250));
        stats.observe(&charging(6000, &pdos, 3250));
        assert_eq!(stats.chargers.len(), 1);
        let row = &stats.chargers[0];
        assert_eq!(row.sessions, 2);
        // 1→3 s and 5→6 s; the unplugged gap isn't counted.
        assert_eq!(row.charging_ms, 3000);
        assert_eq!(row.best_w, Some(65.0));
        assert_eq!(row.negotiated, Negotiated::Watts(65.0));
        assert_eq!(row.port_label, "USB-C (left)");
        assert_eq!(stats.active, Some(0));
    }

    #[test]
    fn other_offers_start_a_row() {
        let mut stats = Stats::default();
        stats.observe(&charging(
            0,
            &[fixed(5000, 3000), fixed(20_000, 3250)],
            3250,
        ));
        // Swapped without a gap: still a different charger.
        stats.observe(&charging(
            1000,
            &[fixed(5000, 3000), fixed(20_000, 2250)],
            2250,
        ));
        stats.observe(&unplugged(2000));
        stats.observe(&charging(
            3000,
            &[fixed(5000, 3000), fixed(20_000, 3250)],
            3250,
        ));
        assert_eq!(stats.chargers.len(), 2);
        assert_eq!(stats.chargers[0].sessions, 2);
        assert_eq!(stats.chargers[1].best_w, Some(45.0));
        assert_eq!(stats.active, Some(0));
    }

    #[test]
    fn unreported_offers_go_by_port_and_current() {
        let mut stats = Stats::default();
        stats.observe(&charging(0, &[], 2750));
        // The current moving mid-session doesn't make another charger.
        stats.observe(&charging(1000, &[], 3000));
        stats.observe(&unplugged(2000));
        // Another charger on the same port, telling only its current.
        stats.observe(&charging(3000, &[], 3250));
        stats.observe(&unplugged(4000));
        // The first one again: its highest current matches.
        stats.observe(&charging(5000, &[], 3000));
        assert_eq!(stats.chargers.len(), 2);
        assert_eq!(
            stats.chargers[0].id.offers,
            Offers::NotReported { max_ma: Some(3000) }
        );
        assert_eq!(stats.chargers[0].sessions, 2);
        assert_eq!(stats.chargers[0].best_w, None);
        assert_eq!(
            stats.chargers[1].negotiated,
            Negotiated::Drawing { ma: 3250 }
        );
        assert_eq!(stats.active, Some(0));
    }

    #[test]
    fn cpu_and_resets_while_charging() {
        let pdos = [fixed(20_000, 2250)];
        let resets = |s: &mut Snapshot, n| {
            s.kernel_log.link_resets = vec![LinkResets {
                place: "left".into(),
                count: n,
            }]
        };
        let cpu = |s: &mut Snapshot, khz, w| {
            s.cpu = Some(CpuSample {
                util_pct: 50,
                avg_khz: khz,
                max_khz: 5_000_000,
                min_khz: None,
                package_w: Some(w),
                package_temp_c: None,
            })
        };
        let mut stats = Stats::default();
        let mut s = unplugged(0);
        resets(&mut s, 3);
        cpu(&mut s, 4_500_000, 40.0);
        stats.observe(&s);
        // Plugging in brings a reset with it.
        let mut s = charging(1000, &pdos, 2250);
        resets(&mut s, 4);
        cpu(&mut s, 1_200_000, 8.0);
        stats.observe(&s);
        let mut s = charging(2000, &pdos, 2250);
        resets(&mut s, 6);
        cpu(&mut s, 3_100_000, 25.5);
        stats.observe(&s);
        let mut s = unplugged(3000);
        resets(&mut s, 9);
        cpu(&mut s, 4_900_000, 50.0);
        stats.observe(&s);
        let row = &stats.chargers[0];
        assert_eq!(row.link_resets, 3);
        assert_eq!(row.max_ghz, Some(3.1));
        assert_eq!(row.max_package_w, Some(25.5));
    }
}
