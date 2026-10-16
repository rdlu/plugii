//! Fixed-size history of timed samples, for the TUI's charts.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Something taken at a point in time.
pub trait Timed {
    /// Unix time in milliseconds.
    fn t_ms(&self) -> u64;
}

/// A ring buffer of the last `capacity` samples, oldest first. Samples are
/// expected to be pushed in time order.
#[derive(Debug, Clone)]
pub struct History<T> {
    samples: VecDeque<T>,
    capacity: usize,
}

impl<T: Timed> History<T> {
    /// # Panics
    ///
    /// If `capacity` is zero.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "History capacity must be at least 1");
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Adds a sample, dropping the oldest when full.
    pub fn push(&mut self, sample: T) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.samples.iter()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn latest(&self) -> Option<&T> {
        self.samples.back()
    }

    /// Samples taken at or after `t_ms`.
    pub fn since(&self, t_ms: u64) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        let start = self.samples.partition_point(|s| s.t_ms() < t_ms);
        self.samples.range(start..)
    }
}

/// One tick of the values the Charts tab plots; `None` where not readable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub t_ms: u64,
    /// Negotiated USB-C PD contract.
    pub pd_w: Option<f32>,
    /// Battery flow: positive charging, negative discharging.
    pub battery_w: Option<f32>,
    pub cpu_package_w: Option<f32>,
    /// Average clock across cores.
    pub cpu_ghz: Option<f32>,
    pub cpu_temp_c: Option<f32>,
}

impl Timed for Sample {
    fn t_ms(&self) -> u64 {
        self.t_ms
    }
}

impl Sample {
    /// The chart values in one snapshot: PD contract (or USB-C supply)
    /// watts, battery flow, and the CPU's package watts, clock and temperature.
    pub fn from_snapshot(snap: &crate::model::Snapshot) -> Self {
        let contract = snap
            .typec
            .ports
            .iter()
            .filter(|p| p.charging)
            .find_map(|p| p.pd_contract)
            .map(|c| c.watts());
        let supplies = snap
            .power
            .usb_c_sources
            .iter()
            .filter_map(|s| s.watts())
            .reduce(|a, b| a + b);
        let cpu = snap.cpu.as_ref();
        Self {
            t_ms: snap.taken_at_ms,
            pd_w: contract.or(supplies),
            battery_w: snap
                .power
                .battery
                .as_ref()
                .and_then(|b| b.flow_mw)
                .map(|mw| mw as f32 / 1000.0),
            cpu_package_w: cpu.and_then(|c| c.package_w),
            // 0 kHz: no CPU reported a clock.
            cpu_ghz: cpu
                .filter(|c| c.avg_khz > 0)
                .map(|c| (f64::from(c.avg_khz) / 1e6) as f32),
            cpu_temp_c: cpu.and_then(|c| c.package_temp_c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Battery, CpuSample, KernelLog, PdContract, Power, Snapshot, TypecPort, TypecState,
        UsbCSource,
    };

    fn snap() -> Snapshot {
        Snapshot {
            taken_at_ms: 1234,
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
            firmware: None,
        }
    }

    fn port(name: &str, charging: bool, contract: Option<(u32, u32)>) -> TypecPort {
        TypecPort {
            name: name.into(),
            label: None,
            partner: None,
            cable_type: None,
            power_role: None,
            charging,
            pd_inconsistent: false,
            pd_contract: contract.map(|(voltage_mv, current_ma)| PdContract {
                voltage_mv,
                current_ma,
                source_pdo: None,
            }),
        }
    }

    fn source(port: &str, mv: Option<u32>, ma: Option<u32>) -> UsbCSource {
        UsbCSource {
            typec_port: port.into(),
            voltage_mv: mv,
            current_ma: ma,
        }
    }

    #[test]
    fn nothing_readable() {
        assert_eq!(
            Sample::from_snapshot(&snap()),
            Sample {
                t_ms: 1234,
                ..Sample::default()
            }
        );
    }

    #[test]
    fn pd_watts_from_the_charging_ports_contract() {
        let mut s = snap();
        s.typec.ports = vec![
            port("port0", false, Some((5000, 3000))),
            port("port1", true, Some((20000, 2250))),
        ];
        s.power.usb_c_sources = vec![source("port1", Some(20000), Some(3000))];
        assert_eq!(Sample::from_snapshot(&s).pd_w, Some(45.0));
    }

    #[test]
    fn pd_watts_fall_back_to_the_supplies() {
        let mut s = snap();
        s.typec.ports = vec![port("port0", true, None)];
        s.power.usb_c_sources = vec![
            source("port0", Some(20000), Some(1500)),
            source("port1", Some(5000), Some(2000)),
            // Lenovo's UCSI: no voltage, so no watts.
            source("port2", None, Some(3250)),
        ];
        assert_eq!(Sample::from_snapshot(&s).pd_w, Some(40.0));
        s.power.usb_c_sources.truncate(0);
        s.power
            .usb_c_sources
            .push(source("port2", None, Some(3250)));
        assert_eq!(Sample::from_snapshot(&s).pd_w, None);
    }

    #[test]
    fn battery_and_cpu() {
        let mut s = snap();
        s.power.battery = Some(Battery {
            capacity_pct: Some(80),
            status: Some("Discharging".into()),
            charge_limit_pct: None,
            flow_mw: Some(-12500),
        });
        s.cpu = Some(CpuSample {
            util_pct: 50,
            avg_khz: 2_400_000,
            max_khz: 4_000_000,
            min_khz: None,
            package_w: Some(15.5),
            package_temp_c: Some(61.0),
        });
        let sample = Sample::from_snapshot(&s);
        assert_eq!(sample.battery_w, Some(-12.5));
        assert_eq!(sample.cpu_package_w, Some(15.5));
        assert_eq!(sample.cpu_ghz, Some(2.4));
        assert_eq!(sample.cpu_temp_c, Some(61.0));
        s.cpu.as_mut().unwrap().avg_khz = 0;
        assert_eq!(Sample::from_snapshot(&s).cpu_ghz, None);
    }
}
