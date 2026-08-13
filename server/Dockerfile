FROM rust:1-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates/protocol/Cargo.toml crates/protocol/Cargo.toml
COPY server/Cargo.toml server/Cargo.toml
COPY client/src-tauri/Cargo.toml client/src-tauri/Cargo.toml
COPY crates crates
COPY server server
RUN cargo build --release -p provider-relay-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --uid 1000 relay
WORKDIR /app
COPY --from=builder /app/target/release/provider-relay-server ./provider-relay-server
RUN mkdir -p /app/data && chown relay:relay /app/data
VOLUME ["/app/data"]
EXPOSE 18080
ENV LISTEN_PORT=18080
USER relay
CMD ["./provider-relay-server"]
