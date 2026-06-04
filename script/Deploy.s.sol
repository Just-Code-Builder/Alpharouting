// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

import {Script, console2} from "forge-std/Script.sol";
import {AlphaRouting} from "../src/AlphaRouting.sol";

/// Deploys AlphaRouting. Router addresses are resolved by chainId so the
/// same script handles every supported network.
///
///   Base mainnet  (8453)  — Aave V3 ✓  Uniswap V3 ✓  Balancer V2 ✓  Aerodrome ✓
///   Base Sepolia  (84532) — Aave V3 ✓  Uniswap V3 ✓  (others not on testnet)
///   Monad mainnet (41454) — Aave V3 ✗  Uniswap V3 ✓  Balancer V2 ✗
///
/// Base mainnet:  forge script script/Deploy.s.sol --rpc-url $BASE_RPC_URL  --broadcast --verify
/// Base Sepolia:  forge script script/Deploy.s.sol --rpc-url $BASE_SEPOLIA_RPC_URL --broadcast --verify
/// Monad mainnet: forge script script/Deploy.s.sol --rpc-url $MONAD_RPC_URL --broadcast
contract Deploy is Script {
    uint256 constant CHAIN_BASE_MAINNET  = 8453;
    uint256 constant CHAIN_BASE_SEPOLIA  = 84532;
    uint256 constant CHAIN_MONAD_MAINNET = 41454;

    // Deterministic across Ethereum, Polygon, Arbitrum, Optimism, Base.
    address constant BALANCER_V2_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;

    function run() external returns (AlphaRouting arb) {
        uint256 pk    = vm.envOr("PRIVATE_KEY", uint256(0));
        address owner = pk != 0 ? vm.addr(pk) : msg.sender;
        uint256 cid   = block.chainid;

        AlphaRouting.Routers memory r;
        if (cid == CHAIN_BASE_MAINNET) {
            r = mainnetRouters();
            console2.log("Deploying to Base mainnet");
        } else if (cid == CHAIN_BASE_SEPOLIA) {
            r = baseSepoliaRouters();
            console2.log("Deploying to Base Sepolia");
        } else if (cid == CHAIN_MONAD_MAINNET) {
            r = monadRouters();
            console2.log("Deploying to Monad mainnet (FlashSwap arb only)");
        } else {
            revert(string.concat("unsupported chainId: ", vm.toString(cid)));
        }
        console2.log("Owner:", owner);

        if (pk != 0) { vm.startBroadcast(pk); } else { vm.startBroadcast(); }
        arb = new AlphaRouting(owner, r);
        vm.stopBroadcast();

        console2.log("AlphaRouting deployed at:", address(arb));
        if (r.aaveProvider != address(0)) {
            console2.log("Aave Pool:", address(arb.POOL()));
        } else {
            console2.log("Aave: not available on this chain");
        }
        if (r.balancerVault != address(0)) {
            console2.log("Balancer V2 Vault:", address(arb.BALANCER_VAULT()));
        } else {
            console2.log("Balancer: not available on this chain");
        }
    }

    function mainnetRouters() public pure returns (AlphaRouting.Routers memory) {
        return AlphaRouting.Routers({
            aaveProvider:        0xe20fCBdBfFC4Dd138cE8b2E6FBb6CB49777ad64D,
            uniswapV3Router:     0x2626664c2603336E57B271c5C0b26F421741e481,
            sushiswapV3Router:   0xfb7Ef66c7e90876087B9b1d3eAaAf45EeF0FC53a,
            baseswapV3Router:    0x1B8eea9315bE495187D873DA7773a874545D9D48,
            pancakeswapV3Router: 0x678Aa4bF4E210cf2166753e054d5b7c31cc7fa86,
            aerodromeRouter:     0xcF77a3Ba9A5CA399B7c97c74d54e5b1Beb874E43,
            balancerVault:       BALANCER_V2_VAULT
        });
    }

    // Sushi V3, BaseSwap V3, PancakeSwap V3, Aerodrome, and Balancer V2 have no
    // Base Sepolia deployments — address(0) disables those swap paths.
    function baseSepoliaRouters() public pure returns (AlphaRouting.Routers memory) {
        return AlphaRouting.Routers({
            aaveProvider:        0xd449FeD49d9C443688d6816fE6872F21402e41de,
            uniswapV3Router:     0x94cC0AaC535CCDB3C01d6787D6413C739ae12bc4,
            sushiswapV3Router:   address(0),
            baseswapV3Router:    address(0),
            pancakeswapV3Router: address(0),
            aerodromeRouter:     address(0),
            balancerVault:       address(0)
        });
    }

    // Aave V3 and Balancer V2 are not deployed on Monad — only the Uniswap V3
    // flash-swap path (executeFlashSwapArb) is active. All Aave/Balancer entry
    // points revert with AaveNotAvailable / BalancerNotAvailable.
    //
    // ⚠ uniswapV3Router: replace with the verified V3-compatible router address
    //   for Monad before going live. Source from the Monad ecosystem docs.
    function monadRouters() public pure returns (AlphaRouting.Routers memory) {
        return AlphaRouting.Routers({
            aaveProvider:        address(0),
            uniswapV3Router:     address(0), // ⚠ set before deployment
            sushiswapV3Router:   address(0),
            baseswapV3Router:    address(0),
            pancakeswapV3Router: address(0),
            aerodromeRouter:     address(0),
            balancerVault:       address(0)
        });
    }
}
