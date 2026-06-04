//! Layer 12 — Execution Observability.
//!
//! Centralised, in-memory replay buffer for every opportunity the bot evaluates.
//! The bot writes synchronously on the hot path; Telegram / API reads on demand.
//!
//! Backed by a `parking_lot::Mutex<VecDeque<...>>` ring buffer with FIFO eviction
//! at capacity (default ~10,000 events). No async work, no I/O, no allocations
//! beyond the event itself in the write path.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod buffer;
pub mod summary;

pub use buffer::ReplayBuffer;
pub use summary::{render_for_telegram, ReplaySummary};

/// Stage of the execution pipeline the event was emitted from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    /// Opportunity first surfaced by the scanner / discovery layer.
    Detection,
    /// Eth_call / state-override simulation step.
    Simulation,
    /// Gas-vs-profit gating step.
    GasGate,
    /// Bundle / tx broadcast (mempool, private relay, or builder).
    Broadcast,
    /// On-chain receipt observed.
    Receipt,
}

/// Terminal outcome — what actually happened to this opportunity.
///
/// `Pending` is the in-flight default; the bot upgrades the outcome as the
/// event traverses the pipeline (Pending -> SimPassed -> Broadcast -> Success
/// or a failure variant at any step).
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

/// One record in the replay buffer.
///
/// Every field beyond the bookkeeping triple (`timestamp`, `block_number`,
/// `kind`, `expected_profit_usdc`, `outcome`, `latency_ms`) is optional so the
/// same struct can describe a `Detection` event (almost nothing known) and a
/// `Receipt` event (everything known).
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
    /// Wall-clock latency from detection to broadcast, in milliseconds.
    pub latency_ms: u64,
    /// Free-form reason for failures / rejections (e.g. "gas > 80% of profit").
    pub reason: Option<String>,
}

impl ExecutionEvent {
    /// Convenience constructor for a fresh `Detection` event with sensible
    /// defaults. Call sites typically `..` -spread this then mutate.
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
