// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @notice Minimal Uniswap V3 SwapRouter stand-in: swaps at a fixed, test-configurable rate.
contract MockSwapRouter {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24  fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    // amountOut per unit amountIn, scaled 1e18 (e.g. 2e18 == tokenOut is worth 2x tokenIn)
    uint256 public rateWad = 1e18;

    function setRate(uint256 rateWad_) external {
        rateWad = rateWad_;
    }

    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut) {
        IERC20(params.tokenIn).transferFrom(msg.sender, address(this), params.amountIn);
        amountOut = (params.amountIn * rateWad) / 1e18;
        require(amountOut >= params.amountOutMinimum, "slippage");
        IERC20(params.tokenOut).transfer(params.recipient, amountOut);
    }
}
