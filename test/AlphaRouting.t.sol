// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

import {Test} from "forge-std/Test.sol";
import {AlphaRouting} from "../src/AlphaRouting.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {MockERC20} from "./mocks/MockERC20.sol";
import {MockAavePool} from "./mocks/MockAavePool.sol";
import {MockSwapRouter} from "./mocks/MockSwapRouter.sol";

contract AlphaRoutingLiquidationTest is Test {
    AlphaRouting internal arb;
    MockAavePool internal pool;
    MockSwapRouter internal router;
    MockERC20 internal usdc; // debt asset
    MockERC20 internal weth; // collateral asset

    address internal owner = makeAddr("owner");
    address internal borrower = makeAddr("borrower");
    address internal stranger = makeAddr("stranger");

    function setUp() public {
        pool = new MockAavePool();
        router = new MockSwapRouter();
        usdc = new MockERC20("USD Coin", "USDC", 6);
        weth = new MockERC20("Wrapped Ether", "WETH", 18);

        AlphaRouting.Routers memory r = AlphaRouting.Routers({
            aaveProvider: address(pool),
            uniswapV3Router: address(router),
            sushiswapV3Router: address(0),
            baseswapV3Router: address(0),
            pancakeswapV3Router: address(0),
            aerodromeRouter: address(0),
            balancerVault: address(0)
        });

        vm.prank(owner);
        arb = new AlphaRouting(owner, r);

        // Fund the mock pool so it can pay out the flashloan and the seized collateral.
        usdc.mint(address(pool), 1_000_000e6);
        weth.mint(address(pool), 1_000e18);
        // Fund the mock router so it can pay out swap proceeds.
        usdc.mint(address(router), 1_000_000e6);
    }

    function _liqParams(string memory dex, uint24 fee, uint256 minProfit) internal pure returns (bytes memory) {
        return abi.encode(dex, fee, minProfit);
    }

    function test_profitableLiquidation_succeeds() public {
        pool.setHealthFactor(0.95e18); // liquidatable
        pool.setCollateralRate(1e27);  // 1000e6 USDC debt covered -> 1e18 (1 whole) WETH seized
        router.setRate(1166e6);        // 1 WETH -> 1166 USDC: comfortably covers the 1000 USDC + 0.09% premium

        uint256 debtAmount = 1000e6;

        vm.prank(owner);
        arb.executeLiquidation(address(weth), address(usdc), borrower, debtAmount, _liqParams("uniswap", 3000, 1));

        // Profit should have stuck to the contract for the owner to withdraw.
        assertGt(usdc.balanceOf(address(arb)), 0);
    }

    function test_revertsWhen_notOwner() public {
        vm.prank(stranger);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, stranger));
        arb.executeLiquidation(address(weth), address(usdc), borrower, 1000e6, _liqParams("uniswap", 3000, 0));
    }

    function test_revertsWhen_positionStillHealthy() public {
        pool.setHealthFactor(1.2e18); // not liquidatable

        vm.prank(owner);
        vm.expectRevert(abi.encodeWithSelector(AlphaRouting.PositionStillHealthy.selector, uint256(1.2e18)));
        arb.executeLiquidation(address(weth), address(usdc), borrower, 1000e6, _liqParams("uniswap", 3000, 0));
    }

    function test_revertsWhen_minProfitNotMet() public {
        pool.setHealthFactor(0.9e18);
        pool.setCollateralRate(1e27); // 1000e6 debt -> 1e18 (1 whole) WETH collateral
        router.setRate(1005e6);       // 1 WETH -> 1005 USDC: small real profit, but we demand way more

        vm.prank(owner);
        vm.expectRevert(abi.encodeWithSelector(AlphaRouting.MinProfitNotMet.selector, uint256(4.1e6), uint256(1_000_000e6)));
        arb.executeLiquidation(address(weth), address(usdc), borrower, 1000e6, _liqParams("uniswap", 3000, 1_000_000e6));
    }

    function test_revertsWhen_callbackCalledDirectly() public {
        vm.prank(stranger);
        vm.expectRevert(AlphaRouting.UnauthorizedCallback.selector);
        arb.executeOperation(address(usdc), 1000e6, 1e6, address(arb), "");
    }

    function test_revertsWhen_aaveNotAvailable() public {
        AlphaRouting.Routers memory r = AlphaRouting.Routers({
            aaveProvider: address(0),
            uniswapV3Router: address(router),
            sushiswapV3Router: address(0),
            baseswapV3Router: address(0),
            pancakeswapV3Router: address(0),
            aerodromeRouter: address(0),
            balancerVault: address(0)
        });

        vm.prank(owner);
        AlphaRouting monadArb = new AlphaRouting(owner, r);

        vm.prank(owner);
        vm.expectRevert(AlphaRouting.AaveNotAvailable.selector);
        monadArb.executeLiquidation(address(weth), address(usdc), borrower, 1000e6, _liqParams("uniswap", 3000, 0));
    }

    function test_unimplementedStrategies_revertExplicitly() public {
        vm.startPrank(owner);

        vm.expectRevert(AlphaRouting.NotYetImplemented.selector);
        arb.executeArbitrage(address(usdc), 1000e6, "");

        vm.expectRevert(AlphaRouting.NotYetImplemented.selector);
        arb.executeFlashSwapArb(address(0), address(usdc), 1000e6, "");

        vm.expectRevert(AlphaRouting.NotYetImplemented.selector);
        arb.executeBalancerArb(address(usdc), 1000e6, "");

        vm.stopPrank();
    }

    function test_ownerCanWithdrawProfit() public {
        pool.setHealthFactor(0.95e18);
        pool.setCollateralRate(1e27);
        router.setRate(1166e6);

        vm.prank(owner);
        arb.executeLiquidation(address(weth), address(usdc), borrower, 1000e6, _liqParams("uniswap", 3000, 1));

        uint256 contractBal = usdc.balanceOf(address(arb));
        assertGt(contractBal, 0);

        vm.prank(owner);
        arb.withdrawToken(address(usdc));

        assertEq(usdc.balanceOf(address(arb)), 0);
        assertEq(usdc.balanceOf(owner), contractBal);
    }
}
