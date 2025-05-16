# This Dockerfile creates an image with the Rust application for the btc-tx-stats project.
FROM rust:1.82-slim-bullseye AS builder

WORKDIR /app

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    libpq-dev \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# For migrations
RUN cargo install diesel_cli --no-default-features --features postgres --locked

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy migrations and code
COPY migrations ./migrations
COPY src ./src
COPY scripts ./scripts

# Build the app
RUN cargo build --release --bin btc-tx-stats

FROM debian:bullseye-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libpq5 \
        postgresql-client \
    && apt-get clean && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/btc-tx-stats /app/btc-tx-stats
COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /usr/local/cargo/bin/diesel /usr/local/bin/diesel

COPY ./scripts/entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

ENTRYPOINT ["/app/entrypoint.sh"] 