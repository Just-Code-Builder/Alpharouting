// Per-chain address registry.
//
// The bot resolves its target chain via CHAIN_ID at startup and loads every
// protocol address from here. Adding a new chain means adding one `ChainSpec`
// constant — no changes to scanner or executor code.
//
// ChainStatus controls strategy gating at runtime:
//   Live     — all dependencies deployed; bot fires normally.
//   Preview  — chain exists but infrastructure not fully deployed yet; bot
//              boots and monitors but refuses to broadcast trades.
//   Disabled — support pulled; bot refuses to start on this chain ID.

use alloy::primitives::{address, Address};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainStatus { Live, Preview, Disabled }

#[derive(Clone, Copy, Debug)]
pub struct ChainSpec {
    pub chain_id:  u64,
    pub name:      &'static str,
    pub status:    ChainStatus,
    // Aave V3
    pub aave_pool:                  Address,
    pub aave_addresses_provider:    Address,
    pub aave_ui_pool_data_provider: Address,
    pub aave_pool_data_provider:    Address,
    // Shared infra
    pub multicall3: Address,
    pub usdc:       Address,
    pub weth:       Address,
    /// `None` = no Chainlink feed; oracle-boundary strategy disables itself.
    pub chainlink_eth_usd: Option<Address>,
    /// `None` = Balancer V2 not on this chain (BNB, Monad). Scanner picks
    /// Aave 5 bps flashloan instead of Balancer 0 bps.
    pub balancer_vault: Option<Address>,
    pub ws_hint_substring: &'static str,
}

/// Canonical Balancer V2 Vault — same address on every supported chain.
pub const BALANCER_V2_VAULT: Address = address!("BA12222222228d8Ba445958a75a0704d566BF2C8");

impl ChainSpec {
    pub fn lending_protocol_available(&self) -> bool {
        self.aave_pool != Address::ZERO
    }

    pub fn oracle_boundary_trigger_available(&self) -> bool {
        self.chainlink_eth_usd.is_some() && self.lending_protocol_available()
    }

    pub fn balancer_available(&self) -> bool {
        self.balancer_vault.is_some()
    }
}

// ── Base mainnet ─────────────────────────────────────────────────────────────
pub const BASE_MAINNET: ChainSpec = ChainSpec {
    chain_id: 8453,
    name:     "Base",
    status:   ChainStatus::Live,
    aave_pool:                    address!("A238Dd80C259a72e81d7e4664a9801593F98d1c5"),
    aave_addresses_provider:      address!("e20fCBdBfFC4Dd138cE8b2E6FBb6CB49777ad64D"),
    aave_ui_pool_data_provider:   address!("68100bD5345eA474D93577127C11F39FF8463e93"),
    aave_pool_data_provider:      address!("d82a47fdebB5bf5329b09441C3DaB4b5df2153Ad"),
    multicall3:                   address!("cA11bde05977b3631167028862bE2a173976CA11"),
    usdc:                         address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
    weth:                         address!("4200000000000000000000000000000000000006"),
    chainlink_eth_usd: Some(address!("71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70")),
    balancer_vault:    Some(BALANCER_V2_VAULT),
    ws_hint_substring: "g.alchemy.com",
};

// ── Ethereum mainnet ──────────────────────────────────────────────────────────
pub const ETHEREUM_MAINNET: ChainSpec = ChainSpec {
    chain_id: 1,
    name:     "Ethereum",
    status:   ChainStatus::Live,
    aave_pool:                    address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
    aave_addresses_provider:      address!("2f39d218133AFaB8F2B819B1066c7E434Ad94E9e"),
    aave_ui_pool_data_provider:   address!("3F78BBD206e4D3c504Eb854232EdA7e47E9Fd8FC"),
    aave_pool_data_provider:      address!("41393e5e337606dc3821075Af65AeE84D7688CBD"),
    multicall3:                   address!("cA11bde05977b3631167028862bE2a173976CA11"),
    usdc:                         address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
    weth:                         address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
    chainlink_eth_usd: Some(address!("5f4eC3Df9cbd43714FE2740f5E3616155c5b8419")),
    balancer_vault:    Some(BALANCER_V2_VAULT),
    ws_hint_substring: "g.alchemy.com",
};

// ── Arbitrum One ──────────────────────────────────────────────────────────────
pub const ARBITRUM_ONE: ChainSpec = ChainSpec {
    chain_id: 42161,
    name:     "Arbitrum",
    status:   ChainStatus::Live,
    aave_pool:                    address!("794a61358D6845594F94dc1DB02A252b5b4814aD"),
    aave_addresses_provider:      address!("a97684ead0e402dC232d5A977953DF7ECBaB3CDb"),
    aave_ui_pool_data_provider:   address!("5c5228aC8BC1528482514aF3e27E692495148717"),
    aave_pool_data_provider:      address!("69FA688f1Dc47d4B5d8029D5a35FB7a548310654"),
    multicall3:                   address!("cA11bde05977b3631167028862bE2a173976CA11"),
    usdc:                         address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
    weth:                         address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
    chainlink_eth_usd: Some(address!("639Fe6ab55C921f74e7fac1ee960C0B6293ba612")),
    balancer_vault:    Some(BALANCER_V2_VAULT),
    ws_hint_substring: "g.alchemy.com",
};

