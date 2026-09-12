//! Liquidation bot: watches Aave V3 borrowers on the configured chain and
//! calls AlphaRouting.executeLiquidation on any position whose health factor
//! drops below 1.0.
//!
//! Required env vars: RPC_URL, PRIVATE_KEY, CONTRACT_ADDRESS, CHAIN_ID,
//! AAVE_POOL_DATA_PROVIDER. See settings.rs for the rest.

use alloy::network::EthereumWallet;
use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use tracing::{info, warn};

use liquidator::aave::{decode_user_configuration, IPool, IPoolDataProvider, HEALTH_FACTOR_LIQUIDATION_THRESHOLD};
use liquidator::discovery::{scan_new_borrowers, BorrowerRegistry};
use liquidator::router::{encode_swap_params, IAlphaRouting};
use liquidator::select::{select_liquidation_target, ReserveBalance};
use liquidator::settings::Settings;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let settings = Settings::from_env().context("loading settings")?;
    let chain_spec = config::chain_spec::for_chain_id(settings.chain_id)
        .with_context(|| format!("unsupported chain id {}", settings.chain_id))?;
    if !chain_spec.lending_protocol_available() {
        anyhow::bail!("Aave is not deployed on {} — nothing to liquidate", chain_spec.name);
    }

    let signer: PrivateKeySigner = settings.private_key.parse().context("parsing PRIVATE_KEY")?;
    let wallet = EthereumWallet::from(signer);
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .on_http(settings.rpc_url.parse().context("parsing RPC_URL")?);

    let pool = IPool::new(chain_spec.aave_pool, provider.clone());
    let data_provider = IPoolDataProvider::new(settings.pool_data_provider, provider.clone());
    let router = IAlphaRouting::new(settings.contract_address, provider.clone());

    let reserves_list = pool.getReservesList().call().await.context("getReservesList")?._0;
    info!(chain = chain_spec.name, reserves = reserves_list.len(), "connected");

    let mut registry = BorrowerRegistry::load_or_default(&settings.borrower_cache_path, settings.discovery_start_block)?;

    loop {
        match scan_new_borrowers(&provider, chain_spec.aave_pool, &mut registry).await {
            Ok(new_count) if new_count > 0 => {
                info!(new_count, total = registry.borrowers.len(), "discovered new borrowers");
                registry.save(&settings.borrower_cache_path)?;
            }
            Ok(_) => {}
            Err(err) => warn!(?err, "borrower discovery scan failed, continuing with known set"),
        }

        for borrower in registry.borrowers.clone() {
            if let Err(err) = check_and_liquidate(
                &pool,
                &data_provider,
                &router,
                &reserves_list,
                borrower,
                &settings,
            )
            .await
            {
                warn!(?borrower, ?err, "liquidation check failed");
            }
        }

        tokio::time::sleep(settings.poll_interval).await;
    }
}

async fn check_and_liquidate<P: Provider + Clone>(
    pool: &IPool::IPoolInstance<(), P>,
    data_provider: &IPoolDataProvider::IPoolDataProviderInstance<(), P>,
    router: &IAlphaRouting::IAlphaRoutingInstance<(), P>,
    reserves_list: &[Address],
    borrower: Address,
    settings: &Settings,
) -> Result<()> {
    let account_data = pool.getUserAccountData(borrower).call().await?;
    if account_data.totalDebtBase.is_zero() {
        return Ok(());
    }
    if account_data.healthFactor >= U256::from(HEALTH_FACTOR_LIQUIDATION_THRESHOLD) {
        return Ok(());
    }

    info!(?borrower, health_factor = %account_data.healthFactor, "liquidatable position found");

    let bitmap: u128 = pool
        .getUserConfiguration(borrower)
        .call()
        .await?
        .data
        .try_into()
        .unwrap_or(u128::MAX);
    let flags = decode_user_configuration(bitmap, reserves_list.len());

    let mut balances = Vec::with_capacity(reserves_list.len());
    for (reserve, flag) in reserves_list.iter().zip(flags.iter()) {
        if !flag.borrowing && !flag.collateral {
            continue;
        }
        let data = data_provider.getUserReserveData(*reserve, borrower).call().await?;
        balances.push(ReserveBalance {
            asset: *reserve,
            debt: data.currentStableDebt + data.currentVariableDebt,
            a_token: data.currentATokenBalance,
            is_collateral: flag.collateral,
            is_borrowed: flag.borrowing,
        });
    }

    let Some(target) = select_liquidation_target(&balances) else {
        warn!(?borrower, "liquidatable but no usable collateral/debt reserve pair found");
        return Ok(());
    };

    info!(
        ?borrower,
        debt_asset = %target.debt_asset,
        collateral_asset = %target.collateral_asset,
        debt_to_cover = %target.debt_to_cover,
        "submitting liquidation"
    );

    let swap_params = encode_swap_params(&settings.sell_dex, settings.sell_fee_bps, settings.min_profit);
    let pending = router
        .executeLiquidation(target.collateral_asset, target.debt_asset, borrower, target.debt_to_cover, swap_params)
        .send()
        .await?;
    let receipt = pending.get_receipt().await?;
    info!(?borrower, tx_hash = %receipt.transaction_hash, "liquidation submitted");

    Ok(())
}
