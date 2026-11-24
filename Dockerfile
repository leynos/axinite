# Multi-stage Dockerfile for the Axinite agent (cloud deployment).
#
# Build:
#   docker build --platform linux/amd64 -t axinite:latest .
#
# Run:
#   docker run --env-file .env -p 3000:3000 axinite:latest

# Stage 1: Build
FROM rust:1.95-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake gcc g++ \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-wasip2 \
    && cargo install wasm-tools

WORKDIR /app

# Copy manifests first for layer caching
COPY Cargo.toml Cargo.lock ./

# Copy source, build script, tests, and supporting directories
COPY build.rs build.rs
COPY src/ src/
COPY crates/ crates/
COPY tests/ tests/
COPY migrations/ migrations/
COPY registry/ registry/
COPY channels-src/ channels-src/
COPY wit/ wit/
COPY providers.json providers.json

RUN cargo build --release --bin axinite

# Stage 1b: Build the Monty helper. It has its own workspace because Monty
# requires Rust 1.98, and its `.cargo/config.toml` uses nightly flags, so the
# pinned nightly is installed explicitly instead of relying on the builder's
# stable default.
FROM rust:1.95-slim-bookworm AS monty

RUN rustup toolchain install nightly-2026-10-16 --profile minimal

WORKDIR /app

COPY crates/monty-helper/ crates/monty-helper/
COPY crates/monty-protocol/ crates/monty-protocol/

RUN cd crates/monty-helper \
    && cargo +nightly-2026-10-16 build --release --locked

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/axinite /usr/local/bin/axinite
COPY --from=monty /app/crates/monty-helper/target/release/axinite-monty-helper \
    /usr/local/bin/axinite-monty-helper
COPY --from=builder /app/migrations /app/migrations

# Non-root user
RUN useradd -m -u 1000 -s /bin/bash axinite
USER axinite

EXPOSE 3000

ENV RUST_LOG=axinite=info

ENTRYPOINT ["axinite"]
