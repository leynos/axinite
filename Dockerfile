# Multi-stage Dockerfile for the Axinite agent (cloud deployment).
#
# Build:
#   docker build --platform linux/amd64 -t axinite:latest .
#
# Run:
#   docker run --env-file .env -p 3000:3000 axinite:latest

# Stage 1: Build
FROM rust:1.93-slim-bookworm AS builder

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

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/axinite /usr/local/bin/axinite
COPY --from=builder /app/migrations /app/migrations

# Non-root user
RUN useradd -m -u 1000 -s /bin/bash axinite
USER axinite

EXPOSE 3000

ENV RUST_LOG=axinite=info

ENTRYPOINT ["axinite"]
