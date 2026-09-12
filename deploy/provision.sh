#!/usr/bin/env bash
#
# Provisions a fresh Debian/Ubuntu droplet to run the radar and/or the
# liquidator under systemd. Idempotent — safe to re-run after a code update.
#
#   sudo ./deploy/provision.sh
#
# Builds from source in the current checkout, installs the binaries to
# /usr/local/bin, installs the systemd units, and creates the config
# directory. It deliberately does NOT write config or key material, and does
# NOT start the services: both need values only you have.

set -euo pipefail

SERVICE_USER="alpharouting"
CONFIG_DIR="/etc/alpharouting"
STATE_DIR="/var/lib/alpharouting"
BIN_DIR="/usr/local/bin"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

log() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m warning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31m error:\033[0m %s\n' "$*" >&2; exit 1; }

[[ ${EUID} -eq 0 ]] || die "run as root (sudo $0)"
command -v systemctl >/dev/null || die "systemd not found; this script targets a systemd host"

log "Installing build and runtime prerequisites"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
# libssl-dev/pkg-config are needed to build; ca-certificates and libssl3 are
# needed at runtime for HTTPS RPC endpoints.
apt-get install -y -qq --no-install-recommends \
    build-essential pkg-config libssl-dev ca-certificates curl libssl3

if ! command -v cargo >/dev/null; then
    log "Installing the Rust toolchain"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
fi

log "Building release binaries (this takes a few minutes on 2 vCPUs)"
cd "${REPO_ROOT}"
cargo build --release -p radar -p liquidator

log "Creating service user '${SERVICE_USER}'"
if ! id -u "${SERVICE_USER}" >/dev/null 2>&1; then
    # No login shell and no home: this account only ever runs the services.
    useradd --system --no-create-home --shell /usr/sbin/nologin "${SERVICE_USER}"
else
    log "  user already exists, leaving it alone"
fi

log "Installing binaries to ${BIN_DIR}"
# Stop services first if running, so a busy binary can be replaced.
for svc in alpharouting-radar alpharouting-liquidator; do
    if systemctl is-active --quiet "${svc}" 2>/dev/null; then
        log "  stopping ${svc} to replace its binary"
        systemctl stop "${svc}"
        touch "/tmp/.${svc}.was-running"
    fi
done
install -m 0755 -o root -g root "${REPO_ROOT}/target/release/radar" "${BIN_DIR}/radar"
install -m 0755 -o root -g root "${REPO_ROOT}/target/release/liquidator" "${BIN_DIR}/liquidator"

log "Creating ${CONFIG_DIR} and ${STATE_DIR}"
# Config is root-owned; the service user needs no read access because
# systemd reads EnvironmentFile and credentials as root before dropping privs.
install -d -m 0750 -o root -g root "${CONFIG_DIR}"
install -d -m 0750 -o "${SERVICE_USER}" -g "${SERVICE_USER}" "${STATE_DIR}"

log "Installing systemd units"
install -m 0644 -o root -g root \
    "${REPO_ROOT}/deploy/systemd/alpharouting-radar.service" \
    "${REPO_ROOT}/deploy/systemd/alpharouting-liquidator.service" \
    /etc/systemd/system/
systemctl daemon-reload

for svc in alpharouting-radar alpharouting-liquidator; do
    if [[ -f "/tmp/.${svc}.was-running" ]]; then
        log "Restarting ${svc}"
        systemctl start "${svc}"
        rm -f "/tmp/.${svc}.was-running"
    fi
done

log "Done. Remaining manual steps:"
cat <<EOF

  1. Radar config (no secrets):
       cp ${REPO_ROOT}/deploy/radar.env.example ${CONFIG_DIR}/radar.env
       \$EDITOR ${CONFIG_DIR}/radar.env
       systemctl enable --now alpharouting-radar
       journalctl -u alpharouting-radar -f

  2. Liquidator — only on a chain where Aave V3 is deployed.
     The signing key goes in its own root-owned file, NOT in the env file:

       cp ${REPO_ROOT}/deploy/liquidator.env.example ${CONFIG_DIR}/liquidator.env
       \$EDITOR ${CONFIG_DIR}/liquidator.env

       # write the key with no trailing newline, then lock it down
       printf '%s' '0xYOUR_PRIVATE_KEY' > ${CONFIG_DIR}/signing-key
       chown root:root ${CONFIG_DIR}/signing-key
       chmod 0600 ${CONFIG_DIR}/signing-key

       systemctl enable --now alpharouting-liquidator

  Use a dedicated wallet holding only gas. Anything on this droplet is only
  as safe as the droplet.

EOF
