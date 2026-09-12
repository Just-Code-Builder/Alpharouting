//! Token launch radar: watches a DEX factory for new pools, then profiles
//! each new token — liquidity, holder distribution, trade activity, LP
//! status — and prints a risk-scored alert.
//!
//! Read-only: it never signs or sends a transaction, so it needs no private
//! key. Required env vars: RPC_URL, DEX_FACTORY, QUOTE_TOKEN. See
//! settings.rs and README.md.

use std::collections::HashMap;

use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::Filter;
use alloy::sol_types::SolEvent;
use anyhow::{Context, Result};
use tracing::{info, warn};

use radar::activity::{classify_v2, classify_v3, Activity, Side};
use radar::dex::{DexKind, IERC20Meta, IUniswapV2Factory, IUniswapV2Pair, IUniswapV3Factory, IUniswapV3Pool};
use radar::holders::{self, BURN_ADDRESSES};
use radar::report::{render, LaunchAlert};
use radar::risk::{assess, Signals};
use radar::scan::{apply_transfers, lp_is_burned, quote_liquidity_usd, SeenPools};
use radar::settings::Settings;

/// A pool creation picked up from the factory.
struct NewPool {
    pool: Address,
    token0: Address,
    token1: Address,
    created_block: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let settings = Settings::from_env().context("loading settings")?;
    let provider = ProviderBuilder::new().on_http(settings.rpc_url.parse().context("parsing RPC_URL")?);

    let chain_id = provider.get_chain_id().await.context("fetching chain id")?;
    info!(chain_id, factory = %settings.dex_factory, "radar starting");

    let mut seen = SeenPools::load_or_default(&settings.seen_cache_path, settings.start_block)?;

    loop {
        if let Err(err) = tick(&provider, &settings, &mut seen).await {
            warn!(?err, "scan tick failed; retrying next interval");
        }
        tokio::time::sleep(settings.poll_interval).await;
    }
}

async fn tick<P: Provider>(provider: &P, settings: &Settings, seen: &mut SeenPools) -> Result<()> {
    let head = provider.get_block_number().await.context("fetching head block")?;
    if seen.last_scanned_block >= head {
        return Ok(());
    }

    let mut from = seen.last_scanned_block + 1;
    while from <= head {
        let to = (from + settings.block_chunk - 1).min(head);

        for new_pool in fetch_new_pools(provider, settings, from, to).await? {
            if !seen.mark_seen(new_pool.pool) {
                continue;
            }
            match profile(provider, settings, &new_pool).await {
                Ok(alert) => println!("{}", render(&alert)),
                Err(err) => warn!(pool = %new_pool.pool, ?err, "could not profile new pool"),
            }
        }

        seen.last_scanned_block = to;
        seen.save(&settings.seen_cache_path)?;
        from = to + 1;
    }

    Ok(())
}

async fn fetch_new_pools<P: Provider>(
    provider: &P,
    settings: &Settings,
    from: u64,
    to: u64,
) -> Result<Vec<NewPool>> {
    let topic0 = match settings.dex_kind {
        DexKind::UniswapV2 => IUniswapV2Factory::PairCreated::SIGNATURE_HASH,
        DexKind::UniswapV3 => IUniswapV3Factory::PoolCreated::SIGNATURE_HASH,
    };

    let filter = Filter::new()
        .address(settings.dex_factory)
        .event_signature(topic0)
        .from_block(from)
        .to_block(to);

    let logs = provider
        .get_logs(&filter)
        .await
        .with_context(|| format!("get_logs [{from}, {to}]"))?;

    let mut pools = Vec::new();
    for log in logs {
        let created_block = log.block_number.unwrap_or(to);
        match settings.dex_kind {
            DexKind::UniswapV2 => {
                if let Ok(decoded) = log.log_decode::<IUniswapV2Factory::PairCreated>() {
                    let ev = decoded.inner.data;
                    pools.push(NewPool {
                        pool: ev.pair,
                        token0: ev.token0,
                        token1: ev.token1,
                        created_block,
                    });
                }
            }
            DexKind::UniswapV3 => {
                if let Ok(decoded) = log.log_decode::<IUniswapV3Factory::PoolCreated>() {
                    let ev = decoded.inner.data;
                    pools.push(NewPool {
                        pool: ev.pool,
                        token0: ev.token0,
                        token1: ev.token1,
                        created_block,
                    });
                }
            }
        }
    }

    Ok(pools)
}

