//! Histogram-based latency collector for measuring WebSocket-to-local delay.
//!
//! Each connection maintains a `LatencyCollector` that records the difference
//! between the exchange event timestamp and local receive time. After a
//! configurable number of samples (or on demand), statistics are computed:
//! min, max, average, and percentiles (p50, p90, p99).
//!
//! The histogram uses fixed 10µs bins up to 30ms (3000 bins). Samples above
//! 30ms are clamped to the last bin.

/// Width of each histogram bin in microseconds.
const BIN_WIDTH_US: u64 = 10;

/// Number of histogram bins (covers 0–30ms).
const NUM_BINS: usize = 3000;

/// Computed latency statistics.
#[derive(Debug, Clone, Copy)]
pub struct LatencyStats {
    pub count: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub avg_us: f64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p99_us: u64,
}

impl std::fmt::Display for LatencyStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={} min={}µs max={}µs avg={:.1}µs p50={}µs p90={}µs p99={}µs",
            self.count, self.min_us, self.max_us, self.avg_us, self.p50_us, self.p90_us, self.p99_us,
        )
    }
}

/// A histogram-based latency collector.
///
/// Not thread-safe — each connection / dedup thread should own its own instance.
pub struct LatencyCollector {
    bins: Vec<u64>,
    count: u64,
    sum: u64,
    min: u64,
    max: u64,
}

impl LatencyCollector {
    /// Create a new, empty collector.
    pub fn new() -> Self {
        Self { bins: vec![0u64; NUM_BINS], count: 0, sum: 0, min: u64::MAX, max: 0 }
    }

    /// Record a latency sample in microseconds.
    #[inline]
    pub fn record(&mut self, latency_us: u64) {
        self.count += 1;
        self.sum += latency_us;
        self.min = self.min.min(latency_us);
        self.max = self.max.max(latency_us);

        let bin = (latency_us / BIN_WIDTH_US) as usize;
        let bin = bin.min(NUM_BINS - 1);
        self.bins[bin] += 1;
    }

    /// Returns the number of recorded samples.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Compute summary statistics. Returns `None` if no samples recorded.
    pub fn stats(&self) -> Option<LatencyStats> {
        if self.count == 0 {
            return None;
        }

        let avg = self.sum as f64 / self.count as f64;
        let p50 = self.percentile(0.50);
        let p90 = self.percentile(0.90);
        let p99 = self.percentile(0.99);

        Some(LatencyStats {
            count: self.count,
            min_us: self.min,
            max_us: self.max,
            avg_us: avg,
            p50_us: p50,
            p90_us: p90,
            p99_us: p99,
        })
    }

    /// Reset all counters and bins.
    pub fn reset(&mut self) {
        self.bins.fill(0);
        self.count = 0;
        self.sum = 0;
        self.min = u64::MAX;
        self.max = 0;
    }

    /// Compute the value at the given percentile (0.0–1.0).
    fn percentile(&self, pct: f64) -> u64 {
        let target = (self.count as f64 * pct).ceil() as u64;
        let mut cumulative = 0u64;
        for (i, &count) in self.bins.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return (i as u64) * BIN_WIDTH_US;
            }
        }
        // All samples are above the histogram range
        self.max
    }
}

impl Default for LatencyCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_stats() {
        let mut lc = LatencyCollector::new();
        for i in 0..100 {
            lc.record(i * 10); // 0, 10, 20, ..., 990 µs
        }
        let stats = lc.stats().unwrap();
        assert_eq!(stats.count, 100);
        assert_eq!(stats.min_us, 0);
        assert_eq!(stats.max_us, 990);
        assert!(stats.avg_us > 490.0 && stats.avg_us < 500.0);
    }

    #[test]
    fn empty_stats() {
        let lc = LatencyCollector::new();
        assert!(lc.stats().is_none());
    }

    #[test]
    fn reset_clears() {
        let mut lc = LatencyCollector::new();
        lc.record(100);
        lc.reset();
        assert_eq!(lc.count(), 0);
        assert!(lc.stats().is_none());
    }

    #[test]
    fn percentile_accuracy() {
        let mut lc = LatencyCollector::new();
        // Insert 100 samples: 10, 20, ..., 1000
        for i in 1..=100 {
            lc.record(i * 10);
        }
        let stats = lc.stats().unwrap();
        assert_eq!(stats.count, 100);
        assert_eq!(stats.min_us, 10);
        assert_eq!(stats.max_us, 1000);
        // p50 should be around 500 (bin 50 = 500µs)
        assert!(stats.p50_us >= 490 && stats.p50_us <= 510);
        // p99 should be around 990
        assert!(stats.p99_us >= 980 && stats.p99_us <= 1000);
    }

    #[test]
    fn high_latency_clamped() {
        let mut lc = LatencyCollector::new();
        lc.record(50_000); // 50ms — above 30ms histogram range
        let stats = lc.stats().unwrap();
        assert_eq!(stats.max_us, 50_000);
        assert_eq!(stats.count, 1);
    }
}
