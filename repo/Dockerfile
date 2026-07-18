# syntax=docker/dockerfile:1.7

# Base images are pinned as tag@digest for reproducibility. Re-pin by running
# `docker pull <tag>` and verifying with
# `docker image inspect <tag> --format '{{index .RepoDigests 0}}'`.

# ── Stage 1: build UI (Node) ─────────────────────────────────────────────────
FROM node:22-bookworm-slim@sha256:7af03b14a13c8cdd38e45058fd957bf00a72bbe17feac43b1c15a689c029c732 AS ui-builder
WORKDIR /ui
COPY ui/package.json ui/package-lock.json* ./
RUN npm ci --prefer-offline
COPY ui/ ./
RUN npm run build

# ── Stage 2: dependency cache (cargo-chef) ───────────────────────────────────
FROM rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS chef
RUN cargo install cargo-chef --version 0.1.70 --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: build dependencies (cached unless Cargo.lock changes) ───────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Build the application
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build -p swift-api --release

# ── Stage 4: minimal runtime image ───────────────────────────────────────────
FROM debian:bookworm-slim@sha256:0104b334637a5f19aa9c983a91b54c89887c0984081f2068983107a6f6c21eeb AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 10001 --create-home --home-dir /var/lib/swiftpipe swiftpipe

WORKDIR /app

COPY --from=builder /app/target/release/swiftpipe-api /usr/local/bin/swiftpipe-api
COPY --from=ui-builder /ui/dist /app/ui
COPY examples/schemas /app/schemas

RUN mkdir -p /data/objects /data/work \
    && chown -R swiftpipe:swiftpipe /data /var/lib/swiftpipe

USER swiftpipe

EXPOSE 8080

HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/healthz || exit 1

ENTRYPOINT ["swiftpipe-api"]
CMD ["--listen", "0.0.0.0:8080", \
     "--schema-path", "/app/schemas", \
     "--object-root", "/data/objects", \
     "--work-root", "/data/work", \
     "--ui-dist", "/app/ui"]
