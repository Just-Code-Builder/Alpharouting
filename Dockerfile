# Alternative to the systemd deployment in deploy/. On a single droplet
# systemd is the lighter path; this exists for reproducible builds and for
# running elsewhere.
#
# NOTE: this Dockerfile was not build-tested in the environment it was
# written in (no Docker daemon available there). The systemd path in
# deploy/ is the verified one.
#
#   docker build -t alpharouting .
#   docker run --rm --env-file radar.env alpharouting radar

FROM rust:1-slim-bookworm AS builder

# alloy links against system OpenSSL, so the build needs its headers.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy manifests first so dependency compilation caches across source edits.
COPY Cargo.toml Cargo.lock ./
COPY crates/config/Cargo.toml      crates/config/
COPY crates/chain/Cargo.toml       crates/chain/
COPY crates/observability/Cargo.toml crates/observability/
COPY crates/workers/Cargo.toml     crates/workers/
COPY crates/liquidator/Cargo.toml  crates/liquidator/
COPY crates/radar/Cargo.toml       crates/radar/

# Stub sources so `cargo build` resolves and compiles the dependency graph
# without the real code, giving a cacheable layer.
RUN for c in config chain observability workers; do \
        mkdir -p "crates/$c/src" && echo "" > "crates/$c/src/lib.rs"; \
    done \
    && for c in liquidator radar; do \
        mkdir -p "crates/$c/src" \
        && echo "" > "crates/$c/src/lib.rs" \
        && echo "fn main() {}" > "crates/$c/src/main.rs"; \
    done \
    && cargo build --release -p radar -p liquidator \
    && rm -rf crates/*/src

COPY crates ./crates

# Touch the real sources so cargo rebuilds them rather than trusting the
# stub artifacts' timestamps.
RUN find crates -name '*.rs' -exec touch {} + \
    && cargo build --release -p radar -p liquidator

FROM debian:bookworm-slim

# ca-certificates for HTTPS RPC endpoints; libssl3 is the runtime half of
# what the build linked against (confirmed via ldd: libssl.so.3, libcrypto.so.3).
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --no-create-home --shell /usr/sbin/nologin --uid 10001 alpharouting \
    && mkdir -p /var/lib/alpharouting \
    && chown alpharouting:alpharouting /var/lib/alpharouting

COPY --from=builder /build/target/release/radar      /usr/local/bin/radar
COPY --from=builder /build/target/release/liquidator /usr/local/bin/liquidator

USER alpharouting
WORKDIR /var/lib/alpharouting

# No default command: pick `radar` or `liquidator` explicitly, since they
# take different configuration.
