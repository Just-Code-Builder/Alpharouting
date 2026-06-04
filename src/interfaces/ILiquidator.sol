// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

/// @notice Aave V3 IPool methods used by the liquidation strategy. Only the
/// methods we actually call — full IPool is huge.
interface ILiquidator {
    function liquidationCall(
        address collateralAsset,
        address debtAsset,
        address user,
        uint256 debtToCover,
        bool receiveAToken
    ) external;

    function getUserAccountData(address user)
        external
        view
        returns (
            uint256 totalCollateralBase,
            uint256 totalDebtBase,
            uint256 availableBorrowsBase,
            uint256 currentLiquidationThreshold,
            uint256 ltv,
            uint256 healthFactor
        );
}
