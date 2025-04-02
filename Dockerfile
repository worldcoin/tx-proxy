FROM rust:1.85.0

COPY . .

RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/target \
    cargo install --path . --locked

ENTRYPOINT [ "tx-proxy" ]