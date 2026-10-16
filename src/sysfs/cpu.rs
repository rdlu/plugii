//! CPU utilisation, clocks, RAPL package power and package temperature.
//!
//! Utilisation and package power are rates, so they need two reads of the
//! counters: [`read_counters`] twice, then [`sample`]. [`measure`] does both
//! with a sleep in between; a ticking caller keeps the last counters instead.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::{Root, entries, read, read_parse};
use crate::model::CpuSample;

/// Cumulative CPU counters at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuCounters {
    /// The `cpu ` line of `/proc/stat` in jiffies: user nice system idle
    /// iowait irq softirq steal.
    pub stat: [u64; 8],
    /// RAPL `package-0` energy; `None` where it's root-only or absent.
    pub energy_uj: Option<u64>,
    /// Where `energy_uj` wraps back to 0.
    pub energy_range_uj: Option<u64>,
}

/// `None` when `/proc/stat` has no aggregate `cpu ` line.
pub fn read_counters(root: &Root) -> Option<CpuCounters> {
    let text = read(root.path("proc/stat"))?;
    let line = text.lines().find_map(|l| l.strip_prefix("cpu "))?;
    let mut stat = [0; 8];
    for (slot, field) in stat.iter_mut().zip(line.split_whitespace()) {
        *slot = field.parse().ok()?;
    }
    let zone = package_zone(root);
    Some(CpuCounters {
        stat,
        energy_uj: zone
            .as_deref()
            .and_then(|z| read_parse(z.join("energy_uj"))),
        energy_range_uj: zone
            .as_deref()
            .and_then(|z| read_parse(z.join("max_energy_range_uj"))),
    })
}

/// The powercap zone named `package-0`. The MSR one (`intel-rapl:0`) wins
/// over the MMIO one (`intel-rapl-mmio:0`) when both exist.
fn package_zone(root: &Root) -> Option<std::path::PathBuf> {
    let mut zones = entries(root.path("sys/class/powercap"), |_| true);
    zones.retain(|z| read(z.join("name")).as_deref() == Some("package-0"));
    zones.sort_by_key(|z| z.to_string_lossy().contains("mmio"));
    zones.into_iter().next()
}

/// The CPU between two counter reads `elapsed` apart, plus the clocks and
/// temperature now.
pub fn sample(
    root: &Root,
    before: &CpuCounters,
    after: &CpuCounters,
    elapsed: Duration,
) -> CpuSample {
    let (avg_khz, max_khz) = clocks(root);
    CpuSample {
        util_pct: util_pct(before, after),
        avg_khz,
        max_khz,
        min_khz: read_parse(root.path("sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq")),
        package_w: package_w(before, after, elapsed),
        package_temp_c: package_temp_c(root),
    }
}

/// Two counter reads `window` apart.
pub fn measure(root: &Root, window: Duration) -> Option<CpuSample> {
    let before = read_counters(root)?;
    let started = Instant::now();
    thread::sleep(window);
    let after = read_counters(root)?;
    Some(sample(root, &before, &after, started.elapsed()))
}

/// Busy share of the jiffies between the reads; idle and iowait aren't busy.
fn util_pct(before: &CpuCounters, after: &CpuCounters) -> u8 {
    let delta = |i: usize| after.stat[i].saturating_sub(before.stat[i]);
    let busy: u64 = [0, 1, 2, 5, 6, 7].into_iter().map(delta).sum();
    let idle = delta(3) + delta(4);
    match busy + idle {
        0 => 0,
        total => u8::try_from(100 * busy / total).unwrap_or(100),
    }
}

fn package_w(before: &CpuCounters, after: &CpuCounters, elapsed: Duration) -> Option<f32> {
    let (e1, e2) = (before.energy_uj?, after.energy_uj?);
    let delta = if e2 >= e1 {
        e2 - e1
    } else {
        let range = before.energy_range_uj?;
        range.checked_sub(e1)? + e2
    };
    let secs = elapsed.as_secs_f64();
    (secs > 0.0).then(|| (delta as f64 / secs / 1e6) as f32)
}

/// Average and highest `scaling_cur_freq` over the CPUs that report one.
fn clocks(root: &Root) -> (u32, u32) {
    let freqs: Vec<u64> = entries(root.path("sys/devices/system/cpu"), |name| {
        name.strip_prefix("cpu")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
    .into_iter()
    .filter_map(|cpu| read_parse(cpu.join("cpufreq/scaling_cur_freq")))
    .collect();
    if freqs.is_empty() {
        return (0, 0);
    }
    let avg = freqs.iter().sum::<u64>() / freqs.len() as u64;
    let max = freqs.iter().copied().max().unwrap_or(0);
    (
        u32::try_from(avg).unwrap_or(u32::MAX),
        u32::try_from(max).unwrap_or(u32::MAX),
    )
}

/// hwmon chips that report a CPU package temperature, and the labels to look
/// for on each, best first.
const PACKAGE_SENSORS: &[(&str, &[&str])] = &[
    ("coretemp", &["Package id 0"]),
    ("k10temp", &["Tctl", "Tdie"]),
    ("zenpower", &["Tctl", "Tdie"]),
];

fn package_temp_c(root: &Root) -> Option<f32> {
    entries(root.path("sys/class/hwmon"), |_| true)
        .into_iter()
        .find_map(|hwmon| {
            let name = read(hwmon.join("name"))?;
            let (_, labels) = PACKAGE_SENSORS.iter().find(|(n, _)| *n == name)?;
            labels.iter().find_map(|label| temp_labelled(&hwmon, label))
        })
}

/// `tempN_input` in °C for the `tempN_label` that reads `label`.
fn temp_labelled(hwmon: &Path, label: &str) -> Option<f32> {
    entries(hwmon, |name| {
        name.starts_with("temp") && name.ends_with("_label")
    })
    .into_iter()
    .find(|path| read(path).as_deref() == Some(label))
    .and_then(|path| {
        let input = path.to_string_lossy().replace("_label", "_input");
        read_parse::<i64>(input)
    })
    .map(|milli| milli as f32 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(stat: [u64; 8], energy_uj: Option<u64>) -> CpuCounters {
        CpuCounters {
            stat,
            energy_uj,
            energy_range_uj: Some(1000),
        }
    }

    #[test]
    fn util_rounds_down_like_the_script() {
        let a = counters([0; 8], None);
        let b = counters([1, 0, 1, 1, 0, 0, 0, 0], None);
        assert_eq!(util_pct(&a, &b), 66);
    }

    #[test]
    fn package_power() {
        let a = counters([0; 8], Some(900));
        let b = counters([0; 8], Some(400));
        // 100 to the top of the range, then 400.
        assert_eq!(package_w(&a, &b, Duration::from_micros(500)), Some(1.0));
        assert_eq!(package_w(&a, &b, Duration::ZERO), None);
        assert_eq!(
            package_w(&a, &counters([0; 8], None), Duration::from_secs(1)),
            None
        );
    }
}
