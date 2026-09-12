//! Capacity-bounded ring buffer of [`ExecutionEvent`]s.
//!
//! All reads return owned snapshots — callers never hold the lock past the
//! function return.

use std::collections::VecDeque;

use parking_lot::Mutex;

use crate::summary::ReplaySummary;
use crate::{EventOutcome, ExecutionEvent};

pub const DEFAULT_CAPACITY: usize = 10_000;

pub struct ReplayBuffer {
    events: Mutex<VecDeque<ExecutionEvent>>,
    capacity: usize,
}

impl ReplayBuffer {
    // Zero capacity is normalised to 1 — buffer always holds at least one event.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            events: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn capacity(&self) -> usize { self.capacity }
    pub fn len(&self) -> usize { self.events.lock().len() }
    pub fn is_empty(&self) -> bool { self.events.lock().is_empty() }

    /// Append an event. Drops the oldest if at capacity (FIFO eviction).
    pub fn record(&self, event: ExecutionEvent) {
        let mut q = self.events.lock();
        if q.len() == self.capacity {
            q.pop_front();
        }
        q.push_back(event);
    }

    /// Last `n` events in chronological order.
    pub fn last_n(&self, n: usize) -> Vec<ExecutionEvent> {
        let q = self.events.lock();
        let start = q.len().saturating_sub(n);
        q.iter().skip(start).cloned().collect()
    }

    /// Most-recent events matching `outcome`, up to `limit`. Output is chronological.
    pub fn by_outcome(&self, outcome: EventOutcome, limit: usize) -> Vec<ExecutionEvent> {
        if limit == 0 { return Vec::new(); }
        let q = self.events.lock();
        let mut hits: Vec<ExecutionEvent> = q
            .iter()
            .rev()
            .filter(|e| e.outcome == outcome)
            .take(limit)
            .cloned()
            .collect();
        hits.reverse();
        hits
    }

    /// Most-recent events for a given borrower (case-insensitive), up to `limit`.
    pub fn by_borrower(&self, borrower: &str, limit: usize) -> Vec<ExecutionEvent> {
        if limit == 0 { return Vec::new(); }
        let needle = borrower.to_ascii_lowercase();
        let q = self.events.lock();
        let mut hits: Vec<ExecutionEvent> = q
            .iter()
            .rev()
            .filter(|e| {
                e.borrower
                    .as_deref()
                    .map(|b| b.to_ascii_lowercase() == needle)
                    .unwrap_or(false)
            })
            .take(limit)
            .cloned()
            .collect();
        hits.reverse();
        hits
    }

    pub fn summary(&self) -> ReplaySummary {
        let q = self.events.lock();
        let total = q.len();
        let (mut sim_passed, mut sim_failed, mut success,
             mut reverted, mut gas_rejected) = (0, 0, 0, 0, 0);
        let mut latency_sum: u128 = 0;
        let mut realized: f64 = 0.0;

        for e in q.iter() {
            match e.outcome {
                EventOutcome::SimPassed   => sim_passed   += 1,
                EventOutcome::SimFailed   => sim_failed   += 1,
                EventOutcome::Success     => success      += 1,
                EventOutcome::Reverted    => reverted     += 1,
                EventOutcome::GateRejected => gas_rejected += 1,
                _ => {}
            }
            latency_sum += e.latency_ms as u128;
            if matches!(e.outcome, EventOutcome::Success) {
                if let Some(p) = e.actual_profit_usdc { realized += p; }
            }
        }

        ReplaySummary {
            total,
            sim_passed,
            sim_failed,
            success,
            reverted,
            gas_rejected,
            mean_latency_ms: if total == 0 { 0.0 } else { (latency_sum as f64) / (total as f64) },
            total_realized_profit_usdc: realized,
        }
    }

