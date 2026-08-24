FROM rust:1-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock build.rs ./
COPY crates/protocol crates/protocol
COPY src src
RUN cargo build --release --locked --bin prelay-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --uid 1000 relay
WORKDIR /app
COPY --from=builder /app/target/release/prelay-server ./prelay-server
RUN mkdir -p /app/data && chown relay:relay /app/data
EXPOSE 18080
ENV LISTEN_PORT=18080
USER relay
CMD ["./prelay-server"]
