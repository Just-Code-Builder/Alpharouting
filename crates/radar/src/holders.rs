//! Holder-distribution math.
//!
//! The pool's own address and burn addresses are excluded from holder stats:
//! tokens sitting in the liquidity pool aren't a dump risk the way a whale's
//! wallet is, and burned supply can never be sold at all. Counting either as
//! a "top holder" produces false alarms on perfectly normal launches.

use std::collections::HashMap;

use alloy::primitives::{address, Address, U256};

/// Addresses that hold supply but can never sell it.
pub const BURN_ADDRESSES: [Address; 2] = [
    Address::ZERO,
    address!("000000000000000000000000000000000000dEaD"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct HolderStats {
    pub holder_count: u64,
    /// 0.0..=1.0 of circulating (non-excluded) supply.
    pub top_holder_share: f64,
    pub top10_share: f64,
    pub creator_share: f64,
}

/// Computes distribution over `balances`, ignoring `pool` and burn
/// addresses. Shares are expressed against circulating supply — total supply
/// minus excluded balances — because that's the supply that can actually hit
/// the market.
///
/// Returns `None` if there is no circulating supply to reason about.
pub fn compute(
    balances: &HashMap<Address, U256>,
    pool: Option<Address>,
    creator: Option<Address>,
) -> Option<HolderStats> {
    let mut excluded: Vec<Address> = BURN_ADDRESSES.to_vec();
    if let Some(p) = pool {
        excluded.push(p);
    }

    let mut held: Vec<(Address, U256)> = balances
        .iter()
        .filter(|(addr, bal)| !excluded.contains(addr) && !bal.is_zero())
        .map(|(addr, bal)| (*addr, *bal))
        .collect();

    let circulating: U256 = held.iter().fold(U256::ZERO, |acc, (_, b)| acc + *b);
    if circulating.is_zero() {
        return None;
    }

    held.sort_by(|a, b| b.1.cmp(&a.1));

    let top_holder_share = share_of(held[0].1, circulating);
    let top10: U256 = held.iter().take(10).fold(U256::ZERO, |acc, (_, b)| acc + *b);
    let top10_share = share_of(top10, circulating);
    let creator_share = creator
        .and_then(|c| balances.get(&c).copied())
        .map(|bal| share_of(bal, circulating))
        .unwrap_or(0.0);

    Some(HolderStats {
        holder_count: held.len() as u64,
        top_holder_share,
        top10_share,
        creator_share,
    })
}

/// Ratio as f64 via integer scaling — `U256 as f64` loses precision badly on
/// 18-decimal supplies, so scale to basis points in integer space first.
fn share_of(part: U256, whole: U256) -> f64 {
    if whole.is_zero() {
        return 0.0;
    }
    let scaled = part.saturating_mul(U256::from(1_000_000u64)) / whole;
    let scaled: u64 = scaled.try_into().unwrap_or(1_000_000);
    scaled as f64 / 1_000_000.0
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
    fn evenly_spread_supply_has_low_concentration() {
        let mut balances = HashMap::new();
        for i in 1..=100u8 {
            balances.insert(addr(i), tokens(10));
        }
        let stats = compute(&balances, None, None).unwrap();
        assert_eq!(stats.holder_count, 100);
        assert!((stats.top_holder_share - 0.01).abs() < 1e-6);
        assert!((stats.top10_share - 0.10).abs() < 1e-6);
    }

    #[test]
    fn whale_concentration_is_detected() {
        let mut balances = HashMap::new();
        balances.insert(addr(1), tokens(900));
        balances.insert(addr(2), tokens(50));
        balances.insert(addr(3), tokens(50));
        let stats = compute(&balances, None, None).unwrap();
        assert_eq!(stats.holder_count, 3);
        assert!((stats.top_holder_share - 0.90).abs() < 1e-6);
    }

    #[test]
    fn pool_balance_is_excluded_from_holder_stats() {
        let pool = addr(9);
        let mut balances = HashMap::new();
        // Pool holds the vast majority, as it normally would right after a launch.
        balances.insert(pool, tokens(9_000));
        balances.insert(addr(1), tokens(500));
        balances.insert(addr(2), tokens(500));

        let stats = compute(&balances, Some(pool), None).unwrap();
        assert_eq!(stats.holder_count, 2, "pool must not count as a holder");
        // Shares are against circulating (1000), not total (10_000).
        assert!((stats.top_holder_share - 0.50).abs() < 1e-6);
    }

    #[test]
    fn burned_supply_is_excluded() {
        let mut balances = HashMap::new();
        balances.insert(Address::ZERO, tokens(5_000));
        balances.insert(address!("000000000000000000000000000000000000dEaD"), tokens(4_000));
        balances.insert(addr(1), tokens(500));
        balances.insert(addr(2), tokens(500));

        let stats = compute(&balances, None, None).unwrap();
        assert_eq!(stats.holder_count, 2);
        assert!((stats.top_holder_share - 0.50).abs() < 1e-6);
    }

    #[test]
    fn creator_share_is_reported() {
        let creator = addr(7);
        let mut balances = HashMap::new();
        balances.insert(creator, tokens(300));
        balances.insert(addr(1), tokens(700));

        let stats = compute(&balances, None, Some(creator)).unwrap();
        assert!((stats.creator_share - 0.30).abs() < 1e-6);
    }

    #[test]
    fn creator_with_no_balance_reports_zero_share() {
        let mut balances = HashMap::new();
        balances.insert(addr(1), tokens(1_000));
        let stats = compute(&balances, None, Some(addr(7))).unwrap();
        assert_eq!(stats.creator_share, 0.0);
    }

    #[test]
    fn zero_balances_are_not_counted_as_holders() {
        let mut balances = HashMap::new();
        balances.insert(addr(1), tokens(100));
        balances.insert(addr(2), U256::ZERO); // sold out entirely
        let stats = compute(&balances, None, None).unwrap();
        assert_eq!(stats.holder_count, 1);
    }

    #[test]
    fn no_circulating_supply_returns_none() {
        let pool = addr(9);
        let mut balances = HashMap::new();
        balances.insert(pool, tokens(1_000));
        balances.insert(Address::ZERO, tokens(500));
        // Everything is either in the pool or burned.
        assert!(compute(&balances, Some(pool), None).is_none());
    }

    #[test]
    fn empty_balances_returns_none() {
        assert!(compute(&HashMap::new(), None, None).is_none());
    }

    #[test]
    fn precision_holds_at_18_decimal_scale() {
        // 1 token out of 1 billion, at 18 decimals — would be lost to f64
        // rounding if we converted U256 to f64 before dividing.
        let mut balances = HashMap::new();
        balances.insert(addr(1), tokens(999_999_999));
        balances.insert(addr(2), tokens(1));
        let stats = compute(&balances, None, None).unwrap();
        assert!(stats.top_holder_share > 0.999);
        assert!(stats.top_holder_share <= 1.0);
    }
}
