// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

import {Script, console2} from "forge-std/Script.sol";
import {MockERC20} from "../test/mocks/MockERC20.sol";
import {MockV2Factory, MockV2Pair} from "../test/mocks/MockDex.sol";

/// Stages a realistic token launch on a local node so the radar can be run
/// against real RPC responses rather than only unit-test fixtures.
///
///   anvil &
///   forge script script/RadarDemo.s.sol --rpc-url http://127.0.0.1:8545 --broadcast
///
/// Then export the addresses it prints and run `cargo run -p radar`.
contract RadarDemo is Script {
    address constant BURN = 0x000000000000000000000000000000000000dEaD;

    function run() external {
        uint256 pk = vm.envOr("PRIVATE_KEY", uint256(0));
        address deployer = pk != 0 ? vm.addr(pk) : msg.sender;

        if (pk != 0) { vm.startBroadcast(pk); } else { vm.startBroadcast(); }

        MockERC20 weth = new MockERC20("Wrapped Ether", "WETH", 18);
        MockERC20 token = new MockERC20("Demo Token", "DEMO", 18);
        MockV2Factory factory = new MockV2Factory();

        // Supply mints to the deployer, then spreads across holders — this is
        // the Transfer history the radar reconstructs balances from.
        token.mint(deployer, 1_000_000e18);
        token.transfer(address(0x1111111111111111111111111111111111111111), 120_000e18);
        token.transfer(address(0x2222222222222222222222222222222222222222), 90_000e18);
        token.transfer(address(0x3333333333333333333333333333333333333333), 60_000e18);
        token.transfer(address(0x4444444444444444444444444444444444444444), 30_000e18);
        token.transfer(address(0x5555555555555555555555555555555555555555), 10_000e18);

        address pair = factory.createPair(address(token), address(weth));

        // 10 units on each side; the radar reads whichever side is the quote
        // token, so either ordering yields the same valuation.
        MockV2Pair(pair).setReserves(10e18, 10e18);

        // Park the token side of the pool in the pair, as a real launch would.
        token.transfer(pair, 600_000e18);

        // LP fully burned — the "safe" case for the LP signal.
        MockV2Pair(pair).mintLp(BURN, 1_000e18);

        bool tokenIsToken0 = address(token) < address(weth);
        // Three buys, one sell: token leaves the pool on a buy.
        for (uint256 i = 0; i < 3; i++) {
            if (tokenIsToken0) {
                MockV2Pair(pair).emitSwap(0, 1e18, 50e18, 0, deployer);
            } else {
                MockV2Pair(pair).emitSwap(1e18, 0, 0, 50e18, deployer);
            }
        }
        if (tokenIsToken0) {
            MockV2Pair(pair).emitSwap(25e18, 0, 0, 5e17, deployer);
        } else {
            MockV2Pair(pair).emitSwap(0, 25e18, 5e17, 0, deployer);
        }

        vm.stopBroadcast();

        console2.log("export DEX_FACTORY=%s", address(factory));
        console2.log("export QUOTE_TOKEN=%s", address(weth));
        console2.log("token (DEMO):      %s", address(token));
        console2.log("pair:              %s", pair);
    }
}
