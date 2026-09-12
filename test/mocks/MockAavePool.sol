// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

interface IFlashLoanReceiver {
    function executeOperation(address asset, uint256 amount, uint256 premium, address initiator, bytes calldata params)
        external
        returns (bool);
}

/// @notice Minimal Aave V3 Pool stand-in for tests: flashLoanSimple, liquidationCall,
/// getUserAccountData. Fee/seizure/health-factor are all test-configurable.
contract MockAavePool {
    uint256 public premiumBps = 9; // 0.09%, matches real Aave V3
    uint256 public healthFactor;
    uint256 public collateralPerDebtUnitWad = 1e18; // how much collateral liquidationCall pays out per unit of debt covered

    constructor() {}

    function setHealthFactor(uint256 hf) external {
        healthFactor = hf;
    }

    function setCollateralRate(uint256 rateWad) external {
        collateralPerDebtUnitWad = rateWad;
    }

    function getPool() external view returns (address) {
        return address(this);
    }

    function getUserAccountData(address)
        external
        view
        returns (uint256, uint256, uint256, uint256, uint256, uint256)
    {
        return (0, 0, 0, 0, 0, healthFactor);
    }

    function flashLoanSimple(
        address receiverAddress,
        address asset,
        uint256 amount,
        bytes calldata params,
        uint16
    ) external {
        uint256 premium = (amount * premiumBps) / 10_000;
        IERC20(asset).transfer(receiverAddress, amount);
        bool ok = IFlashLoanReceiver(receiverAddress).executeOperation(asset, amount, premium, msg.sender, params);
        require(ok, "executeOperation failed");
        IERC20(asset).transferFrom(receiverAddress, address(this), amount + premium);
    }

    function liquidationCall(
        address collateralAsset,
        address debtAsset,
        address, /* user */
        uint256 debtToCover,
        bool /* receiveAToken */
    ) external {
        IERC20(debtAsset).transferFrom(msg.sender, address(this), debtToCover);
        uint256 collateralOut = (debtToCover * collateralPerDebtUnitWad) / 1e18;
        IERC20(collateralAsset).transfer(msg.sender, collateralOut);
    }
}
