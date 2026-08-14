# Build stage
FROM rust:1-slim AS builder

WORKDIR /app

# Install SSL certificates and build essentials
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Run stage
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/backend /app/backend

ENV PORT=3000
EXPOSE 3000

CMD ["/app/backend"]
