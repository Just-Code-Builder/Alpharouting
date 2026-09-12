# liquidator

Watches Aave V3 borrowers on a configured chain and calls
`AlphaRouting.executeLiquidation` on any position whose health factor drops
below 1.0. Pairs with the liquidation strategy implemented in
`src/AlphaRouting.sol`.

## How it works

1. **Discovery** (`discovery.rs`) — Aave doesn't expose "list all borrowers",
   so the bot scans historical `Borrow` event logs from the Aave Pool
   contract to build a watchlist. It caches progress to
   `BORROWER_CACHE_PATH` so a restart resumes instead of rescanning from
   genesis.
2. **Scanning** (`main.rs`, `aave.rs`) — for each known borrower, calls
   `getUserAccountData` for its health factor. Below 1.0 and with nonzero
   debt, it's a liquidation candidate.
3. **Asset selection** (`select.rs`) — decodes the borrower's per-reserve
   bitmap (`getUserConfiguration`), pulls balances for every flagged reserve
   (`getUserReserveData`), and picks the largest debt reserve to repay and
   the largest collateral reserve to seize, capped at Aave's 50% close
   factor.
4. **Execution** (`router.rs`) — calls `executeLiquidation` on the deployed
   `AlphaRouting` contract, which flashloans the repay amount, seizes
   collateral, sells it, and reverts the whole transaction if the resulting
   profit doesn't clear `MIN_PROFIT`.

## Running it

Requires a chain where Aave V3 is deployed (Base mainnet — Monad has no Aave
deployment, so this bot can't run there). You need your own RPC endpoint and
a funded wallet that owns the deployed `AlphaRouting` contract.

```bash
export RPC_URL=https://your-base-rpc                    # your own endpoint
export PRIVATE_KEY=0x...                                  # wallet that owns AlphaRouting
export CONTRACT_ADDRESS=0x...                              # deployed AlphaRouting address
export CHAIN_ID=8453                                       # Base mainnet
export AAVE_POOL_DATA_PROVIDER=0xd82a47fdebB5bf5329b09441C3DaB4b5df2153Ad  # Base AaveProtocolDataProvider
export DISCOVERY_START_BLOCK=0                              # Aave V3 Pool's deployment block on Base, ideally
export MIN_PROFIT=1000000                                   # in the debt asset's smallest unit (e.g. 1 USDC = 1e6)

cargo run --release -p liquidator
```

See `src/settings.rs` for the full list of env vars and their defaults.

## What this doesn't do (yet)

- No gas-price-aware profitability check before submitting — the contract's
  own `MinProfitNotMet` revert is the only backstop, so unprofitable attempts
  still cost gas.
- No mempool/MEV protection (private RPC, Flashbots-style bundle submission)
  — a liquidation broadcast to a public mempool can be front-run.
- Discovery is a simple full-history event scan; a large chain history means
  a slow first run. A subgraph or indexer would be faster for production use.
- No test coverage against a live or forked chain — only pure-logic unit
  tests (bitmap decoding, asset selection, ABI encoding, registry
  persistence). Verify against a Base Sepolia deployment before running with
  real funds.
