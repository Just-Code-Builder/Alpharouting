//! In-memory replay buffer for execution pipeline events.
//!
//! Writes are synchronous on the hot path. Reads happen on demand from the
//! Telegram notifier and the /metrics endpoint. Ring buffer with FIFO eviction
//! at capacity — no I/O, no allocations beyond the event struct itself.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod buffer;
pub mod summary;

pub use buffer::ReplayBuffer;
pub use summary::{render_for_telegram, ReplaySummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    Detection,
    Simulation,
    GasGate,
    Broadcast,
    Receipt,
}

/// Lifecycle: Pending → SimPassed → Broadcast → Success (or a failure variant
/// at any stage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventOutcome {
    Pending,
    SimPassed,
    SimFailed,
    GateRejected,
    Broadcast,
    Success,
    Reverted,
    Error,
}

/// All optional fields beyond the core triple are None at Detection and fill
/// in as the opportunity moves through the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub timestamp: DateTime<Utc>,
    pub block_number: u64,
    pub kind: EventKind,
    pub borrower: Option<String>,
    pub route_id: Option<String>,
    pub expected_profit_usdc: f64,
    pub actual_profit_usdc: Option<f64>,
    pub gas_estimate: Option<u64>,
    pub gas_used: Option<u64>,
    pub gas_cost_usdc: Option<f64>,
    pub sim_confidence: Option<f64>,
    pub inclusion_probability: Option<f64>,
    pub builder: Option<String>,
    pub tx_hash: Option<String>,
    pub outcome: EventOutcome,
    pub latency_ms: u64,
    pub reason: Option<String>,
}

impl ExecutionEvent {
    pub fn new_detection(block_number: u64, expected_profit_usdc: f64) -> Self {
        Self {
            timestamp: Utc::now(),
            block_number,
            kind: EventKind::Detection,
            borrower: None,
            route_id: None,
            expected_profit_usdc,
            actual_profit_usdc: None,
            gas_estimate: None,
            gas_used: None,
            gas_cost_usdc: None,
            sim_confidence: None,
            inclusion_probability: None,
            builder: None,
            tx_hash: None,
            outcome: EventOutcome::Pending,
            latency_ms: 0,
            reason: None,
        }
    }
}
