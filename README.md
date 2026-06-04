<div align="center">

<img src="https://capsule-render.vercel.app/api?type=waving&color=gradient&customColorList=6,11,20&height=200&section=header&text=AlphaRouting&fontSize=80&fontColor=fff&animation=twinkling&fontAlignY=35&desc=On-chain%20MEV%20Execution%20Engine&descAlignY=60&descSize=22" width="100%"/>

<br/>

[![Base](https://img.shields.io/badge/Base-Mainnet-0052FF?style=for-the-badge&logo=coinbase&logoColor=white)](https://base.org)
[![Monad](https://img.shields.io/badge/Monad-Mainnet-836EF9?style=for-the-badge&logo=ethereum&logoColor=white)](https://monad.xyz)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.26-363636?style=for-the-badge&logo=solidity&logoColor=white)](https://soliditylang.org)
[![Foundry](https://img.shields.io/badge/Foundry-Framework-FF6B35?style=for-the-badge&logo=rust&logoColor=white)](https://getfoundry.sh)
[![License](https://img.shields.io/badge/License-MIT-22c55e?style=for-the-badge&logoColor=white)](LICENSE)

<br/>

> **Multi-chain MEV execution contract — flashloan arbitrage, liquidations, and flash-swap strategies.**
> Deployed on **Base** and **Monad** mainnet.

<br/>

[![X Follow](https://img.shields.io/badge/Follow-%40web4coder-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/web4coder)
[![Contact](https://img.shields.io/badge/Email-theblockchaincoder%40gmail.com-EA4335?style=for-the-badge&logo=gmail&logoColor=white)](mailto:theblockchaincoder@gmail.com)

</div>

---

<div align="center">

## ⚡ What is AlphaRouting?

</div>

**AlphaRouting** is a production MEV bot running on **Base mainnet** and **Monad mainnet**.

It executes flashloan-funded arbitrage and liquidations **atomically** — the entire flashloan, swap sequence, and repayment happen in one transaction. You either profit or the whole transaction reverts. **No principal at risk, ever.**

This repository is a **public interface snippet** of the on-chain contract.
The full execution engine — Rust, 16 crates, ~8 000 lines — is **private**.

> 💬 Want the full source? **DM [@web4coder](https://x.com/web4coder)** on X or email [theblockchaincoder@gmail.com](mailto:theblockchaincoder@gmail.com)

---

<div align="center">

## 🌐 Supported Chains

</div>

<div align="center">

| Chain | Status | Flashloan Source | Notes |
|:---:|:---:|:---:|:---|
| <img src="https://img.shields.io/badge/Base-0052FF?style=flat-square&logo=coinbase&logoColor=white"/> | 🟢 **Live** | Aave V3 + Balancer V2 + Uni V3 | Full strategy suite — all 7 strategies active |
| <img src="https://img.shields.io/badge/Monad-836EF9?style=flat-square&logo=ethereum&logoColor=white"/> | 🟢 **Live** | Balancer V2 + Uni V3 flash-swap | Aave not yet on Monad — Balancer + FlashSwap arb active |

</div>

> **Same contract bytecode deploys to both chains.** Router addresses are constructor-injected — `address(0)` for any protocol not deployed on that chain gracefully disables those strategy paths.

---

<div align="center">

## 🎯 Strategies

</div>

<div align="center">

| # | Strategy | ⛽ Flashloan Source | 📋 Description |
|:---:|:---|:---:|:---|
| `1` | ⚡ **Triangular Arb** | Aave V3 | A→B→C→A across 2–3 DEXes in one atomic tx |
| `2` | 🔄 **Flash-Swap Arb** | Uniswap V3 Pool | Borrows directly from a V3 pool; pays pool fee instead of Aave's 9 bps |
| `3` | 🏦 **Balancer Arb** | Balancer V2 Vault | Same A→B→C→A hop structure; **0-fee** borrow from Balancer |
| `4` | 🎯 **Liquidation** | Aave V3 | Atomically seize an under-collateralised position + sell collateral |
| `5` | 💥 **Liquidation Combo** | Aave V3 | Liquidation + immediate cross-DEX arb on the price gap it opens |
| `6` | 📦 **Batch Liquidation** | Aave V3 | Multiple borrowers in a **single** flashloan — one tx, N liquidations |
| `7` | 🔁 **Rebase Arb** | Aave V3 | Fires at scheduled rebase windows for rebasing tokens |

</div>

All strategies share a **`minProfit` revert guard** — if the trade doesn't clear the configured profit floor, the entire transaction reverts. Gas is the only cost you can ever incur.

---

<div align="center">

## 🏗️ Architecture

</div>

```
                        ┌─────────────────────────────────┐
                        │        AlphaRouting.sol          │
                        │   Ownable  ·  ReentrancyGuard   │
                        └──────────────┬──────────────────┘
                                       │
             ┌─────────────────────────┼──────────────────────┐
             │                         │                      │
    ┌────────▼────────┐    ┌──────────▼──────────┐  ┌───────▼────────┐
    │   Aave V3 Path  │    │  Uniswap V3 Path     │  │  Balancer Path │
    │  flashLoanSimple│    │  pool.flash()        │  │  vault.flashL  │
    └────────┬────────┘    └──────────┬──────────┘  └───────┬────────┘
             │                        │                      │
    ┌────────▼────────┐    ┌──────────▼──────────┐  ┌───────▼────────┐
    │executeOperation │    │uniswapV3FlashCallback│  │receiveFlashLoan│
    │  (Aave cb)      │    │   (Uni V3 cb)        │  │ (Balancer cb)  │
    └────────┬────────┘    └──────────┬──────────┘  └───────┬────────┘
             │                        │                      │
             └─────────────────────────┴──────────────────────┘
                                       │
                              ┌────────▼────────┐
                              │   _swap()        │
                              │  DEX dispatcher  │
                              └────────┬────────┘
                                       │
        ┌──────────┬──────────┬────────┴──────────┬──────────┐
        ▼          ▼          ▼                    ▼          ▼
   Uniswap V3  SushiSwap  BaseSwap V3        PancakeSwap  Aerodrome
```

---

<div align="center">

## 🔐 Security Properties

</div>

<div align="center">

| Property | Implementation |
|:---|:---|
| 🔒 **Re-entrancy proof** | EIP-1153 transient-storage locks — impossible to re-enter even within the same tx |
| 🛡️ **Callback auth** | Strategy slots in transient storage — callbacks authenticated from **trusted state**, never caller-supplied data |
| 📊 **Delta-based profit** | Pre-flashloan inventory snapshot — losing trades that consume balance fail loudly, never silently |
| 👑 **Owner-only execution** | All entry points gated by `onlyOwner` — only the deployer wallet can trigger trades |
| 🔄 **Health-factor guard** | Double-checked inside the Aave callback — prevents wasted gas on already-liquidated positions |

</div>

---

<div align="center">

## 🔌 DEX Support

</div>

<div align="center">

| DEX | Protocol | Base | Monad |
|:---:|:---:|:---:|:---:|
| 🦄 **Uniswap V3** | Concentrated liquidity | ✅ | ✅ |
| 🍣 **SushiSwap V3** | Concentrated liquidity | ✅ | ✅ |
| 🔵 **BaseSwap V3** | Base-native fork | ✅ | ➖ |
| 🥞 **PancakeSwap V3** | Concentrated liquidity | ✅ | ✅ |
| 🌊 **Aerodrome** | Stable + volatile AMM | ✅ | ➖ |

</div>

---

<div align="center">

## 📄 Contract Interface (Snippet)

</div>

```solidity
// Entry points — owner only
function executeArbitrage(address borrowToken, uint256 borrowAmount, bytes calldata params) external;
function executeFlashSwapArb(address pool, address tokenBorrow, uint256 amount, bytes calldata params) external;
function executeBalancerArb(address borrowToken, uint256 borrowAmount, bytes calldata params) external;
function executeLiquidation(address collateral, address debt, address borrower, uint256 amount, bytes calldata params) external;
function executeLiquidationCombo(address collateral, address debt, address borrower, uint256 amount, bytes calldata params) external;
function executeBatchLiquidations(BatchLiqParams calldata p) external;
function executeRebaseArb(address borrowToken, uint256 borrowAmount, bytes calldata params) external;

// Flashloan callbacks — authenticated via transient storage
function executeOperation(address asset, uint256 amount, uint256 premium, address initiator, bytes calldata params) external returns (bool);
function uniswapV3FlashCallback(uint256 fee0, uint256 fee1, bytes calldata data) external;
function receiveFlashLoan(address[] calldata tokens, uint256[] calldata amounts, uint256[] calldata feeAmounts, bytes calldata userData) external;
```

> Full implementation bodies are private. [**Request access →**](mailto:theblockchaincoder@gmail.com)

---

<div align="center">

## 🛠️ Tech Stack

</div>

<div align="center">

![Rust](https://img.shields.io/badge/Rust-Bot_Engine-000000?style=for-the-badge&logo=rust&logoColor=white)
![Solidity](https://img.shields.io/badge/Solidity-0.8.26-363636?style=for-the-badge&logo=solidity&logoColor=white)
![Foundry](https://img.shields.io/badge/Foundry-Testing-FF6B35?style=for-the-badge&logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-Persistence-003B57?style=for-the-badge&logo=sqlite&logoColor=white)
![Telegram](https://img.shields.io/badge/Telegram-Alerts_UI-26A5E4?style=for-the-badge&logo=telegram&logoColor=white)
![Railway](https://img.shields.io/badge/Railway-Infra-0B0D0E?style=for-the-badge&logo=railway&logoColor=white)

</div>

| Layer | Details |
|:---|:---|
| **On-chain** | Solidity 0.8.26 · Foundry · OpenZeppelin 5 · EIP-1153 transient storage |
| **Off-chain bot** | Rust (async/tokio) · 16 crates · ~8 000 lines |
| **Persistence** | SQLite — trades, borrowers, routes, RPC keys |
| **Infra** | Railway / VPS · Flashbots Protect / MEV Blocker private RPC |
| **Alerts & UI** | Telegram inline-keyboard — `/trades`, `/diag`, `/rpc`, daily digest |

---

<div align="center">

## 🗺️ Roadmap

</div>

<div align="center">

| Phase | Status | Chain | What |
|:---:|:---:|:---:|:---|
| **1** | 🟢 **Live** | Base | Aave V3 liquidations + triangular arb + flashswap + Balancer |
| **2** | 🟢 **Live** | Monad | Balancer + FlashSwap arb (Aave not yet on Monad) |
| **3** | 🟡 Planned | Ethereum | Same contract re-deployed; Aave V3 ETH addresses |
| **4** | 🟡 Planned | BNB Chain | Venus protocol + PancakeSwap V3 |
| **5** | 🔵 Research | Solana | Rust SDK rewrite — Solend + Raydium/Orca/Meteora |
| **6** | 🔵 Design | All chains | **$AR token** — revenue-share bridge token, holders earn % of bot profits |

</div>

---

<div align="center">

## 📬 Get the Full Source

</div>

<div align="center">

This repo shows the **contract interface only** — structs, events, errors, and function signatures.

**Implementation bodies + the full Rust execution engine are private.**

<br/>

| Contact | Link |
|:---:|:---|
| 🐦 **X (Twitter)** | [@web4coder](https://x.com/web4coder) |
| 📧 **Email** | [theblockchaincoder@gmail.com](mailto:theblockchaincoder@gmail.com) |

<br/>

*Built atomic. Running live. Profits only.*

<br/>

<img src="https://capsule-render.vercel.app/api?type=waving&color=gradient&customColorList=6,11,20&height=100&section=footer" width="100%"/>

</div>
