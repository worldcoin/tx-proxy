FROM rust:1.85.1 AS base

RUN cargo install sccache --version ^0.9
RUN cargo install cargo-chef --version ^0.1

ENV CARGO_HOME=/usr/local/cargo
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_DIR=/sccache

#
# Planner container (running "cargo chef prepare")
#
FROM base AS planner
WORKDIR /app

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef prepare --recipe-path recipe.json

#
# Builder container (running "cargo chef cook" and "cargo build --release")
#
FROM base AS builder
WORKDIR /app

# Default binary filename
ARG TX_PROXY_BIN="tx-proxy"
COPY --from=planner /app/recipe.json recipe.json

RUN --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo build --release --package=${TX_PROXY_BIN}

#
# Runtime container
#
FROM debian:bookworm-slim
WORKDIR /app

ARG TX_PROXY_BIN="tx-proxy"
COPY --from=builder /app/target/release/${TX_PROXY_BIN} /usr/local/bin/

EXPOSE 8545 9001

ENTRYPOINT ["/usr/local/bin/tx-proxy"]
