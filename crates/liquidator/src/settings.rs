//! Runtime configuration, read from environment variables so no secrets
//! ever live in source or in this repo.

use std::path::PathBuf;
use std::time::Duration;

use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};

pub struct Settings {
    /// HTTP(S) JSON-RPC endpoint for the target chain.
    pub rpc_url: String,
    /// Hex-encoded private key (with or without 0x prefix) for the wallet
    /// that owns the deployed AlphaRouting contract. Required to send txs.
    pub private_key: String,
    /// Address of the deployed AlphaRouting contract.
    pub contract_address: Address,
    /// Chain ID — selects the Aave addresses from the `config` crate.
    pub chain_id: u64,
    /// Address of Aave's AaveProtocolDataProvider for this chain (not part
    /// of `config::ChainSpec` yet — pass explicitly until it is).
    pub pool_data_provider: Address,
    /// Block number to start borrower discovery from if no cache exists.
    /// Use the Aave V3 pool's deployment block for a full history.
    pub discovery_start_block: u64,
    /// Where the borrower watchlist cache is persisted between runs.
    pub borrower_cache_path: PathBuf,
    /// Minimum profit (in the debt asset's smallest unit) required for the
    /// contract to accept a liquidation — see AlphaRouting's MinProfitNotMet.
    pub min_profit: U256,
    /// DEX used to sell seized collateral back into the debt asset.
    pub sell_dex: String,
    pub sell_fee_bps: u32,
    /// How often to re-poll known borrowers' health factors.
    pub poll_interval: Duration,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let rpc_url = require_env("RPC_URL")?;
        let private_key = require_env("PRIVATE_KEY")?;
        let contract_address = require_env("CONTRACT_ADDRESS")?
            .parse()
            .context("CONTRACT_ADDRESS is not a valid address")?;
        let chain_id = require_env("CHAIN_ID")?
            .parse()
            .context("CHAIN_ID must be a u64")?;
        let pool_data_provider = require_env("AAVE_POOL_DATA_PROVIDER")?
            .parse()
            .context("AAVE_POOL_DATA_PROVIDER is not a valid address")?;
        let discovery_start_block = std::env::var("DISCOVERY_START_BLOCK")
            .ok()
            .map(|s| s.parse())
            .transpose()
            .context("DISCOVERY_START_BLOCK must be a u64")?
            .unwrap_or(0);
        let borrower_cache_path = std::env::var("BORROWER_CACHE_PATH")
            .unwrap_or_else(|_| "borrowers.json".to_string())
            .into();
        let min_profit = std::env::var("MIN_PROFIT")
            .ok()
            .map(|s| s.parse::<U256>())
            .transpose()
            .context("MIN_PROFIT must be a u256")?
            .unwrap_or(U256::from(1u64));
        let sell_dex = std::env::var("SELL_DEX").unwrap_or_else(|_| "uniswap".to_string());
        let sell_fee_bps = std::env::var("SELL_FEE_BPS")
            .ok()
            .map(|s| s.parse())
            .transpose()
            .context("SELL_FEE_BPS must be a u32")?
            .unwrap_or(3000);
        let poll_interval = Duration::from_secs(
            std::env::var("POLL_INTERVAL_SECS")
                .ok()
                .map(|s| s.parse())
                .transpose()
                .context("POLL_INTERVAL_SECS must be a u64")?
                .unwrap_or(30),
        );

        Ok(Self {
            rpc_url,
            private_key,
            contract_address,
            chain_id,
            pool_data_provider,
            discovery_start_block,
            borrower_cache_path,
            min_profit,
            sell_dex,
            sell_fee_bps,
            poll_interval,
        })
    }
}

fn require_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}