    /// Serialise all buffered events as JSON-Lines (one object per line).
    pub fn export_jsonl(&self) -> String {
        let q = self.events.lock();
        let mut out = String::with_capacity(q.len() * 256);
        for e in q.iter() {
            match serde_json::to_string(e) {
                Ok(line) => { out.push_str(&line); out.push('\n'); }
                Err(err) => {
                    tracing::warn!(error = %err, "observability: failed to serialise event");
                }
            }
        }
        out
    }
}

impl Default for ReplayBuffer {
    fn default() -> Self { Self::new(DEFAULT_CAPACITY) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventKind;
    use chrono::Utc;

    fn mk(outcome: EventOutcome, borrower: Option<&str>, profit: f64, latency: u64) -> ExecutionEvent {
        ExecutionEvent {
            timestamp: Utc::now(),
            block_number: 1,
            kind: EventKind::Detection,
            borrower: borrower.map(|s| s.to_string()),
            route_id: None,
            expected_profit_usdc: profit,
            actual_profit_usdc: Some(profit),
            gas_estimate: None, gas_used: None, gas_cost_usdc: None,
            sim_confidence: None, inclusion_probability: None,
            builder: None, tx_hash: None,
            outcome,
            latency_ms: latency,
            reason: None,
        }
    }

    #[test]
    fn new_normalises_zero_capacity_to_one() {
        let b = ReplayBuffer::new(0);
        assert_eq!(b.capacity(), 1);
        b.record(mk(EventOutcome::Pending, None, 0.0, 0));
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn default_capacity_is_ten_thousand() {
        let b = ReplayBuffer::default();
        assert_eq!(b.capacity(), DEFAULT_CAPACITY);
        assert!(b.is_empty());
    }

    #[test]
    fn buffer_caps_at_capacity_fifo() {
        let b = ReplayBuffer::new(3);
        for n in 1..=3u64 {
            let mut e = mk(EventOutcome::Pending, None, 0.0, 0);
            e.block_number = n;
            b.record(e);
        }
        let mut e = mk(EventOutcome::Pending, None, 0.0, 0);
        e.block_number = 4;
        b.record(e);
        assert_eq!(b.len(), 3);
        let blocks: Vec<u64> = b.last_n(10).iter().map(|e| e.block_number).collect();
        assert_eq!(blocks, vec![2, 3, 4]);
    }

    #[test]
    fn last_n_chronological_and_bounded() {
        let b = ReplayBuffer::new(10);
        for n in 1..=5u64 {
            let mut e = mk(EventOutcome::Pending, None, 0.0, 0);
            e.block_number = n;
            b.record(e);
        }
        let all = b.last_n(100);
        assert_eq!(all.len(), 5);
        assert_eq!(all.first().unwrap().block_number, 1);
        assert_eq!(all.last().unwrap().block_number, 5);
        let tail: Vec<u64> = b.last_n(2).iter().map(|e| e.block_number).collect();
        assert_eq!(tail, vec![4, 5]);
        assert!(b.last_n(0).is_empty());
    }

    #[test]
    fn by_outcome_filters_correctly() {
        let b = ReplayBuffer::new(100);
        b.record(mk(EventOutcome::Success, None, 1.0, 0));
        b.record(mk(EventOutcome::SimFailed, None, 0.0, 0));
        b.record(mk(EventOutcome::Success, None, 2.0, 0));
        b.record(mk(EventOutcome::Reverted, None, 0.0, 0));
        b.record(mk(EventOutcome::Success, None, 3.0, 0));
        let profits: Vec<f64> = b.by_outcome(EventOutcome::Success, 10)
            .iter().map(|e| e.expected_profit_usdc).collect();
        assert_eq!(profits, vec![1.0, 2.0, 3.0]);
        let limited: Vec<f64> = b.by_outcome(EventOutcome::Success, 2)
            .iter().map(|e| e.expected_profit_usdc).collect();
        assert_eq!(limited, vec![2.0, 3.0]);
        assert!(b.by_outcome(EventOutcome::Error, 10).is_empty());
        assert!(b.by_outcome(EventOutcome::Success, 0).is_empty());
    }

    #[test]
    fn by_borrower_filters_case_insensitive() {
        let b = ReplayBuffer::new(100);
        b.record(mk(EventOutcome::Success, Some("0xAaa"), 1.0, 0));
        b.record(mk(EventOutcome::Success, Some("0xBbb"), 2.0, 0));
        b.record(mk(EventOutcome::Success, Some("0xaaa"), 3.0, 0));
        b.record(mk(EventOutcome::Success, None, 4.0, 0));
        let hits = b.by_borrower("0xAAA", 10);
        assert_eq!(hits.len(), 2);
        let one = b.by_borrower("0xaaa", 1);
        assert_eq!(one[0].expected_profit_usdc, 3.0);
        assert!(b.by_borrower("0xdead", 10).is_empty());
        assert!(b.by_borrower("0xaaa", 0).is_empty());
    }

    #[test]
    fn summary_counts_each_outcome() {
        let b = ReplayBuffer::new(100);
        b.record(mk(EventOutcome::SimPassed, None, 0.0, 100));
        b.record(mk(EventOutcome::SimPassed, None, 0.0, 200));
        b.record(mk(EventOutcome::SimFailed, None, 0.0, 50));
        b.record(mk(EventOutcome::GateRejected, None, 0.0, 25));
        b.record(mk(EventOutcome::Success, None, 5.0, 300));
        b.record(mk(EventOutcome::Success, None, 7.5, 400));
        b.record(mk(EventOutcome::Reverted, None, 0.0, 350));
        b.record(mk(EventOutcome::Pending, None, 0.0, 0));
        b.record(mk(EventOutcome::Broadcast, None, 0.0, 0));
        b.record(mk(EventOutcome::Error, None, 0.0, 0));
        let s = b.summary();
        assert_eq!((s.sim_passed, s.sim_failed, s.success, s.reverted, s.gas_rejected), (2, 1, 2, 1, 1));
        assert!((s.total_realized_profit_usdc - 12.5).abs() < 1e-9);
        let expected_mean = (100 + 200 + 50 + 25 + 300 + 400 + 350) as f64 / 10.0;
        assert!((s.mean_latency_ms - expected_mean).abs() < 1e-9);
    }

    #[test]
    fn summary_empty_buffer_no_divide_by_zero() {
        let b = ReplayBuffer::new(100);
        let s = b.summary();
        assert_eq!(s.total, 0);
        assert_eq!(s.mean_latency_ms, 0.0);
        assert_eq!(s.total_realized_profit_usdc, 0.0);
    }

    #[test]
    fn realized_profit_only_counts_success_outcome() {
        let b = ReplayBuffer::new(100);
        b.record(mk(EventOutcome::Reverted, None, 99.0, 0));
        b.record(mk(EventOutcome::Success, None, 2.0, 0));
        assert!((b.summary().total_realized_profit_usdc - 2.0).abs() < 1e-9);
    }

    #[test]
    fn export_jsonl_one_line_per_event() {
        let b = ReplayBuffer::new(10);
        b.record(mk(EventOutcome::Success, Some("0xabc"), 1.5, 100));
        b.record(mk(EventOutcome::SimFailed, None, 0.0, 50));
        let jsonl = b.export_jsonl();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);
        let _: ExecutionEvent = serde_json::from_str(lines[0]).unwrap();
    }

    #[test]
    fn record_after_eviction_keeps_filters_correct() {
        let b = ReplayBuffer::new(2);
        b.record(mk(EventOutcome::Success, Some("0xaaa"), 1.0, 0));
        b.record(mk(EventOutcome::Success, Some("0xaaa"), 2.0, 0));
        b.record(mk(EventOutcome::Success, Some("0xbbb"), 3.0, 0)); // evicts first
        let hits = b.by_borrower("0xaaa", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].expected_profit_usdc, 2.0);
    }
}
