# Deploying on a DigitalOcean droplet

Target box: **2 Intel vCPU / 8 GB RAM / 160 GB NVMe / 5 TB transfer.**

## Is that droplet the right size?

It is comfortably oversized, which is fine. Measured on the release builds:

| | radar | liquidator |
|:---|:---|:---|
| Binary size | 4.8 MB | 6.2 MB |
| Resident memory, idle/polling | ~4.6 MB | same order |
| CPU while polling | ~0% | ~0% |

Both services are I/O-bound on RPC calls, not CPU-bound. Peak memory rises
above idle when reconstructing holder balances (a wide block window of
`Transfer` logs is buffered and folded into a map), which is why the units
cap the radar at 1 GB and the liquidator at 2 GB — comfortably above any
realistic peak, low enough that a leak gets killed instead of taking the
whole box down.

Bandwidth is a non-issue. Polling every 15s is ~5,800 requests/day; even
generous multi-MB `getLogs` responses land in the low GB/month against a
5 TB allowance.

**The real constraint is your RPC provider, not this droplet.** These bots
are rate-limit and compute-unit bound. A free-tier RPC key will throttle
you long before 2 vCPUs or 8 GB matter. Spend the money there, not on a
bigger droplet. If you outgrow anything first, it will be the RPC plan.

Two things that *would* change the sizing: running your own node (needs far
more disk than 160 GB on most chains), or adding a full historical indexer
with a local database. Neither is in this repo today.

## Quick start (systemd — the verified path)

```bash
git clone https://github.com/Just-Code-Builder/Alpharouting.git
cd Alpharouting
sudo ./deploy/provision.sh
```

`provision.sh` installs prerequisites and the Rust toolchain, builds both
release binaries, creates the `alpharouting` service user, installs the
binaries to `/usr/local/bin`, and installs the systemd units. It is
idempotent, so re-run it to deploy a code update — it stops the services,
swaps the binaries, and restarts whatever was running.

It deliberately does not write config or key material, and does not start
anything. Then:

### Radar (no secrets)

```bash
sudo cp deploy/radar.env.example /etc/alpharouting/radar.env
sudo $EDITOR /etc/alpharouting/radar.env          # RPC_URL, DEX_FACTORY, QUOTE_TOKEN
sudo systemctl enable --now alpharouting-radar
journalctl -u alpharouting-radar -f
```

### Liquidator (signs transactions — read the key section first)

Only worth running on a chain where Aave V3 is actually deployed.

```bash
sudo cp deploy/liquidator.env.example /etc/alpharouting/liquidator.env
sudo $EDITOR /etc/alpharouting/liquidator.env

# printf, not echo: a trailing newline becomes an opaque key-parse failure
sudo sh -c "printf '%s' '0xYOUR_KEY' > /etc/alpharouting/signing-key"
sudo chown root:root /etc/alpharouting/signing-key
sudo chmod 0600 /etc/alpharouting/signing-key

sudo systemctl enable --now alpharouting-liquidator
```

## The private key is the real risk here

Everything else on this list is routine. This part is not.

The liquidator signs transactions, so a key has to be on the droplet, and
**anything on that droplet is only as safe as the droplet.** One RCE, one
leaked SSH key, one compromised dependency in a future `cargo update`, and
the wallet is gone. Nothing in this deployment changes that; it only
narrows the blast radius.

How it is handled:

- The key lives in `/etc/alpharouting/signing-key`, root-owned and `0600`.
- systemd reads it via `LoadCredential=` **as root**, before dropping to the
  `alpharouting` user, and exposes it under the process's private
  credentials directory. The `alpharouting` user cannot read the original
  file at all.
- It is never in the env file, the unit file, the image, or git. It is not
  in the process environment either, so it does not leak via
  `/proc/<pid>/environ`.
- `.gitignore` covers `*.pem` and `.env`; the key path is outside the repo
  entirely.

What you should still do:

- **Use a dedicated wallet holding only gas.** Not your main wallet. The
  contract itself is `onlyOwner`-gated, so this wallet needs to be the
  contract owner — which means treating it as hot and keeping nothing in it
  beyond gas. Profits accumulate in the *contract*, and
  `withdrawToken`/`withdrawETH` can be called from a cold wallet later if
  you transfer ownership.
- Consider a remote signer or KMS instead of a local key file if this ever
  handles meaningful size. That is a code change, not a config change.
- The radar needs no key at all. If you only run the radar, this whole
  section is moot — which is a good reason to start there.

## Droplet hardening basics

Neither service listens on any port — both are outbound-only — so there is
nothing to expose. Close everything inbound except SSH:

```bash
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow OpenSSH
sudo ufw enable
```

Then, at minimum: SSH keys only (`PasswordAuthentication no`), unattended
security upgrades, and a non-root sudo user. DigitalOcean's firewall at the
control-panel level is worth setting too, since it applies before traffic
reaches the box.

The systemd units already apply `ProtectSystem=strict`, `NoNewPrivileges`,
an empty `CapabilityBoundingSet`, a `@system-service` syscall filter, and
`RestrictAddressFamilies=AF_INET AF_INET6`. Verify with:

```bash
systemd-analyze security alpharouting-radar
```

## Operating it

```bash
systemctl status alpharouting-radar
journalctl -u alpharouting-radar -f            # follow
journalctl -u alpharouting-radar --since "1 hour ago"
systemctl restart alpharouting-radar           # after a config edit
```

**If a service looks hung and logs nothing, check `RUST_LOG` is set.** The
tracing filter defaults to silence when it is unset, which looks identical
to a wedged process. Both env examples set `RUST_LOG=info`.

State (the seen-pool cache and borrower watchlist) lives in
`/var/lib/alpharouting` and survives restarts. Deleting those files forces a
full rescan from `START_BLOCK`, which will re-alert on historical launches.

## Docker alternative

`../Dockerfile` and `docker-compose.yml` are provided, but the systemd path
above is the one that was validated. The Dockerfile was written without a
Docker daemon available to build-test it, so expect to iterate on it.

## Still needed for Robinhood Chain

The radar is chain-agnostic and needs no code change — only config. To point
it at Robinhood Chain, supply:

1. **RPC endpoint URL** (and whether it needs an API key)
2. **Chain ID**
3. **DEX factory address** + whether it is Uniswap V2- or V3-shaped
4. **Quote token address** (the chain's WETH/USDC equivalent) and its decimals

The liquidator additionally needs a lending protocol with liquidatable
positions to exist on that chain, plus a `ChainSpec` entry in
`crates/config/src/chain_spec.rs`. If Aave V3 is not deployed there, the
liquidator has nothing to do and should not be started.
