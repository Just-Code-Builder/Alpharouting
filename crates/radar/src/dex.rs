//! Bindings for the factory/pool/token reads the radar needs.
//!
//! Both Uniswap V2- and V3-style factories are supported because which one a
//! new chain ships first is not predictable — most EVM chains get a V2 fork
//! before a V3 one.

use alloy::sol;

sol! {
    #[sol(rpc)]
    interface IUniswapV2Factory {
        event PairCreated(address indexed token0, address indexed token1, address pair, uint256 index);
        function getPair(address tokenA, address tokenB) external view returns (address pair);
    }

    #[sol(rpc)]
    interface IUniswapV2Pair {
        event Swap(
            address indexed sender,
            uint256 amount0In,
            uint256 amount1In,
            uint256 amount0Out,
            uint256 amount1Out,
            address indexed to
        );
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
        function token0() external view returns (address);
        function token1() external view returns (address);
        function totalSupply() external view returns (uint256);
        function balanceOf(address owner) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IUniswapV3Factory {
        event PoolCreated(
            address indexed token0,
            address indexed token1,
            uint24 indexed fee,
            int24 tickSpacing,
            address pool
        );
    }

    #[sol(rpc)]
    interface IUniswapV3Pool {
        event Swap(
            address indexed sender,
            address indexed recipient,
            int256 amount0,
            int256 amount1,
            uint160 sqrtPriceX96,
            uint128 liquidity,
            int24 tick
        );
        function token0() external view returns (address);
        function token1() external view returns (address);
        function fee() external view returns (uint24);
        function liquidity() external view returns (uint128);
    }

    #[sol(rpc)]
    interface IERC20Meta {
        event Transfer(address indexed from, address indexed to, uint256 value);
        function totalSupply() external view returns (uint256);
        function balanceOf(address account) external view returns (uint256);
        function decimals() external view returns (uint8);
        function symbol() external view returns (string);
    }
}

/// Which factory flavour the radar is watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexKind {
    UniswapV2,
    UniswapV3,
}

impl DexKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "v2" | "uniswapv2" | "uniswap_v2" => Some(DexKind::UniswapV2),
            "v3" | "uniswapv3" | "uniswap_v3" => Some(DexKind::UniswapV3),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_dex_kinds() {
        assert_eq!(DexKind::parse("v2"), Some(DexKind::UniswapV2));
        assert_eq!(DexKind::parse("V3"), Some(DexKind::UniswapV3));
        assert_eq!(DexKind::parse("uniswap_v3"), Some(DexKind::UniswapV3));
    }

    #[test]
    fn rejects_unknown_dex_kinds() {
        assert_eq!(DexKind::parse("sushiswap-v4"), None);
        assert_eq!(DexKind::parse(""), None);
    }
}
