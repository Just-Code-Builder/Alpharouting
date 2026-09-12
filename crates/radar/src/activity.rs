//! Classifies swap events into buys and sells of the token under watch.
//!
//! "Buy" always means the watched token left the pool (someone acquired it);
//! "sell" means it entered the pool. Which side of the pair the token sits on
//! flips the sign, which is the usual source of bugs here, so both the V2 and
//! V3 event shapes get their own explicit classifier.

use alloy::primitives::{I256, U256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Activity {
    pub buys: u64,
    pub sells: u64,
}

impl Activity {
    pub fn tally(sides: impl IntoIterator<Item = Side>) -> Self {
        let mut a = Activity::default();
        for side in sides {
            match side {
                Side::Buy => a.buys += 1,
                Side::Sell => a.sells += 1,
            }
        }
        a
    }
}

/// Uniswap V2 `Swap(sender, amount0In, amount1In, amount0Out, amount1Out, to)`.
/// All amounts are unsigned and only one side of each pair is non-zero.
pub fn classify_v2(
    token_is_token0: bool,
    amount0_in: U256,
    amount1_in: U256,
    amount0_out: U256,
    amount1_out: U256,
) -> Option<Side> {
    let (token_in, token_out) = if token_is_token0 {
        (amount0_in, amount0_out)
    } else {
        (amount1_in, amount1_out)
    };

    if !token_out.is_zero() {
        Some(Side::Buy)
    } else if !token_in.is_zero() {
        Some(Side::Sell)
    } else {
        None
    }
}

/// Uniswap V3 `Swap(sender, recipient, int256 amount0, int256 amount1, ...)`.
/// Amounts are signed from the *pool's* perspective: positive means the pool
/// received that token, negative means it paid it out.
pub fn classify_v3(token_is_token0: bool, amount0: I256, amount1: I256) -> Option<Side> {
    let delta = if token_is_token0 { amount0 } else { amount1 };

    if delta.is_negative() {
        Some(Side::Buy) // pool paid out the watched token
    } else if delta.is_positive() {
        Some(Side::Sell) // pool took the watched token in
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(n: u64) -> U256 {
        U256::from(n)
    }

    fn i(n: i64) -> I256 {
        I256::try_from(n).unwrap()
    }

    #[test]
    fn v2_buy_when_watched_token_leaves_pool_as_token0() {
        // Someone paid token1 in, took token0 out. token0 is ours -> buy.
        let side = classify_v2(true, u(0), u(1_000), u(50), u(0));
        assert_eq!(side, Some(Side::Buy));
    }

    #[test]
    fn v2_sell_when_watched_token_enters_pool_as_token0() {
        let side = classify_v2(true, u(50), u(0), u(0), u(1_000));
        assert_eq!(side, Some(Side::Sell));
    }

    #[test]
    fn v2_side_flips_when_watched_token_is_token1() {
        // Identical event, but now token1 is ours.
        // token1 went in (amount1In) -> that's a sell of our token.
        let side = classify_v2(false, u(0), u(1_000), u(50), u(0));
        assert_eq!(side, Some(Side::Sell));

        let side = classify_v2(false, u(50), u(0), u(0), u(1_000));
        assert_eq!(side, Some(Side::Buy));
    }

    #[test]
    fn v2_empty_swap_is_unclassifiable() {
        assert_eq!(classify_v2(true, u(0), u(0), u(0), u(0)), None);
    }

    #[test]
    fn v3_negative_delta_on_watched_token_is_a_buy() {
        // Pool paid out token0 (negative), took token1 in (positive).
        assert_eq!(classify_v3(true, i(-50), i(1_000)), Some(Side::Buy));
    }

    #[test]
    fn v3_positive_delta_on_watched_token_is_a_sell() {
        assert_eq!(classify_v3(true, i(50), i(-1_000)), Some(Side::Sell));
    }

    #[test]
    fn v3_side_flips_when_watched_token_is_token1() {
        // Same event as the buy case above, but token1 is ours: token1 went
        // into the pool, so from our token's perspective it's a sell.
        assert_eq!(classify_v3(false, i(-50), i(1_000)), Some(Side::Sell));
        assert_eq!(classify_v3(false, i(50), i(-1_000)), Some(Side::Buy));
    }

    #[test]
    fn v3_zero_delta_is_unclassifiable() {
        assert_eq!(classify_v3(true, i(0), i(1_000)), None);
    }

    #[test]
    fn tally_counts_both_sides() {
        let sides = vec![Side::Buy, Side::Buy, Side::Sell, Side::Buy];
        assert_eq!(Activity::tally(sides), Activity { buys: 3, sells: 1 });
    }

    #[test]
    fn tally_of_nothing_is_zeroed() {
        assert_eq!(Activity::tally(vec![]), Activity { buys: 0, sells: 0 });
    }
}
