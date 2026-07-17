# Multi-stage Docker build for Impostor Among Us server (Rust)
#
# Stage 1: Build the binary
# Stage 2: Minimal runtime image

# ── Build Stage ─────────────────────────────────────────────────
FROM rust:1-alpine AS builder

ARG VERSION=unknown
ENV IMPOSTOR_VERSION=${VERSION}

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /build

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY crates/railway-hazel/Cargo.toml ./crates/railway-hazel/
COPY crates/railway-protocol/Cargo.toml ./crates/railway-protocol/
COPY crates/railway-game-logic/Cargo.toml ./crates/railway-game-logic/
COPY crates/railway-server/Cargo.toml ./crates/railway-server/

# Create dummy src files for dependency caching
RUN mkdir -p crates/railway-hazel/src && echo 'fn main() {}' > crates/railway-hazel/src/lib.rs && \
    mkdir -p crates/railway-protocol/src && echo 'fn main() {}' > crates/railway-protocol/src/lib.rs && \
    mkdir -p crates/railway-game-logic/src && echo 'fn main() {}' > crates/railway-game-logic/src/lib.rs && \
    mkdir -p crates/railway-server/src && echo 'fn main() {}' > crates/railway-server/src/main.rs

RUN cargo build --release -p railway-server && \
    rm -rf crates/*/src

# Copy actual source and build
COPY crates ./crates
RUN touch crates/railway-hazel/src/lib.rs && \
    touch crates/railway-protocol/src/lib.rs && \
    touch crates/railway-game-logic/src/lib.rs && \
    touch crates/railway-server/src/main.rs

RUN cargo build --release -p railway-server && \
    strip /build/target/release/railway-server

# ── Runtime Stage ───────────────────────────────────────────────
FROM alpine:3.21

RUN apk add --no-cache ca-certificates tzdata && \
    adduser -D -h /app impostor

USER impostor
WORKDIR /app

COPY --from=builder /build/target/release/railway-server /app/railway-server

# Default config (can be overridden by mounting config.toml)
COPY config.toml /app/config.toml

EXPOSE 22023/udp
EXPOSE 8080/tcp

ENTRYPOINT ["/app/railway-server"]
