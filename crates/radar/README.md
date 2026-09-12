# radar

Token launch radar. Watches a DEX factory for new pools, then profiles each
new token — liquidity, holder distribution, trade activity, LP status — and
emits a risk-scored alert.

**Read-only.** It never signs or sends a transaction, so it needs no private
key. Point it at an RPC endpoint and a factory address and it runs.

```
🚨 New token: DEMO
Address: 0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512
Liquidity: $60k
Holders: 6
Buys: 3
Sells: 1
Contract risk: High (65/100)
Flags:
  - moderate liquidity ($60000)
  - only 6 holders
  - top holder has 30% of supply
  - deployer still holds 22%
Pool: 0x75537828f2ce51be7289709686A69CbFDbB714F1
```

## Which chain does this run on?

Any EVM chain. Nothing is hardcoded — `RPC_URL`, `DEX_FACTORY`, and
`QUOTE_TOKEN` come from the environment, so the same binary works on Base,
Arbitrum, or Robinhood Chain once you have those three values.

**Robinhood Chain is not preconfigured.** No chain ID, factory, or token
addresses for it exist in this repo, because none have been verified. A
config full of guessed addresses is worse than no config: it either fails
silently or, worse, watches the wrong contract and reports confident
nonsense. To run there, supply an RPC endpoint and the address of whichever
DEX factory is actually deployed on it.

## Signals and how they're scored

| Signal | Source | Weight |
|:---|:---|:---|
| Liquidity (USD) | pool reserves × quote price × 2 | up to +30 |
| Holder count | reconstructed from `Transfer` history | up to +20 |
| Top holder share | same, pool and burn addresses excluded | up to +30 |
| Top-10 share | same, only scored above 10 holders | +15 |
| Deployer share | first mint recipient's current balance | +20 |
| LP burned/locked | LP supply held at burn addresses (V2) | +25 |
| Sell pressure | `Swap` events classified by side | +10 |

Score maps to `Low` (0–19), `Medium` (20–44), `High` (45–69),
`Critical` (70+).

Two deliberate design decisions worth knowing about:

- **Unknown is never treated as safe.** If liquidity, holder distribution, or
  LP status can't be read, that signal is reported as `unknown`, listed under
  "Could not verify", and the band is floored at `Medium`. A scanner that
  prints "Low risk" because an RPC call failed is worse than one that prints
  nothing.
- **The pool and burn addresses are excluded from holder stats.** Tokens
  sitting in the liquidity pool aren't a dump risk the way a whale's wallet
  is, and burned supply can never be sold at all. Counting either produces
  false alarms on completely normal launches.

## Running it

```bash
export RPC_URL=https://your-rpc
export DEX_FACTORY=0x...        # the factory to watch
export DEX_KIND=v2              # v2 or v3
export QUOTE_TOKEN=0x...        # WETH/USDC/etc, used to size liquidity
export QUOTE_USD_PRICE=3000     # omit and liquidity reports as unknown
export QUOTE_DECIMALS=18
export START_BLOCK=0            # factory deployment block is a good value

cargo run --release -p radar
```

See `src/settings.rs` for every variable and its default.

## Reproducing the end-to-end check

The RPC plumbing (log filters, event decoding, contract reads) is verified
against a local node rather than only mocks:

```bash
anvil &
PRIVATE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  forge script script/RadarDemo.s.sol --rpc-url http://127.0.0.1:8545 --broadcast
# then export the addresses it prints and run the radar as above
```

`script/RadarDemo.s.sol` stages a launch — supply spread across holders, a
pool with reserves, burned LP, three buys and a sell — so the radar's output
can be checked against known-correct inputs.

## Limits worth knowing

- **The risk score is a heuristic, not a verdict.** It reads on-chain state;
  it does not decompile the token. A contract with a hidden transfer tax,
  a mint function, a blocklist, or a proxy upgrade path can score `Low`.
  Honeypot detection needs trade simulation, which this doesn't do.
- **`Buys`/`Sells` are transaction counts, not volume.** Ten dust buys
  outrank one large one. Wash trading inflates both trivially.
- **Deployer identification is a heuristic** — the first mint recipient,
  which is usually but not always the deployer.
- **V3 LP status is always `unknown`.** V3 liquidity is held as position
  NFTs, so the V2 burn check doesn't apply.
- **Holder reconstruction covers a block window, not all history.** Set
  `ACTIVITY_WINDOW_BLOCKS` wide enough for the chain's block time, or an
  indexer/subgraph will be faster and more complete.
- Not verified against live Robinhood Chain data — no RPC endpoint for it was
  available when this was written.
