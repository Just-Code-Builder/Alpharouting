//! Picks which reserve to repay and which to seize for a liquidatable
//! borrower, given their per-reserve balances. Pure logic — no RPC calls —
//! so it's fully unit-testable.

use alloy::primitives::{Address, U256};

#[derive(Debug, Clone, Copy)]
pub struct ReserveBalance {
    pub asset: Address,
    pub debt: U256,       // currentStableDebt + currentVariableDebt
    pub a_token: U256,    // currentATokenBalance
    pub is_collateral: bool,
    pub is_borrowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidationTarget {
    pub collateral_asset: Address,
    pub debt_asset: Address,
    pub debt_to_cover: U256,
}

/// Aave V3's close factor: liquidators may repay up to 50% of a position's
/// debt in a single call (100% only applies once health factor is deep
/// enough below 1 — we stay conservative and always use 50%).
const CLOSE_FACTOR_BPS: u64 = 5_000;

/// Chooses the largest debt reserve to repay and the largest collateral
/// reserve to seize. Returns `None` if the borrower has no reserve flagged
/// as both borrowed and (separately) no reserve flagged as collateral.
pub fn select_liquidation_target(reserves: &[ReserveBalance]) -> Option<LiquidationTarget> {
    let debt_reserve = reserves
        .iter()
        .filter(|r| r.is_borrowed && r.debt > U256::ZERO)
        .max_by_key(|r| r.debt)?;

    let collateral_reserve = reserves
        .iter()
        .filter(|r| r.is_collateral && r.a_token > U256::ZERO)
        .max_by_key(|r| r.a_token)?;

    let debt_to_cover = debt_reserve.debt * U256::from(CLOSE_FACTOR_BPS) / U256::from(10_000u64);
    if debt_to_cover.is_zero() {
        return None;
    }

    Some(LiquidationTarget {
        collateral_asset: collateral_reserve.asset,
        debt_asset: debt_reserve.asset,
        debt_to_cover,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address::repeat_byte(byte)
    }

    #[test]
    fn picks_largest_debt_and_largest_collateral() {
        let reserves = vec![
            ReserveBalance {
                asset: addr(1),
                debt: U256::from(100u64),
                a_token: U256::ZERO,
                is_collateral: false,
                is_borrowed: true,
            },
            ReserveBalance {
                asset: addr(2),
                debt: U256::from(500u64), // larger debt -> chosen
                a_token: U256::ZERO,
                is_collateral: false,
                is_borrowed: true,
            },
            ReserveBalance {
                asset: addr(3),
                debt: U256::ZERO,
                a_token: U256::from(1000u64), // only collateral -> chosen
                is_collateral: true,
                is_borrowed: false,
            },
        ];

        let target = select_liquidation_target(&reserves).unwrap();
        assert_eq!(target.debt_asset, addr(2));
        assert_eq!(target.collateral_asset, addr(3));
        assert_eq!(target.debt_to_cover, U256::from(250u64)); // 50% close factor
    }

    #[test]
    fn returns_none_without_any_debt_reserve() {
        let reserves = vec![ReserveBalance {
            asset: addr(1),
            debt: U256::ZERO,
            a_token: U256::from(1000u64),
            is_collateral: true,
            is_borrowed: false,
        }];
        assert!(select_liquidation_target(&reserves).is_none());
    }

    #[test]
    fn returns_none_without_any_collateral_reserve() {
        let reserves = vec![ReserveBalance {
            asset: addr(1),
            debt: U256::from(1000u64),
            a_token: U256::ZERO,
            is_collateral: false,
            is_borrowed: true,
        }];
        assert!(select_liquidation_target(&reserves).is_none());
    }

    #[test]
    fn ignores_reserves_not_flagged_as_borrowed_even_with_dust_debt() {
        // A reserve can carry stale `debt` data (e.g. rounding) without the
        // borrowing flag set; only flagged reserves are eligible.
        let reserves = vec![
            ReserveBalance {
                asset: addr(1),
                debt: U256::from(10u64),
                a_token: U256::ZERO,
                is_collateral: false,
                is_borrowed: false,
            },
            ReserveBalance {
                asset: addr(2),
                debt: U256::from(100u64),
                a_token: U256::ZERO,
                is_collateral: false,
                is_borrowed: true,
            },
            ReserveBalance {
                asset: addr(3),
                debt: U256::ZERO,
                a_token: U256::from(100u64),
                is_collateral: true,
                is_borrowed: false,
            },
        ];
        let target = select_liquidation_target(&reserves).unwrap();
        assert_eq!(target.debt_asset, addr(2));
    }
}
