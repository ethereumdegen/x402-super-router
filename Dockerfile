# Build stage
FROM rust:1.88-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev cmake && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates libssl3 ffmpeg && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/x402-super-router /app/x402-super-router
COPY --from=builder /app/target/release/migrate /app/migrate
COPY migrations /app/migrations
COPY endpoints.ron /app/endpoints.ron
RUN mkdir -p /app/tmp
EXPOSE 3402
CMD ["/app/x402-super-router"]