/// Builds the signal set for a newly created pool. Every individual lookup
/// that fails leaves its signal as `None` rather than failing the whole
/// profile — a partial report marked "could not verify" beats no report.
async fn profile<P: Provider>(provider: &P, settings: &Settings, new_pool: &NewPool) -> Result<LaunchAlert> {
    // The launched token is whichever side isn't the quote asset. If neither
    // is, we can't value liquidity and say so rather than guessing.
    let (token, quote_is_token0) = if new_pool.token1 == settings.quote_token {
        (new_pool.token0, false)
    } else if new_pool.token0 == settings.quote_token {
        (new_pool.token1, true)
    } else {
        (new_pool.token0, false)
    };
    let paired_against_quote =
        new_pool.token0 == settings.quote_token || new_pool.token1 == settings.quote_token;
    let token_is_token0 = token == new_pool.token0;

    let head = provider.get_block_number().await.unwrap_or(new_pool.created_block);
    let window_end = (new_pool.created_block + settings.activity_window_blocks).min(head);

    let symbol = IERC20Meta::new(token, provider)
        .symbol()
        .call()
        .await
        .ok()
        .map(|s| s._0);

    let liquidity_usd = if paired_against_quote {
        read_quote_reserve(provider, settings, new_pool, quote_is_token0)
            .await
            .and_then(|reserve| {
                quote_liquidity_usd(reserve, settings.quote_decimals, settings.quote_usd_price)
            })
    } else {
        None
    };

    let (holder_stats, creator) =
        read_holders(provider, token, new_pool, settings, window_end).await;

    let activity = read_activity(provider, settings, new_pool, token_is_token0, window_end).await;

    let lp_burned_or_locked = match settings.dex_kind {
        DexKind::UniswapV2 => read_lp_status(provider, new_pool.pool).await,
        // V3 liquidity is held as position NFTs, so the V2 burn check doesn't
        // apply; reporting it as unknown is the honest answer here.
        DexKind::UniswapV3 => None,
    };

    let signals = Signals {
        liquidity_usd,
        holder_count: holder_stats.as_ref().map(|h| h.holder_count),
        top_holder_share: holder_stats.as_ref().map(|h| h.top_holder_share),
        top10_share: holder_stats.as_ref().map(|h| h.top10_share),
        creator_share: holder_stats.as_ref().map(|h| h.creator_share),
        lp_burned_or_locked,
        buys: activity.map(|a| a.buys),
        sells: activity.map(|a| a.sells),
    };

    let _ = creator;
    let assessment = assess(&signals);

    Ok(LaunchAlert { token, symbol, pool: new_pool.pool, signals, assessment })
}

async fn read_quote_reserve<P: Provider>(
    provider: &P,
    settings: &Settings,
    new_pool: &NewPool,
    quote_is_token0: bool,
) -> Option<U256> {
    match settings.dex_kind {
        DexKind::UniswapV2 => {
            let reserves = IUniswapV2Pair::new(new_pool.pool, provider)
                .getReserves()
                .call()
                .await
                .ok()?;
            let quote = if quote_is_token0 { reserves.reserve0 } else { reserves.reserve1 };
            Some(U256::from(quote))
        }
        DexKind::UniswapV3 => {
            // Concentrated liquidity isn't a simple reserve read; use the
            // pool's quote-token balance as the tradable depth proxy.
            let balance = IERC20Meta::new(settings.quote_token, provider)
                .balanceOf(new_pool.pool)
                .call()
                .await
                .ok()?;
            Some(balance._0)
        }
    }
}

async fn read_holders<P: Provider>(
    provider: &P,
    token: Address,
    new_pool: &NewPool,
    settings: &Settings,
    window_end: u64,
) -> (Option<holders::HolderStats>, Option<Address>) {
    let filter = Filter::new()
        .address(token)
        .event_signature(IERC20Meta::Transfer::SIGNATURE_HASH)
        .from_block(new_pool.created_block.saturating_sub(settings.activity_window_blocks))
        .to_block(window_end);

    let Ok(logs) = provider.get_logs(&filter).await else {
        return (None, None);
    };

    let mut transfers = Vec::with_capacity(logs.len());
    let mut creator = None;
    for log in &logs {
        if let Ok(decoded) = log.log_decode::<IERC20Meta::Transfer>() {
            let ev = decoded.inner.data;
            // Heuristic: the first mint's recipient is usually the deployer.
            if creator.is_none() && ev.from == Address::ZERO {
                creator = Some(ev.to);
            }
            transfers.push((ev.from, ev.to, ev.value));
        }
    }

    let mut balances: HashMap<Address, U256> = HashMap::new();
    apply_transfers(&mut balances, &transfers);

    (holders::compute(&balances, Some(new_pool.pool), creator), creator)
}

async fn read_activity<P: Provider>(
    provider: &P,
    settings: &Settings,
    new_pool: &NewPool,
    token_is_token0: bool,
    window_end: u64,
) -> Option<Activity> {
    let topic0 = match settings.dex_kind {
        DexKind::UniswapV2 => IUniswapV2Pair::Swap::SIGNATURE_HASH,
        DexKind::UniswapV3 => IUniswapV3Pool::Swap::SIGNATURE_HASH,
    };

    let filter = Filter::new()
        .address(new_pool.pool)
        .event_signature(topic0)
        .from_block(new_pool.created_block)
        .to_block(window_end);

    let logs = provider.get_logs(&filter).await.ok()?;

    let mut sides: Vec<Side> = Vec::with_capacity(logs.len());
    for log in &logs {
        let side = match settings.dex_kind {
            DexKind::UniswapV2 => log
                .log_decode::<IUniswapV2Pair::Swap>()
                .ok()
                .and_then(|d| {
                    let ev = d.inner.data;
                    classify_v2(token_is_token0, ev.amount0In, ev.amount1In, ev.amount0Out, ev.amount1Out)
                }),
            DexKind::UniswapV3 => log
                .log_decode::<IUniswapV3Pool::Swap>()
                .ok()
                .and_then(|d| {
                    let ev = d.inner.data;
                    classify_v3(token_is_token0, ev.amount0, ev.amount1)
                }),
        };
        if let Some(side) = side {
            sides.push(side);
        }
    }

    Some(Activity::tally(sides))
}

async fn read_lp_status<P: Provider>(provider: &P, pool: Address) -> Option<bool> {
    let pair = IUniswapV2Pair::new(pool, provider);
    let total = pair.totalSupply().call().await.ok()?._0;

    let mut burned = U256::ZERO;
    for burn_addr in BURN_ADDRESSES {
        if let Ok(bal) = pair.balanceOf(burn_addr).call().await {
            burned = burned.saturating_add(bal._0);
        }
    }

    lp_is_burned(total, burned)
}
