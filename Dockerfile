# ── ui-builder: compile admin UI ─────────────────────────────────────────────
FROM node:22-alpine AS ui-builder

WORKDIR /app/admin-ui
COPY admin-ui/package.json admin-ui/package-lock.json ./
RUN npm ci
COPY admin-ui/ ./
RUN npm run build

# ── base: dependency cache layer ─────────────────────────────────────────────
FROM rust:1.93-bookworm AS base

WORKDIR /app

# Install system dependencies required by aws-lc-sys and native-tls
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    clang \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests and lockfile for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY proxy/Cargo.toml proxy/Cargo.toml
COPY migration/ migration/

# Create a dummy binary so `cargo build` can cache dependencies
RUN mkdir -p proxy/src && echo 'fn main() {}' > proxy/src/main.rs \
    && cargo build --release \
    && rm -rf proxy/src

# ── dev: hot-reload stage ─────────────────────────────────────────────────────
FROM base AS dev

RUN cargo install cargo-watch

ENV BR_PROXY_BIND_ADDR=0.0.0.0:5434
ENV BR_ADMIN_BIND_ADDR=0.0.0.0:5435

WORKDIR /app/proxy

CMD ["cargo", "watch", "-x", "run"]

# ── builder: compile release binary ──────────────────────────────────────────
FROM base AS builder

COPY proxy/src proxy/src

RUN cargo build --release

# ── prod: minimal runtime image ───────────────────────────────────────────────
FROM debian:bookworm-slim AS prod

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Pre-create /data so the volume mount has a writable landing spot
RUN mkdir -p /data && chown -R 1000:1000 /data

COPY --from=builder /app/target/release/proxy /usr/local/bin/proxy
COPY --from=ui-builder /app/admin-ui/dist /usr/local/share/admin-ui

ENV BR_PROXY_BIND_ADDR=0.0.0.0:5434
ENV BR_ADMIN_BIND_ADDR=0.0.0.0:5435

EXPOSE 5434
EXPOSE 5435

USER 1000
CMD ["/usr/local/bin/proxy"]
