//! Lightweight latency histogram for Prometheus exposition.
//!
//! Records latencies in pre-defined buckets (microseconds).
//! No external crate needed — just atomic counters per bucket.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Histogram buckets in microseconds.
const BUCKETS_US: &[u64] = &[
    100,     // 0.1ms
    250,     // 0.25ms
    500,     // 0.5ms
    1_000,   // 1ms
    2_500,   // 2.5ms
    5_000,   // 5ms
    10_000,  // 10ms
    25_000,  // 25ms
    50_000,  // 50ms
    100_000, // 100ms
    250_000, // 250ms
    500_000, // 500ms
];

pub struct Histogram {
    name: &'static str,
    help: &'static str,
    buckets: [AtomicU64; 12], // matches BUCKETS_US.len()
    count: AtomicU64,
    sum_us: AtomicU64,
}

impl Histogram {
    pub const fn new(name: &'static str, help: &'static str) -> Self {
        // Can't use array::from_fn in const, so manual init
        Self {
            name,
            help,
            buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            count: AtomicU64::new(0),
            sum_us: AtomicU64::new(0),
        }
    }

    /// Record a latency in microseconds.
    pub fn observe_us(&self, us: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_us.fetch_add(us, Ordering::Relaxed);
        for (i, &threshold) in BUCKETS_US.iter().enumerate() {
            if us <= threshold {
                self.buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Record duration from an Instant.
    pub fn observe_since(&self, start: Instant) {
        self.observe_us(start.elapsed().as_micros() as u64);
    }

    /// Start a timer that records on drop.
    pub fn start_timer(&self) -> Timer<'_> {
        Timer {
            histogram: self,
            start: Instant::now(),
        }
    }

    /// Format as Prometheus text.
    pub fn format_prometheus(&self, out: &mut String) {
        out.push_str(&format!("# HELP {} {}\n", self.name, self.help));
        out.push_str(&format!("# TYPE {} histogram\n", self.name));

        let mut cumulative = 0u64;
        for (i, &threshold) in BUCKETS_US.iter().enumerate() {
            cumulative += self.buckets[i].load(Ordering::Relaxed);
            let le = threshold as f64 / 1_000_000.0; // convert µs to seconds
            out.push_str(&format!(
                "{}_bucket{{le=\"{:.6}\"}} {}\n",
                self.name, le, cumulative
            ));
        }
        out.push_str(&format!(
            "{}_bucket{{le=\"+Inf\"}} {}\n",
            self.name,
            self.count.load(Ordering::Relaxed)
        ));
        out.push_str(&format!(
            "{}_sum {:.6}\n",
            self.name,
            self.sum_us.load(Ordering::Relaxed) as f64 / 1_000_000.0
        ));
        out.push_str(&format!(
            "{}_count {}\n",
            self.name,
            self.count.load(Ordering::Relaxed)
        ));
    }

    /// Get p50, p95, p99 estimates from bucket boundaries.
    pub fn percentiles(&self) -> (f64, f64, f64) {
        let total = self.count.load(Ordering::Relaxed);
        if total == 0 {
            return (0.0, 0.0, 0.0);
        }

        let mut cumulative = 0u64;
        let mut p50 = 0.0f64;
        let mut p95 = 0.0f64;
        let mut p99 = 0.0f64;
        let mut found_p50 = false;
        let mut found_p95 = false;
        let mut found_p99 = false;

        for (i, &threshold) in BUCKETS_US.iter().enumerate() {
            cumulative += self.buckets[i].load(Ordering::Relaxed);
            let pct = cumulative as f64 / total as f64;

            if !found_p50 && pct >= 0.50 {
                p50 = threshold as f64 / 1000.0; // µs → ms
                found_p50 = true;
            }
            if !found_p95 && pct >= 0.95 {
                p95 = threshold as f64 / 1000.0;
                found_p95 = true;
            }
            if !found_p99 && pct >= 0.99 {
                p99 = threshold as f64 / 1000.0;
                found_p99 = true;
            }
        }

        (p50, p95, p99)
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn avg_ms(&self) -> f64 {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        (self.sum_us.load(Ordering::Relaxed) as f64 / count as f64) / 1000.0
    }
}

/// Timer that records latency on drop.
pub struct Timer<'a> {
    histogram: &'a Histogram,
    start: Instant,
}

impl Drop for Timer<'_> {
    fn drop(&mut self) {
        self.histogram.observe_since(self.start);
    }
}
