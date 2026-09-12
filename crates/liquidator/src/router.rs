//! Binding for our own `AlphaRouting.executeLiquidation` entry point, plus
//! the swapParams encoding it expects
//! (`abi.decode(swapParams, (string sellDex, uint24 sellFee, uint256 minProfit))`).

use alloy::primitives::{Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolValue;

sol! {
    #[sol(rpc)]
    interface IAlphaRouting {
        function executeLiquidation(
            address collateralAsset,
            address debtAsset,
            address borrower,
            uint256 debtAmount,
            bytes calldata swapParams
        ) external;
    }
}

pub fn encode_swap_params(sell_dex: &str, sell_fee_bps: u32, min_profit: U256) -> Bytes {
    // Solidity's `uint24` round-trips through Rust's `u32` in alloy's sol!
    // tuple encoding as long as the value fits in 24 bits, which pool fee
    // tiers (500 / 3000 / 10000) always do.
    let sell_fee = alloy::primitives::Uint::<24, 1>::from(sell_fee_bps);
    let encoded = (sell_dex.to_string(), sell_fee, min_profit).abi_encode_params();
    Bytes::from(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::sol_types::SolType;

    #[test]
    fn round_trips_through_solidity_style_decode() {
        let encoded = encode_swap_params("uniswap", 3000, U256::from(42u64));

        type Decoded = (
            alloy::sol_types::sol_data::String,
            alloy::sol_types::sol_data::Uint<24>,
            alloy::sol_types::sol_data::Uint<256>,
        );
        let (dex, fee, min_profit) = Decoded::abi_decode_params(&encoded, true).unwrap();

        assert_eq!(dex, "uniswap");
        assert_eq!(fee, alloy::primitives::Uint::<24, 1>::from(3000u32));
        assert_eq!(min_profit, U256::from(42u64));
    }
}
