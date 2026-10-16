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
