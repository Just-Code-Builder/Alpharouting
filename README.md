# AlphaRouting (AR)

> **On-chain MEV execution contract — Base mainnet.**
> Built and operated by [@web4coder](https://x.com/web4coder)

---

## What is this?

**AlphaRouting** is a production MEV bot running on Base mainnet.
It executes flashloan-funded arbitrage and liquidations atomically — you either profit or the whole transaction reverts. No principal at risk, ever.

This repo is a **public interface snippet** of the on-chain contract.
The full execution engine (Rust, 16 crates, ~8 000 lines) is private.

---

## Strategies

| # | Strategy | Flashloan source | Description |
|---|---|---|---|
| 1 | **Triangular Arb** | Aave V3 | A→B→C→A across 2–3 DEXes in one tx |
| 2 | **Flash-Swap Arb** | Uniswap V3 pool | Borrows directly from a V3 pool; pool fee in lieu of Aave 9 bps |
| 3 | **Balancer Arb** | Balancer V2 Vault | Same hop structure; 0-fee borrow from Balancer |
| 4 | **Liquidation** | Aave V3 | Seize under-collateralised position + sell collateral atomically |
| 5 | **Liquidation Combo** | Aave V3 | Liquidation + immediate cross-DEX arb on the price gap it opens |
| 6 | **Batch Liquidation** | Aave V3 | Multiple borrowers in a single flashloan call |
| 7 | **Rebase Arb** | Aave V3 | Scheduled at rebase windows for rebasing tokens |

All strategies share a **`minProfit` revert guard** — if the trade isn't profitable by at least the configured floor, the whole transaction reverts.

---

## DEX support

- Uniswap V3
- SushiSwap V3
- BaseSwap V3
- PancakeSwap V3
- Aerodrome (stable + volatile pools)

---

## Architecture (snippet view)

```
AlphaRouting.sol
├── executeArbitrage()         ← owner-only entry, sets transient lock + strategy
├── executeFlashSwapArb()      ← borrows from Uni V3 pool directly
├── executeBalancerArb()       ← 0-fee Balancer V2 vault borrow
├── executeLiquidation()       ← single liquidation
├── executeLiquidationCombo()  ← liquidation + same-tx arb
├── executeBatchLiquidations() ← up to N borrowers, one flashloan
│
├── executeOperation()         ← Aave V3 callback (validates initiator + strategy slot)
├── uniswapV3FlashCallback()   ← Uni V3 flash callback (validates pool from transient)
└── receiveFlashLoan()         ← Balancer V2 callback
```

Security properties:
- **Transient-storage locks** (EIP-1153) — re-entrancy impossible even within the same tx
- **Strategy slots** — callback dispatch authenticated via trusted transient storage, never caller-supplied data
- **Delta-based profit** — pre-flashloan inventory snapshot prevents losing trades from silently consuming balance
- **Owner-only execution** — all entry points gated by `onlyOwner`

---

## Deployed contract

| Network | Address |
|---|---|
| Base mainnet | *(available on request)* |
| Base Sepolia | *(testnet — ask)* |

---

## Full source

This snippet shows structs, events, errors, and function signatures.
**Implementation bodies, deploy scripts, and the full Rust execution engine are private.**

If you want access to the full codebase (for learning, collaboration, or licensing):

> **DM me on X → [@web4coder](https://x.com/web4coder)**
> or email **theblockchaincoder@gmail.com**

---

## Tech stack

| Layer | Tech |
|---|---|
| On-chain | Solidity 0.8.26, Foundry, OpenZeppelin 5 |
| Off-chain bot | Rust (async, tokio), 16 crates |
| Persistence | SQLite |
| Infra | Railway / VPS, Flashbots Protect RPC |
| Alerts | Telegram inline-keyboard UI |

---

*Built on Base. All flashloans are atomic — you only ever pay gas.*
