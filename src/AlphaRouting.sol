// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

// ─────────────────────────────────────────────────────────────────────────────
//  AlphaRouting — Aave V3 flashloan executor (Base mainnet / Base Sepolia)
//
//  Liquidation strategy is fully implemented. The other six strategies
//  (triangular arb, flash-swap arb, Balancer arb, liquidation combo, batch
//  liquidation, rebase arb) are not yet built — their entry points revert
//  with NotYetImplemented rather than silently no-op.
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
///         Routers are constructor-injected so the same bytecode works across
///         chains and testnets — an address(0) router disables that path.
contract AlphaRouting is Ownable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    uint256 private constant HEALTH_FACTOR_LIQUIDATION_THRESHOLD = 1e18;

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

    // ── Transient storage slots (EIP-1153) ───────────────────────────────
    // Unstructured slots so future strategies can add more without collisions.
    bytes32 private constant LOCK_SLOT     = keccak256("alpharouting.tstore.lock");
    bytes32 private constant STRATEGY_SLOT = keccak256("alpharouting.tstore.strategy");
    bytes32 private constant SNAPSHOT_SLOT = keccak256("alpharouting.tstore.snapshot");

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
    error NotYetImplemented();
    error EthTransferFailed();

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
    constructor(address owner_, Routers memory r) Ownable(owner_) {
        UNISWAP_V3_ROUTER     = r.uniswapV3Router;
        SUSHISWAP_V3_ROUTER   = r.sushiswapV3Router;
        BASESWAP_V3_ROUTER    = r.baseswapV3Router;
        PANCAKESWAP_V3_ROUTER = r.pancakeswapV3Router;
        AERODROME_ROUTER      = r.aerodromeRouter;
        BALANCER_VAULT        = IBalancerVault(r.balancerVault);

        if (r.aaveProvider != address(0)) {
            AAVE_PROVIDER = IPoolAddressesProvider(r.aaveProvider);
            POOL = IAavePool(AAVE_PROVIDER.getPool());
        } else {
            AAVE_PROVIDER = IPoolAddressesProvider(address(0));
            POOL = IAavePool(address(0));
        }
    }

    receive() external payable {}

    // ── Entry points (owner-only) ─────────────────────────────────────────

    /// @notice Aave V3 flashloan-funded triangular / 2-hop arb.
    function executeArbitrage(address, uint256, bytes calldata) external view onlyOwner {
        revert NotYetImplemented();
    }

    /// @notice Same as executeArbitrage but tagged as a scheduled rebase window.
    function executeRebaseArb(address, uint256, bytes calldata) external view onlyOwner {
        revert NotYetImplemented();
    }

    /// @notice Uniswap V3 flash-swap arb (borrows from V3 pool directly).
    function executeFlashSwapArb(address, address, uint256, bytes calldata) external view onlyOwner {
        revert NotYetImplemented();
    }

    /// @notice Balancer V2 vault flashloan arb (0-fee on most chains).
    function executeBalancerArb(address, uint256, bytes calldata) external view onlyOwner {
        revert NotYetImplemented();
    }

    /// @notice Aave V3 flashloan-funded liquidation + collateral sell.
    /// @param  swapParams abi.encode(string sellDex, uint24 sellFee, uint256 minProfit)
    function executeLiquidation(
        address collateralAsset,
        address debtAsset,
        address borrower,
        uint256 debtAmount,
        bytes calldata swapParams
    ) external onlyOwner nonReentrant {
        if (address(POOL) == address(0)) revert AaveNotAvailable();
        if (borrower == address(0)) revert ZeroBorrower();

        (string memory sellDex, uint24 sellFee, uint256 minProfit) =
            abi.decode(swapParams, (string, uint24, uint256));

        LiqParams memory p = LiqParams({
            collateralAsset: collateralAsset,
            debtAsset: debtAsset,
            borrower: borrower,
            debtToCover: debtAmount,
            sellDex: sellDex,
            sellFee: sellFee,
            minProfit: minProfit
        });

        _setLock(true);
        _setStrategy(STRAT_LIQ);
        _setSnapshot(IERC20(debtAsset).balanceOf(address(this)));

        POOL.flashLoanSimple(address(this), debtAsset, debtAmount, abi.encode(p), 0);

        _setLock(false);
    }

    /// @notice Liquidation + same-tx cross-DEX arb on the price gap opened by the sale.
    function executeLiquidationCombo(address, address, address, uint256, bytes calldata)
        external
        view
        onlyOwner
    {
        revert NotYetImplemented();
    }

    /// @notice Batch multiple liquidations into a single flashloan (shared debtAsset).
    function executeBatchLiquidations(BatchLiqParams calldata) external view onlyOwner {
        revert NotYetImplemented();
    }

    // ── Flashloan callbacks ───────────────────────────────────────────────

    /// @dev Aave V3 callback. `initiator == address(this)` is only true when this
    ///      contract itself called flashLoanSimple, so `params` is trusted.
    function executeOperation(
        address asset,
        uint256 amount,
        uint256 premium,
        address initiator,
        bytes calldata params
    ) external returns (bool) {
        if (msg.sender != address(POOL)) revert UnauthorizedCallback();
        if (initiator != address(this)) revert InitiatorMismatch();
        if (!_isLocked()) revert LockNotSet();

        uint8 strat = _getStrategy();
        if (strat == STRAT_LIQ) {
            _runLiquidation(asset, amount, premium, params);
        } else {
            revert UnsupportedStrategy(strat);
        }

        IERC20(asset).forceApprove(address(POOL), amount + premium);
        return true;
    }

    /// @dev Uniswap V3 flash callback. Not wired up yet — no strategy uses it.
    function uniswapV3FlashCallback(uint256, uint256, bytes calldata) external nonReentrant {
        revert NotYetImplemented();
    }

    /// @dev Balancer V2 callback. Not wired up yet — no strategy uses it.
    function receiveFlashLoan(
        address[] calldata,
        uint256[] calldata,
        uint256[] calldata,
        bytes calldata
    ) external nonReentrant {
        revert NotYetImplemented();
    }

    // ── Strategy implementation ───────────────────────────────────────────

    /// @dev Liquidate `p.borrower`'s position, sell the seized collateral for
    ///      `debtAsset`, and require enough proceeds to repay the flashloan
    ///      plus `p.minProfit`. Reverts (undoing the liquidation) otherwise.
    function _runLiquidation(address debtAsset, uint256 amount, uint256 premium, bytes calldata params)
        private
    {
        LiqParams memory p = abi.decode(params, (LiqParams));

        (,,,,, uint256 healthFactor) = ILiquidator(address(POOL)).getUserAccountData(p.borrower);
        if (healthFactor >= HEALTH_FACTOR_LIQUIDATION_THRESHOLD) revert PositionStillHealthy(healthFactor);

        uint256 collateralBefore = IERC20(p.collateralAsset).balanceOf(address(this));

        IERC20(debtAsset).forceApprove(address(POOL), p.debtToCover);
        ILiquidator(address(POOL)).liquidationCall(p.collateralAsset, debtAsset, p.borrower, p.debtToCover, false);

        uint256 collateralSeized = IERC20(p.collateralAsset).balanceOf(address(this)) - collateralBefore;
        if (collateralSeized == 0) revert NoCollateralSeized();

        if (p.collateralAsset != debtAsset) {
            _swap(p.sellDex, p.collateralAsset, debtAsset, p.sellFee, collateralSeized);
        }

        uint256 owed = amount + premium;
        uint256 snapshot = _getSnapshot();
        uint256 finalBalance = IERC20(debtAsset).balanceOf(address(this));

        if (finalBalance < snapshot + owed) revert UnprofitableArb(finalBalance, snapshot + owed);

        uint256 profit = finalBalance - snapshot - owed;
        if (profit < p.minProfit) revert MinProfitNotMet(profit, p.minProfit);

        emit LiquidationExecuted(debtAsset, p.collateralAsset, p.borrower, p.debtToCover, collateralSeized, premium, profit);
    }

    /// @dev Routes a single-hop swap to the named DEX. Slippage is bounded by
    ///      the overall minProfit check in the caller, not per-swap here.
    function _swap(string memory dex, address tokenIn, address tokenOut, uint24 fee, uint256 amountIn)
        private
        returns (uint256 amountOut)
    {
        address router = _routerFor(dex);
        IERC20(tokenIn).forceApprove(router, amountIn);

        if (_isDex(dex, DEX_AERODROME)) {
            IAerodromeRouter.Route[] memory routes = new IAerodromeRouter.Route[](1);
            routes[0] = IAerodromeRouter.Route({
                from: tokenIn,
                to: tokenOut,
                stable: false,
                factory: IAerodromeRouter(router).defaultFactory()
            });
            uint256[] memory amounts = IAerodromeRouter(router).swapExactTokensForTokens(
                amountIn, 0, routes, address(this), block.timestamp
            );
            amountOut = amounts[amounts.length - 1];
        } else {
            amountOut = IUniswapV3SwapRouter(router).exactInputSingle(
                IUniswapV3SwapRouter.ExactInputSingleParams({
                    tokenIn: tokenIn,
                    tokenOut: tokenOut,
                    fee: fee,
                    recipient: address(this),
                    amountIn: amountIn,
                    amountOutMinimum: 0,
                    sqrtPriceLimitX96: 0
                })
            );
        }
    }

    function _routerFor(string memory dex) private view returns (address router) {
        if (_isDex(dex, DEX_UNISWAP))          router = UNISWAP_V3_ROUTER;
        else if (_isDex(dex, DEX_SUSHISWAP))   router = SUSHISWAP_V3_ROUTER;
        else if (_isDex(dex, DEX_BASESWAP))    router = BASESWAP_V3_ROUTER;
        else if (_isDex(dex, DEX_PANCAKESWAP)) router = PANCAKESWAP_V3_ROUTER;
        else if (_isDex(dex, DEX_AERODROME))   router = AERODROME_ROUTER;
        else revert UnsupportedDex(dex);

        if (router == address(0)) revert DexNotConfigured(dex);
    }

    function _isDex(string memory dex, string memory candidate) private pure returns (bool) {
        return keccak256(bytes(dex)) == keccak256(bytes(candidate));
    }

    // ── Transient storage (EIP-1153) helpers ──────────────────────────────
    // Authenticate flashloan callbacks from state only this contract can set
    // in the same transaction, never from caller-supplied calldata.

    function _setLock(bool locked) private {
        uint256 v = locked ? 1 : 0;
        bytes32 slot = LOCK_SLOT;
        assembly { tstore(slot, v) }
    }

    function _isLocked() private view returns (bool locked) {
        bytes32 slot = LOCK_SLOT;
        uint256 v;
        assembly { v := tload(slot) }
        locked = v == 1;
    }

    function _setStrategy(uint8 s) private {
        bytes32 slot = STRATEGY_SLOT;
        assembly { tstore(slot, s) }
    }

    function _getStrategy() private view returns (uint8 s) {
        bytes32 slot = STRATEGY_SLOT;
        uint256 v;
        assembly { v := tload(slot) }
        s = uint8(v);
    }

    function _setSnapshot(uint256 bal) private {
        bytes32 slot = SNAPSHOT_SLOT;
        assembly { tstore(slot, bal) }
    }

    function _getSnapshot() private view returns (uint256 bal) {
        bytes32 slot = SNAPSHOT_SLOT;
        assembly { bal := tload(slot) }
    }

    // ── Owner withdrawals ─────────────────────────────────────────────────
    function withdrawToken(address token) external onlyOwner nonReentrant {
        uint256 bal = IERC20(token).balanceOf(address(this));
        IERC20(token).safeTransfer(owner(), bal);
        emit Withdrawn(token, bal, owner());
    }

    function withdrawETH() external onlyOwner nonReentrant {
        uint256 bal = address(this).balance;
        (bool ok,) = owner().call{value: bal}("");
        if (!ok) revert EthTransferFailed();
        emit Withdrawn(address(0), bal, owner());
    }

    function emergencyWithdraw(address token, uint256 amount) external onlyOwner nonReentrant {
        IERC20(token).safeTransfer(owner(), amount);
        emit Withdrawn(token, amount, owner());
    }
}
