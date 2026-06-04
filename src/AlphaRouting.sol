// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

// ─────────────────────────────────────────────────────────────────────────────
//  AlphaRouting — Aave V3 flashloan executor (Base mainnet / Base Sepolia)
//
//  This file is a PUBLIC INTERFACE SNIPPET.
//  Full implementation is private — DM @Just-Code-Builder on GitHub for access.
// ─────────────────────────────────────────────────────────────────────────────

import {Ownable}        from "@openzeppelin/contracts/access/Ownable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {IERC20}         from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20}      from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ILiquidator}    from "./interfaces/ILiquidator.sol";

// ── External protocol interfaces ─────────────────────────────────────────────

interface IPoolAddressesProvider {
    function getPool() external view returns (address);
}

interface IAavePool {
    function flashLoanSimple(
        address receiverAddress,
        address asset,
        uint256 amount,
        bytes calldata params,
        uint16 referralCode
    ) external;
}

interface IUniswapV3SwapRouter {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24  fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256);
}

interface IUniswapV3Pool {
    function flash(address recipient, uint256 amount0, uint256 amount1, bytes calldata data) external;
    function token0() external view returns (address);
    function token1() external view returns (address);
    function fee()    external view returns (uint24);
}

interface IAerodromeRouter {
    struct Route { address from; address to; bool stable; address factory; }
    function swapExactTokensForTokens(
        uint256 amountIn, uint256 amountOutMin,
        Route[] calldata routes, address to, uint256 deadline
    ) external returns (uint256[] memory);
    function defaultFactory() external view returns (address);
}

interface IBalancerVault {
    function flashLoan(
        address recipient,
        address[] calldata tokens,
        uint256[] calldata amounts,
        bytes calldata userData
    ) external;
}

// ── Contract ─────────────────────────────────────────────────────────────────

