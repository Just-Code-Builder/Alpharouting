//! Pool discovery and signal gathering.
//!
//! The pure helpers here (balance reconstruction, liquidity valuation, the
//! seen-pool registry) are unit-tested offline; the RPC-driven functions that
//! feed them need a live endpoint.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Pools already reported, so a restart doesn't re-alert on every historical
/// launch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeenPools {
    pub last_scanned_block: u64,
    pub pools: BTreeSet<Address>,
}

impl SeenPools {
    pub fn load_or_default(path: &Path, start_block: u64) -> Result<Self> {
        if !path.exists() {
            return Ok(Self { last_scanned_block: start_block, pools: BTreeSet::new() });
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading seen-pool cache at {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing seen-pool cache at {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("writing seen-pool cache to {}", path.display()))?;
        Ok(())
    }

    /// Returns true if this pool hadn't been seen before.
    pub fn mark_seen(&mut self, pool: Address) -> bool {
        self.pools.insert(pool)
    }
}

/// Rebuilds current balances from a token's full `Transfer` history.
///
/// Mints arrive as transfers from the zero address and burns as transfers to
/// it; the zero address is tracked like any other holder and filtered out
/// later by `holders::compute`. Underflow is saturated rather than panicking:
/// a malformed or reordered log stream shouldn't take the scanner down.
pub fn apply_transfers(
    balances: &mut HashMap<Address, U256>,
    transfers: &[(Address, Address, U256)],
) {
    for (from, to, value) in transfers {
        if *from != Address::ZERO {
            let entry = balances.entry(*from).or_insert(U256::ZERO);
            *entry = entry.saturating_sub(*value);
        }
        let entry = balances.entry(*to).or_insert(U256::ZERO);
        *entry = entry.saturating_add(*value);
    }
}

/// Values a pool's liquidity from its quote-token reserve.
///
/// Uses the standard AMM convention of doubling the quote side: in a balanced
/// pool the two sides are equal in value, so total ≈ 2 × quote side. Returns
/// `None` without a quote price rather than inventing a figure.
pub fn quote_liquidity_usd(
    quote_reserve: U256,
    quote_decimals: u8,
    quote_usd_price: Option<f64>,
) -> Option<f64> {
    let price = quote_usd_price?;
    let whole = to_f64_with_decimals(quote_reserve, quote_decimals);
    Some(whole * price * 2.0)
}

/// U256 -> f64 respecting token decimals, via integer scaling to avoid the
/// precision loss of a direct cast on 18-decimal values.
fn to_f64_with_decimals(amount: U256, decimals: u8) -> f64 {
    let divisor = U256::from(10u64).pow(U256::from(decimals));
    if divisor.is_zero() {
        return 0.0;
    }
    let whole = amount / divisor;
    let remainder = amount % divisor;

    let whole_f: f64 = u128::try_from(whole).map(|v| v as f64).unwrap_or(f64::MAX);
    let frac_f = if remainder.is_zero() {
        0.0
    } else {
        // Scale the fractional part into a range f64 handles exactly.
        let scaled = remainder.saturating_mul(U256::from(1_000_000u64)) / divisor;
        u128::try_from(scaled).map(|v| v as f64 / 1_000_000.0).unwrap_or(0.0)
    };

    whole_f + frac_f
}

/// Whether a V2 pool's LP supply is effectively burned or locked away.
///
/// `lp_in_burn_addresses / lp_total_supply` above this fraction counts as
/// burned; deployers commonly retain a dust amount.
const LP_BURNED_THRESHOLD: f64 = 0.95;

pub fn lp_is_burned(lp_total_supply: U256, lp_held_by_burn_addresses: U256) -> Option<bool> {
    if lp_total_supply.is_zero() {
        return None;
    }
    let scaled = lp_held_by_burn_addresses.saturating_mul(U256::from(1_000_000u64)) / lp_total_supply;
    let fraction = u64::try_from(scaled).unwrap_or(1_000_000) as f64 / 1_000_000.0;
    Some(fraction >= LP_BURNED_THRESHOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(b: u8) -> Address {
        Address::repeat_byte(b)
    }

    fn tokens(n: u64) -> U256 {
        U256::from(n) * U256::from(10u64).pow(U256::from(18u64))
    }

    #[test]
    fn mint_then_transfer_rebuilds_balances() {
        let mut balances = HashMap::new();
        apply_transfers(
            &mut balances,
            &[
                (Address::ZERO, addr(1), tokens(1_000)), // mint
                (addr(1), addr(2), tokens(300)),
                (addr(2), addr(3), tokens(100)),
            ],
        );

        assert_eq!(balances[&addr(1)], tokens(700));
        assert_eq!(balances[&addr(2)], tokens(200));
        assert_eq!(balances[&addr(3)], tokens(100));
        // The mint source is never debited.
        assert!(!balances.contains_key(&Address::ZERO));
    }

    #[test]
    fn burn_credits_the_zero_address() {
        let mut balances = HashMap::new();
        apply_transfers(
            &mut balances,
            &[
                (Address::ZERO, addr(1), tokens(500)),
                (addr(1), Address::ZERO, tokens(200)), // burn
            ],
        );
        assert_eq!(balances[&addr(1)], tokens(300));
        assert_eq!(balances[&Address::ZERO], tokens(200));
    }

    #[test]
    fn underflow_saturates_instead_of_panicking() {
        let mut balances = HashMap::new();
        // A send from an address we never saw funded (reordered/partial logs).
        apply_transfers(&mut balances, &[(addr(1), addr(2), tokens(50))]);
        assert_eq!(balances[&addr(1)], U256::ZERO);
        assert_eq!(balances[&addr(2)], tokens(50));
    }

    #[test]
    fn liquidity_doubles_the_quote_side() {
        // 10 WETH at $3,000 -> $30k quote side -> $60k pool.
        let liq = quote_liquidity_usd(tokens(10), 18, Some(3_000.0)).unwrap();
        assert!((liq - 60_000.0).abs() < 1.0);
    }

    #[test]
    fn liquidity_is_unknown_without_a_quote_price() {
        assert!(quote_liquidity_usd(tokens(10), 18, None).is_none());
    }

    #[test]
    fn liquidity_handles_six_decimal_quote_tokens() {
        // 50,000 USDC (6 decimals) at $1 -> $100k pool.
        let reserve = U256::from(50_000u64) * U256::from(10u64).pow(U256::from(6u64));
        let liq = quote_liquidity_usd(reserve, 6, Some(1.0)).unwrap();
        assert!((liq - 100_000.0).abs() < 1.0);
    }

    #[test]
    fn liquidity_handles_fractional_reserves() {
        // 0.5 WETH at $3,000 -> $1,500 quote side -> $3,000 pool.
        let half = tokens(1) / U256::from(2u64);
        let liq = quote_liquidity_usd(half, 18, Some(3_000.0)).unwrap();
        assert!((liq - 3_000.0).abs() < 1.0);
    }

    #[test]
    fn lp_fully_burned_is_detected() {
        assert_eq!(lp_is_burned(tokens(1_000), tokens(1_000)), Some(true));
    }

    #[test]
    fn lp_retained_by_deployer_is_detected() {
        assert_eq!(lp_is_burned(tokens(1_000), tokens(100)), Some(false));
    }

    #[test]
    fn lp_burned_with_dust_retained_still_counts_as_burned() {
        assert_eq!(lp_is_burned(tokens(1_000), tokens(990)), Some(true));
    }

    #[test]
    fn lp_status_unknown_when_supply_is_zero() {
        assert_eq!(lp_is_burned(U256::ZERO, U256::ZERO), None);
    }

    #[test]
    fn seen_pools_round_trip_and_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seen.json");

        let mut seen = SeenPools::load_or_default(&path, 500).unwrap();
        assert_eq!(seen.last_scanned_block, 500);
        assert!(seen.mark_seen(addr(1)));
        assert!(!seen.mark_seen(addr(1)), "second sighting is not new");
        seen.last_scanned_block = 900;
        seen.save(&path).unwrap();

        let reloaded = SeenPools::load_or_default(&path, 0).unwrap();
        assert_eq!(reloaded.last_scanned_block, 900);
        assert!(reloaded.pools.contains(&addr(1)));
    }
}
