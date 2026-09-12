//! Builds the set of addresses that have ever borrowed from the Aave pool,
//! by scanning `Borrow` event logs. Aave doesn't expose "list all borrowers"
//! directly, so this is the standard way liquidation bots build a watchlist.
//!
//! The registry persists to a JSON file so a restart resumes from the last
//! scanned block instead of re-scanning from genesis.

use std::collections::BTreeSet;
use std::path::Path;

use alloy::primitives::{Address, B256};
use alloy::providers::Provider;
use alloy::rpc::types::Filter;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// keccak256("Borrow(address,address,address,uint256,uint8,uint256,uint16)")
/// — Aave V3 Pool's Borrow event. `onBehalfOf` (topic 3) is the borrower.
pub const BORROW_EVENT_SIGNATURE: &str =
    "Borrow(address,address,address,uint256,uint8,uint256,uint16)";

/// How many blocks to request per `eth_getLogs` call. Public RPC providers
/// commonly cap ranges (Base full nodes often limit to a few thousand);
/// chunking keeps every request within that ceiling.
pub const DEFAULT_BLOCK_CHUNK: u64 = 5_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BorrowerRegistry {
    pub last_scanned_block: u64,
    pub borrowers: BTreeSet<Address>,
}

impl BorrowerRegistry {
    pub fn load_or_default(path: &Path, start_block: u64) -> Result<Self> {
        if !path.exists() {
            return Ok(Self { last_scanned_block: start_block, borrowers: BTreeSet::new() });
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading borrower registry at {}", path.display()))?;
        let registry: Self = serde_json::from_str(&raw)
            .with_context(|| format!("parsing borrower registry at {}", path.display()))?;
        Ok(registry)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(path, raw)
            .with_context(|| format!("writing borrower registry to {}", path.display()))?;
        Ok(())
    }

    pub fn insert_all(&mut self, addrs: impl IntoIterator<Item = Address>) -> usize {
        let before = self.borrowers.len();
        self.borrowers.extend(addrs);
        self.borrowers.len() - before
    }
}

/// Scans `[registry.last_scanned_block + 1, chain_head]` for Borrow events in
/// `DEFAULT_BLOCK_CHUNK`-sized windows, adds newly seen borrowers, and
/// advances `last_scanned_block`. Requires a live RPC connection.
pub async fn scan_new_borrowers<P: Provider>(
    provider: &P,
    pool_address: Address,
    registry: &mut BorrowerRegistry,
) -> Result<usize> {
    let head = provider.get_block_number().await.context("fetching chain head")?;
    if registry.last_scanned_block >= head {
        return Ok(0);
    }

    let topic0: B256 = alloy::primitives::keccak256(BORROW_EVENT_SIGNATURE.as_bytes());
    let mut total_new = 0usize;
    let mut from = registry.last_scanned_block + 1;

    while from <= head {
        let to = (from + DEFAULT_BLOCK_CHUNK - 1).min(head);

        let filter = Filter::new()
            .address(pool_address)
            .event_signature(topic0)
            .from_block(from)
            .to_block(to);

        let logs = provider.get_logs(&filter).await
            .with_context(|| format!("get_logs [{from}, {to}]"))?;

        let borrowers = logs.iter().filter_map(|log| {
            // topics: [event sig, reserve, user, onBehalfOf] — onBehalfOf is
            // the address whose debt actually increased.
            log.topics().get(3).map(|t| Address::from_slice(&t.as_slice()[12..]))
        });

        total_new += registry.insert_all(borrowers);
        registry.last_scanned_block = to;
        from = to + 1;
    }

    Ok(total_new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_default_returns_seed_block_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("borrowers.json");

        let registry = BorrowerRegistry::load_or_default(&path, 12_345).unwrap();
        assert_eq!(registry.last_scanned_block, 12_345);
        assert!(registry.borrowers.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("borrowers.json");

        let mut registry = BorrowerRegistry { last_scanned_block: 100, borrowers: BTreeSet::new() };
        registry.insert_all([Address::repeat_byte(1), Address::repeat_byte(2)]);
        registry.save(&path).unwrap();

        let loaded = BorrowerRegistry::load_or_default(&path, 0).unwrap();
        assert_eq!(loaded.last_scanned_block, 100);
        assert_eq!(loaded.borrowers.len(), 2);
    }

    #[test]
    fn insert_all_deduplicates_and_reports_only_new_count() {
        let mut registry = BorrowerRegistry::default();
        let a = Address::repeat_byte(1);
        let b = Address::repeat_byte(2);

        assert_eq!(registry.insert_all([a, b]), 2);
        assert_eq!(registry.insert_all([a]), 0); // already known
        assert_eq!(registry.borrowers.len(), 2);
    }
}
