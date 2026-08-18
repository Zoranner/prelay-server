FROM rust:1-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY crates crates
COPY server server
RUN cargo build --manifest-path server/Cargo.toml --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --uid 1000 relay
WORKDIR /app
COPY --from=builder /app/server/target/release/provider-relay-server ./provider-relay-server
RUN mkdir -p /app/data && chown relay:relay /app/data
VOLUME ["/app/data"]
EXPOSE 18080
ENV LISTEN_PORT=18080
USER relay
CMD ["./provider-relay-server"]
