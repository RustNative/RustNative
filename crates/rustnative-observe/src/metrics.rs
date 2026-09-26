//! Metrics: counters and histograms, rendered in the Prometheus text
//! format (what `rustnative-server` serves at `/metrics`).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// A monotonically increasing count.
#[derive(Debug, Clone, Default)]
pub struct Counter(Arc<AtomicU64>);

impl Counter {
    /// Adds `by`.
    pub fn add(&self, by: u64) {
        self.0.fetch_add(by, Ordering::Relaxed);
    }

    /// The count.
    #[must_use]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A distribution of values over fixed buckets.
#[derive(Debug, Clone)]
pub struct Histogram {
    bounds: Arc<Vec<f64>>,
    state: Arc<Mutex<(Vec<u64>, f64, u64)>>,
}

impl Histogram {
    /// A histogram with upper `bounds`.
    #[must_use]
    pub fn new(bounds: &[f64]) -> Self {
        Self {
            bounds: Arc::new(bounds.to_vec()),
            state: Arc::new(Mutex::new((vec![0; bounds.len()], 0.0, 0))),
        }
    }

    /// Records `value`.
    pub fn record(&self, value: f64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        for (index, bound) in self.bounds.iter().enumerate() {
            if value <= *bound {
                state.0[index] += 1;
            }
        }
        state.1 += value;
        state.2 += 1;
    }

    /// How many values were recorded.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).2
    }
}

/// A registry of named metrics.
#[derive(Debug, Clone, Default)]
pub struct Metrics {
    counters: Arc<Mutex<BTreeMap<String, Counter>>>,
    histograms: Arc<Mutex<BTreeMap<String, Histogram>>>,
}

impl Metrics {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The counter `name`, made on first use.
    #[must_use]
    pub fn counter(&self, name: &str) -> Counter {
        self.counters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(name.to_owned())
            .or_default()
            .clone()
    }

    /// The histogram `name`, made on first use with `bounds`.
    #[must_use]
    pub fn histogram(&self, name: &str, bounds: &[f64]) -> Histogram {
        self.histograms
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(name.to_owned())
            .or_insert_with(|| Histogram::new(bounds))
            .clone()
    }

    /// Everything, in the Prometheus text format.
    #[must_use]
    pub fn prometheus(&self) -> String {
        let mut text = String::new();
        for (name, counter) in self.counters.lock().unwrap_or_else(PoisonError::into_inner).iter() {
            let _ = writeln!(text, "# TYPE {name} counter\n{name} {}", counter.get());
        }
        for (name, histogram) in
            self.histograms.lock().unwrap_or_else(PoisonError::into_inner).iter()
        {
            let state = histogram.state.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(text, "# TYPE {name} histogram");
            for (bound, count) in histogram.bounds.iter().zip(&state.0) {
                let _ = writeln!(text, "{name}_bucket{{le=\"{bound}\"}} {count}");
            }
            let _ = writeln!(
                text,
                "{name}_bucket{{le=\"+Inf\"}} {}\n{name}_sum {}\n{name}_count {}",
                state.2, state.1, state.2
            );
        }
        text
    }
}