// ── Optimism ──────────────────────────────────────────────────────────────────
pub const OPTIMISM: ChainSpec = ChainSpec {
    chain_id: 10,
    name:     "Optimism",
    status:   ChainStatus::Live,
    aave_pool:                    address!("794a61358D6845594F94dc1DB02A252b5b4814aD"),
    aave_addresses_provider:      address!("a97684ead0e402dC232d5A977953DF7ECBaB3CDb"),
    aave_ui_pool_data_provider:   address!("9b1Ef85D44778b224b6Ae4C00194E5C690091cAa"),
    aave_pool_data_provider:      address!("7F23D86Ee20D869112572136221e173428DD740B"),
    multicall3:                   address!("cA11bde05977b3631167028862bE2a173976CA11"),
    usdc:                         address!("0b2C639c533813f4Aa9D7837CAf62653d097Ff85"),
    weth:                         address!("4200000000000000000000000000000000000006"),
    chainlink_eth_usd: Some(address!("13e3Ee699D1909E989722E753853AE30b17e08c5")),
    balancer_vault:    Some(BALANCER_V2_VAULT),
    ws_hint_substring: "g.alchemy.com",
};

// ── Polygon ───────────────────────────────────────────────────────────────────
pub const POLYGON: ChainSpec = ChainSpec {
    chain_id: 137,
    name:     "Polygon",
    status:   ChainStatus::Live,
    aave_pool:                    address!("794a61358D6845594F94dc1DB02A252b5b4814aD"),
    aave_addresses_provider:      address!("a97684ead0e402dC232d5A977953DF7ECBaB3CDb"),
    aave_ui_pool_data_provider:   address!("c4806d24da0d6d28f48a0DAa9b1F39A4F4cB6E3F"),
    aave_pool_data_provider:      address!("69FA688f1Dc47d4B5d8029D5a35FB7a548310654"),
    multicall3:                   address!("cA11bde05977b3631167028862bE2a173976CA11"),
    usdc:                         address!("3c499c542cEF5E3811e1192ce70d8cC03d5c3359"),
    weth:                         address!("7ceB23fD6bC0adD59E62ac25578270cFf1b9f619"),
    chainlink_eth_usd: Some(address!("F9680D99D6C9589e2a93a78A04A279e509205945")),
    balancer_vault:    Some(BALANCER_V2_VAULT),
    ws_hint_substring: "g.alchemy.com",
};

// ── BNB Chain ────────────────────────────────────────────────────────────────
pub const BNB_CHAIN: ChainSpec = ChainSpec {
    chain_id: 56,
    name:     "BNB Chain",
    status:   ChainStatus::Preview, // verify Aave addresses before going Live
    aave_pool:                    address!("6807dc923806fE8Fd134338EABCA509979a7e0cB"),
    aave_addresses_provider:      address!("ff75B6da14FfbbfD355Daf7a2731456b3562Ba6D"),
    aave_ui_pool_data_provider:   address!("83a26baf7Bd09Be78B8a25e10De7Df0bf41eB9c9"),
    aave_pool_data_provider:      address!("23dF2a19384231aFD114b036C14b6b03324D79BC"),
    multicall3:                   address!("cA11bde05977b3631167028862bE2a173976CA11"),
    usdc:                         address!("8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d"),
    weth:                         address!("2170Ed0880ac9A755fd29B2688956BD959F933F8"),
    chainlink_eth_usd: Some(address!("9ef1B8c0E4F7dc8bF5719Ea496883DC6401d5b2e")),
    balancer_vault: None, // Balancer V2 not on BNB Chain
    ws_hint_substring: "binance.org",
};

// ── Monad mainnet — FlashSwap arb only ───────────────────────────────────────
// Aave V3 and Balancer V2 are not yet deployed on Monad. The bot enables only
// the Uniswap V3 flash-swap arb path. Liquidation and Aave/Balancer-funded arb
// strategies are auto-disabled via lending_protocol_available() /
// balancer_available() returning false.
//
// ⚠ usdc / weth are placeholders — replace with verified bridged token
//   addresses from the Monad token registry before going live.
pub const MONAD: ChainSpec = ChainSpec {
    chain_id: 41454,
    name:     "Monad",
    status:   ChainStatus::Live,
    aave_pool:                    Address::ZERO,
    aave_addresses_provider:      Address::ZERO,
    aave_ui_pool_data_provider:   Address::ZERO,
    aave_pool_data_provider:      Address::ZERO,
    multicall3:                   address!("cA11bde05977b3631167028862bE2a173976CA11"),
    usdc:                         address!("0000000000000000000000000000000000000001"), // ⚠ placeholder
    weth:                         address!("0000000000000000000000000000000000000002"), // ⚠ placeholder
    chainlink_eth_usd: None, // no Chainlink feeds on Monad yet
    balancer_vault:    None, // Balancer V2 not on Monad
    ws_hint_substring: "",
};

pub fn for_chain_id(chain_id: u64) -> Option<&'static ChainSpec> {
    match chain_id {
        8453  => Some(&BASE_MAINNET),
        1     => Some(&ETHEREUM_MAINNET),
        42161 => Some(&ARBITRUM_ONE),
        10    => Some(&OPTIMISM),
        137   => Some(&POLYGON),
        56    => Some(&BNB_CHAIN),
        41454 => Some(&MONAD),
        _     => None,
    }
}

pub fn all_chains() -> &'static [&'static ChainSpec] {
    &[&BASE_MAINNET, &ETHEREUM_MAINNET, &ARBITRUM_ONE,
      &OPTIMISM, &POLYGON, &BNB_CHAIN, &MONAD]
}
