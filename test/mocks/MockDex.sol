// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

/// Minimal Uniswap V2-shaped factory/pair pair, used to exercise the radar's
/// log-filtering and contract-read paths against a local node. Event
/// signatures match the real ones exactly — that's the whole point, since a
/// mismatched topic0 is the failure mode this is meant to catch.
contract MockV2Pair {
    address public token0;
    address public token1;
    uint112 private reserve0;
    uint112 private reserve1;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;

    event Swap(
        address indexed sender,
        uint256 amount0In,
        uint256 amount1In,
        uint256 amount0Out,
        uint256 amount1Out,
        address indexed to
    );

    constructor(address token0_, address token1_) {
        token0 = token0_;
        token1 = token1_;
    }

    function setReserves(uint112 reserve0_, uint112 reserve1_) external {
        reserve0 = reserve0_;
        reserve1 = reserve1_;
    }

    function getReserves() external view returns (uint112, uint112, uint32) {
        return (reserve0, reserve1, uint32(block.timestamp));
    }

    function mintLp(address to, uint256 amount) external {
        totalSupply += amount;
        balanceOf[to] += amount;
    }

    function emitSwap(
        uint256 amount0In,
        uint256 amount1In,
        uint256 amount0Out,
        uint256 amount1Out,
        address to
    ) external {
        emit Swap(msg.sender, amount0In, amount1In, amount0Out, amount1Out, to);
    }
}

contract MockV2Factory {
    event PairCreated(address indexed token0, address indexed token1, address pair, uint256 index);

    uint256 public pairCount;
    mapping(address => mapping(address => address)) public getPair;

    function createPair(address tokenA, address tokenB) external returns (address pair) {
        (address t0, address t1) = tokenA < tokenB ? (tokenA, tokenB) : (tokenB, tokenA);
        pair = address(new MockV2Pair(t0, t1));
        getPair[t0][t1] = pair;
        pairCount++;
        emit PairCreated(t0, t1, pair, pairCount);
    }
}