/// @title  AlphaRouting
/// @notice Multi-strategy flashloan executor: triangular arb, liquidations,
///         flash-swap arb, Balancer arb, batch liquidations, rebase arb.
///         Deployed on Base mainnet. Routers are constructor-injected so the
///         same bytecode works on mainnet and testnet.
///
/// @dev    SNIPPET ONLY — implementation bodies are not shown.
///         Contact @Just-Code-Builder for the full source + Rust execution engine.
contract AlphaRouting is Ownable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    // ── Strategy constants ────────────────────────────────────────────────
    uint8 public constant STRAT_NONE       = 0;
    uint8 public constant STRAT_ARB        = 1;
    uint8 public constant STRAT_LIQ        = 2;
    uint8 public constant STRAT_REBASE     = 3;
    uint8 public constant STRAT_LIQ_COMBO  = 4;
    uint8 public constant STRAT_BATCH_LIQ  = 5;
    uint8 public constant STRAT_FLASH_SWAP = 6;
    uint8 public constant STRAT_BALANCER   = 7;

    string public constant DEX_UNISWAP     = "uniswap";
    string public constant DEX_SUSHISWAP   = "sushiswap";
    string public constant DEX_BASESWAP    = "baseswap";
    string public constant DEX_PANCAKESWAP = "pancakeswap";
    string public constant DEX_AERODROME   = "aerodrome";

    // ── Router bundle (constructor-injected) ─────────────────────────────
    struct Routers {
        address aaveProvider;       // address(0) → Aave not available on this chain
        address uniswapV3Router;
        address sushiswapV3Router;
        address baseswapV3Router;
        address pancakeswapV3Router;
        address aerodromeRouter;
        address balancerVault;      // address(0) → Balancer not available on this chain
    }

    IPoolAddressesProvider public immutable AAVE_PROVIDER;
    address public immutable UNISWAP_V3_ROUTER;
    address public immutable SUSHISWAP_V3_ROUTER;
    address public immutable BASESWAP_V3_ROUTER;
    address public immutable PANCAKESWAP_V3_ROUTER;
    address public immutable AERODROME_ROUTER;
    IAavePool public immutable POOL;
    IBalancerVault public immutable BALANCER_VAULT;

    // ── Errors ────────────────────────────────────────────────────────────
    error UnauthorizedCallback();
    error InitiatorMismatch();
    error UnprofitableArb(uint256 finalBalance, uint256 owed);
    error MinProfitNotMet(uint256 got, uint256 minRequired);
    error UnsupportedDex(string dex);
    error UnsupportedHopCount(uint8 numHops);
    error UnsupportedStrategy(uint8 strategy);
    error EmptyParams();
    error LockNotSet();
    error PositionStillHealthy(uint256 healthFactor);
    error NoCollateralSeized();
    error DexNotConfigured(string dex);
    error NoEntries();
    error BatchAssetMismatch();
    error ZeroBorrower();
    error AaveNotAvailable();
    error BalancerNotAvailable();
    error BalancerRepayShort();

    // ── Events ────────────────────────────────────────────────────────────
    event ArbExecuted(address indexed asset, uint256 borrowed, uint256 fee, uint256 profit);
    event RebaseArbExecuted(address indexed asset, uint256 borrowed, uint256 fee, uint256 profit);
    event FlashSwapArbExecuted(address indexed pool, address indexed asset, uint256 borrowed, uint256 profit);
    event BalancerArbExecuted(address indexed asset, uint256 borrowed, uint256 fee, uint256 profit);
    event LiquidationExecuted(
        address indexed debtAsset, address indexed collateralAsset, address indexed borrower,
        uint256 debtCovered, uint256 collateralSeized, uint256 fee, uint256 profit
    );
    event LiquidationComboExecuted(
        address indexed debtAsset, address indexed collateralAsset, address indexed borrower,
        uint256 debtCovered, uint256 collateralSeized, uint256 totalProfit
    );
    event BatchLiquidationExecuted(
        address indexed debtAsset, uint256 entryCount, uint256 successCount,
        uint256 totalBorrowed, uint256 premium, uint256 netProfit
    );
    event BatchEntryFailed(address indexed borrower, address indexed collateralAsset, string reason);
    event Withdrawn(address indexed token, uint256 amount, address indexed to);

    // ── Parameter structs ─────────────────────────────────────────────────
    struct ArbParams {
        uint8   numHops;
        address tokenA; address tokenB; address tokenC;
        uint24  fee1;   uint24  fee2;   uint24  fee3;
        string  dex1;   string  dex2;   string  dex3;
        uint256 minProfit;
    }

    struct LiqParams {
        address collateralAsset; address debtAsset; address borrower;
        uint256 debtToCover;
        string  sellDex; uint24 sellFee;
        uint256 minProfit;
    }

    struct LiqComboParams {
        address collateralAsset; address debtAsset; address borrower;
        uint256 debtToCover;
        string  sellDex;    uint24 sellFee;
        string  arbBuyDex;  uint24 arbBuyFee;
        string  arbSellDex; uint24 arbSellFee;
        uint256 arbAmountIn;
        uint256 minProfit;
    }

    struct BatchLiqEntry {
        address collateralAsset; address borrower;
        uint256 debtToCover;
        string  sellDex; uint24 sellFee;
    }

    struct BatchLiqParams {
        address         debtAsset;
        BatchLiqEntry[] entries;
        uint256         minProfit;
    }

    struct FlashSwapParams {
        uint8   numHops;
        address tokenA; address tokenB; address tokenC;
        uint24  fee2;   uint24  fee3;
        string  dex2;   string  dex3;
        uint256 minProfit;
    }

    struct BalancerArbParams {
        uint8   numHops;
        address tokenA; address tokenB; address tokenC;
        uint24  fee1;   uint24  fee2;   uint24  fee3;
        string  dex1;   string  dex2;   string  dex3;
        uint256 minProfit;
    }

    // ── Constructor ───────────────────────────────────────────────────────
    constructor(address owner_, Routers memory r) Ownable(owner_) { /* ... */ }

    receive() external payable {}

    // ── Entry points (owner-only) ─────────────────────────────────────────

    /// @notice Aave V3 flashloan-funded triangular / 2-hop arb.
    function executeArbitrage(address borrowToken, uint256 borrowAmount, bytes calldata params)
        external onlyOwner nonReentrant { /* ... */ }

    /// @notice Same as executeArbitrage but tagged as a scheduled rebase window.
    function executeRebaseArb(address borrowToken, uint256 borrowAmount, bytes calldata params)
        external onlyOwner nonReentrant { /* ... */ }

    /// @notice Uniswap V3 flash-swap arb (borrows from V3 pool directly).
    function executeFlashSwapArb(address pool, address tokenBorrow, uint256 amount, bytes calldata params)
        external onlyOwner nonReentrant { /* ... */ }

    /// @notice Balancer V2 vault flashloan arb (0-fee on most chains).
    function executeBalancerArb(address borrowToken, uint256 borrowAmount, bytes calldata params)
        external onlyOwner nonReentrant { /* ... */ }

    /// @notice Aave V3 flashloan-funded liquidation + collateral sell.
    function executeLiquidation(
        address collateralAsset, address debtAsset, address borrower,
        uint256 debtAmount, bytes calldata swapParams
    ) external onlyOwner nonReentrant { /* ... */ }

    /// @notice Liquidation + same-tx cross-DEX arb on the price gap opened by the sale.
    function executeLiquidationCombo(
        address collateralAsset, address debtAsset, address borrower,
        uint256 debtAmount, bytes calldata params
    ) external onlyOwner nonReentrant { /* ... */ }

    /// @notice Batch multiple liquidations into a single flashloan (shared debtAsset).
    function executeBatchLiquidations(BatchLiqParams calldata p)
        external onlyOwner nonReentrant { /* ... */ }

    // ── Flashloan callbacks ───────────────────────────────────────────────

    /// @dev Aave V3 callback.
    function executeOperation(
        address asset, uint256 amount, uint256 premium,
        address initiator, bytes calldata params
    ) external returns (bool) { /* ... */ }

    /// @dev Uniswap V3 flash callback.
    function uniswapV3FlashCallback(uint256 fee0, uint256 fee1, bytes calldata data)
        external nonReentrant { /* ... */ }

    /// @dev Balancer V2 callback.
    function receiveFlashLoan(
        address[] calldata tokens, uint256[] calldata amounts,
        uint256[] calldata feeAmounts, bytes calldata userData
    ) external nonReentrant { /* ... */ }

    // ── Owner withdrawals ─────────────────────────────────────────────────
    function withdrawToken(address token)                      external onlyOwner nonReentrant { /* ... */ }
    function withdrawETH()                                     external onlyOwner nonReentrant { /* ... */ }
    function emergencyWithdraw(address token, uint256 amount)  external onlyOwner nonReentrant { /* ... */ }

    // ── Internal helpers (not shown) ──────────────────────────────────────
    //
    //  _runArb / _runLiquidation / _runLiquidationCombo / _runBatchLiquidation
    //  _doLiquidation / _executeSwaps / _swap / _swapUniV3 / _swapAerodrome
    //  Transient-storage lock/strategy/flash-pool/balance-snapshot helpers
    //
    //  Full source available on request — DM @Just-Code-Builder on GitHub.
}
