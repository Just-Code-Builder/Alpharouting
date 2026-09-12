//! Minimal Aave V3 bindings — only the read methods the scanner needs.

use alloy::sol;

sol! {
    #[sol(rpc)]
    interface IPool {
        function getUserAccountData(address user) external view returns (
            uint256 totalCollateralBase,
            uint256 totalDebtBase,
            uint256 availableBorrowsBase,
            uint256 currentLiquidationThreshold,
            uint256 ltv,
            uint256 healthFactor
        );

        function getUserConfiguration(address user) external view returns (uint256 data);

        function getReservesList() external view returns (address[] memory);
    }

    #[sol(rpc)]
    interface IPoolDataProvider {
        function getUserReserveData(address asset, address user) external view returns (
            uint256 currentATokenBalance,
            uint256 currentStableDebt,
            uint256 currentVariableDebt,
            uint256 principalStableDebt,
            uint256 scaledVariableDebt,
            uint256 stableBorrowRate,
            uint256 liquidityRate,
            uint40 stableRateLastUpdated,
            bool usageAsCollateralEnabled
        );
    }
}

/// Health factor is scaled 1e18; a position is liquidatable below this.
pub const HEALTH_FACTOR_LIQUIDATION_THRESHOLD: u128 = 1_000_000_000_000_000_000;

/// Decodes Aave's per-reserve user configuration bitmap. Reserve `i` (its
/// index in `getReservesList()`) uses bits `2*i` (borrowing enabled) and
/// `2*i + 1` (used as collateral). See Aave V3's `UserConfiguration` library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReserveFlags {
    pub borrowing: bool,
    pub collateral: bool,
}

pub fn decode_user_configuration(bitmap: u128, num_reserves: usize) -> Vec<ReserveFlags> {
    (0..num_reserves)
        .map(|i| {
            let borrowing = (bitmap >> (2 * i)) & 1 == 1;
            let collateral = (bitmap >> (2 * i + 1)) & 1 == 1;
            ReserveFlags { borrowing, collateral }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_no_positions() {
        let flags = decode_user_configuration(0, 4);
        assert!(flags.iter().all(|f| !f.borrowing && !f.collateral));
    }

    #[test]
    fn decodes_single_collateral_reserve() {
        // Reserve index 1 used as collateral only: bit 2*1+1 = bit 3 set.
        let flags = decode_user_configuration(0b1000, 4);
        assert_eq!(flags[0], ReserveFlags { borrowing: false, collateral: false });
        assert_eq!(flags[1], ReserveFlags { borrowing: false, collateral: true });
        assert_eq!(flags[2], ReserveFlags { borrowing: false, collateral: false });
    }

    #[test]
    fn decodes_borrow_and_collateral_together() {
        // Reserve 0 borrowed (bit 0) and reserve 2 used as collateral (bit 5).
        let flags = decode_user_configuration(0b100001, 3);
        assert_eq!(flags[0], ReserveFlags { borrowing: true, collateral: false });
        assert_eq!(flags[2], ReserveFlags { borrowing: false, collateral: true });
    }
}
