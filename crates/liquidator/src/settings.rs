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
    /// Sourced from `PRIVATE_KEY_FILE` in preference to `PRIVATE_KEY` — see
    /// `resolve_private_key`.
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
        let private_key = resolve_private_key(
            std::env::var("PRIVATE_KEY").ok(),
            std::env::var("PRIVATE_KEY_FILE").ok(),
        )?;
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

/// Resolves the signing key, preferring a file path over an inline value.
///
/// A key in an env var is visible to anything that can read the process's
/// `/proc/<pid>/environ`, and tends to end up in shell history and logs. A
/// file lets the key live in a root-owned `0600` file that systemd reads via
/// `LoadCredential`/`EnvironmentFile` while the service user never can.
/// Trailing newlines — which every text editor adds — are stripped, because
/// they otherwise produce an opaque key-parse failure at startup.
pub fn resolve_private_key(
    key_inline: Option<String>,
    key_file: Option<String>,
) -> Result<String> {
    if let Some(path) = key_file.filter(|p| !p.trim().is_empty()) {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading PRIVATE_KEY_FILE at {path}"))?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("PRIVATE_KEY_FILE at {path} is empty");
        }
        return Ok(trimmed.to_string());
    }

    match key_inline.filter(|k| !k.trim().is_empty()) {
        Some(key) => Ok(key.trim().to_string()),
        None => anyhow::bail!("set PRIVATE_KEY_FILE (preferred) or PRIVATE_KEY"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn key_file_with(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn inline_key_is_used_when_no_file_given() {
        let key = resolve_private_key(Some("0xabc123".into()), None).unwrap();
        assert_eq!(key, "0xabc123");
    }

    #[test]
    fn file_takes_precedence_over_inline_key() {
        let f = key_file_with("0xfromfile");
        let key = resolve_private_key(
            Some("0xinline".into()),
            Some(f.path().to_string_lossy().into_owned()),
        )
        .unwrap();
        assert_eq!(key, "0xfromfile", "the file is the more secure source");
    }

    #[test]
    fn trailing_newline_in_key_file_is_stripped() {
        // Every editor adds this; without trimming it becomes an opaque
        // key-parse error at startup.
        let f = key_file_with("0xdeadbeef\n");
        let key =
            resolve_private_key(None, Some(f.path().to_string_lossy().into_owned())).unwrap();
        assert_eq!(key, "0xdeadbeef");
    }

    #[test]
    fn surrounding_whitespace_in_inline_key_is_stripped() {
        let key = resolve_private_key(Some("  0xabc  ".into()), None).unwrap();
        assert_eq!(key, "0xabc");
    }

    #[test]
    fn empty_key_file_is_an_error_not_an_empty_key() {
        let f = key_file_with("   \n");
        let err = resolve_private_key(None, Some(f.path().to_string_lossy().into_owned()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("is empty"), "got: {err}");
    }

    #[test]
    fn missing_key_file_reports_the_path() {
        let err = resolve_private_key(None, Some("/nonexistent/key.txt".into()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("/nonexistent/key.txt"), "got: {err}");
    }

    #[test]
    fn no_key_source_at_all_is_an_error() {
        assert!(resolve_private_key(None, None).is_err());
    }

    #[test]
    fn blank_inline_key_is_treated_as_absent() {
        assert!(resolve_private_key(Some("   ".into()), None).is_err());
    }

    #[test]
    fn blank_file_path_falls_back_to_inline_key() {
        let key = resolve_private_key(Some("0xabc".into()), Some("".into())).unwrap();
        assert_eq!(key, "0xabc");
    }
}
