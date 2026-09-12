//! Runtime configuration from environment variables.
//!
//! Deliberately chain-agnostic: point `RPC_URL` and `DEX_FACTORY` at any EVM
//! chain and the radar works there. No chain addresses are hardcoded, so
//! nothing here can silently target the wrong contract on a chain whose
//! deployments haven't been verified.

use std::path::PathBuf;
use std::time::Duration;

use alloy::primitives::Address;
use anyhow::{Context, Result};

use crate::dex::DexKind;

pub struct Settings {
    pub rpc_url: String,
    /// DEX factory to watch for new pools/pairs.
    pub dex_factory: Address,
    pub dex_kind: DexKind,
    /// Quote token (WETH/USDC/etc) used to size liquidity. A new token paired
    /// against something else entirely is reported with unknown liquidity
    /// rather than a fabricated number.
    pub quote_token: Address,
    /// USD price of one whole quote token. Without it, liquidity is reported
    /// as unknown rather than guessed.
    pub quote_usd_price: Option<f64>,
    pub quote_decimals: u8,
    /// Block to begin scanning from if there's no saved progress.
    pub start_block: u64,
    pub seen_cache_path: PathBuf,
    pub poll_interval: Duration,
    /// Blocks per getLogs request.
    pub block_chunk: u64,
    /// How many blocks after a pool's creation to sample swap activity over.
    pub activity_window_blocks: u64,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let rpc_url = require("RPC_URL")?;
        let dex_factory = require("DEX_FACTORY")?
            .parse()
            .context("DEX_FACTORY is not a valid address")?;
        let dex_kind_raw = std::env::var("DEX_KIND").unwrap_or_else(|_| "v2".to_string());
        let dex_kind = DexKind::parse(&dex_kind_raw)
            .with_context(|| format!("DEX_KIND '{dex_kind_raw}' is not one of: v2, v3"))?;
        let quote_token = require("QUOTE_TOKEN")?
            .parse()
            .context("QUOTE_TOKEN is not a valid address")?;
        let quote_usd_price = std::env::var("QUOTE_USD_PRICE")
            .ok()
            .map(|s| s.parse())
            .transpose()
            .context("QUOTE_USD_PRICE must be a number")?;
        let quote_decimals = std::env::var("QUOTE_DECIMALS")
            .ok()
            .map(|s| s.parse())
            .transpose()
            .context("QUOTE_DECIMALS must be a u8")?
            .unwrap_or(18);
        let start_block = std::env::var("START_BLOCK")
            .ok()
            .map(|s| s.parse())
            .transpose()
            .context("START_BLOCK must be a u64")?
            .unwrap_or(0);
        let seen_cache_path = std::env::var("SEEN_CACHE_PATH")
            .unwrap_or_else(|_| "seen_pools.json".to_string())
            .into();
        let poll_interval = Duration::from_secs(
            std::env::var("POLL_INTERVAL_SECS")
                .ok()
                .map(|s| s.parse())
                .transpose()
                .context("POLL_INTERVAL_SECS must be a u64")?
                .unwrap_or(15),
        );
        let block_chunk = std::env::var("BLOCK_CHUNK")
            .ok()
            .map(|s| s.parse())
            .transpose()
            .context("BLOCK_CHUNK must be a u64")?
            .unwrap_or(2_000);
        let activity_window_blocks = std::env::var("ACTIVITY_WINDOW_BLOCKS")
            .ok()
            .map(|s| s.parse())
            .transpose()
            .context("ACTIVITY_WINDOW_BLOCKS must be a u64")?
            .unwrap_or(5_000);

        Ok(Self {
            rpc_url,
            dex_factory,
            dex_kind,
            quote_token,
            quote_usd_price,
            quote_decimals,
            start_block,
            seen_cache_path,
            poll_interval,
            block_chunk,
            activity_window_blocks,
        })
    }
}

fn require(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}
