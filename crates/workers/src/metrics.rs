//! Worker pool metrics — atomic counters + EMA latency accumulators.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use parking_lot::Mutex;

#[derive(Debug, Clone, Default)]
pub struct PoolMetrics {
    pub queue_depth:      usize,
    pub active_workers:   usize,
    pub completed_total:  u64,
    pub deadline_dropped: u64,
    pub queue_rejected:   u64,
    pub avg_wait_ms:      f64,
    pub avg_run_ms:       f64,
}

#[derive(Default)]
struct AvgAccum {
    value:   f64,
    samples: u64,
}

impl AvgAccum {
    fn observe(&mut self, x: f64) {
        const ALPHA: f64 = 0.1; // recent samples weigh ~10%
        self.value = if self.samples == 0 {
            x
        } else {
            ALPHA * x + (1.0 - ALPHA) * self.value
        };
        self.samples += 1;
    }
}

pub(crate) struct MetricsState {
    pub queue_depth:      AtomicUsize,
    pub active_workers:   AtomicUsize,
    pub completed_total:  AtomicU64,
    pub deadline_dropped: AtomicU64,
    pub queue_rejected:   AtomicU64,
    pub wait_ms:          Mutex<AvgAccum>,
    pub run_ms:           Mutex<AvgAccum>,
}

impl MetricsState {
    pub fn new() -> Self {
        Self {
            queue_depth:      AtomicUsize::new(0),
            active_workers:   AtomicUsize::new(0),
            completed_total:  AtomicU64::new(0),
            deadline_dropped: AtomicU64::new(0),
            queue_rejected:   AtomicU64::new(0),
            wait_ms:          Mutex::new(AvgAccum::default()),
            run_ms:           Mutex::new(AvgAccum::default()),
        }
    }

    pub fn observe_wait(&self, ms: f64) { self.wait_ms.lock().observe(ms); }
    pub fn observe_run(&self, ms: f64)  { self.run_ms.lock().observe(ms);  }

    pub fn snapshot(&self) -> PoolMetrics {
        PoolMetrics {
            queue_depth:      self.queue_depth.load(Ordering::Relaxed),
            active_workers:   self.active_workers.load(Ordering::Relaxed),
            completed_total:  self.completed_total.load(Ordering::Relaxed),
            deadline_dropped: self.deadline_dropped.load(Ordering::Relaxed),
            queue_rejected:   self.queue_rejected.load(Ordering::Relaxed),
            avg_wait_ms:      self.wait_ms.lock().value,
            avg_run_ms:       self.run_ms.lock().value,
        }
    }
}
