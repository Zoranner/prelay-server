# Stage 1: 构建 Web 前端
FROM oven/bun:1 AS web-builder
WORKDIR /app/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY web/ .
RUN bun run build
# vite outDir: '../static' → 输出到 /app/static

# Stage 2: cargo-chef 生成依赖清单
FROM rust:1.86-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/ ./src/
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: 编译 Rust 后端
FROM chef AS backend-builder
RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
RUN SKIP_FRONTEND_BUILD=1 cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/ ./src/
RUN SKIP_FRONTEND_BUILD=1 cargo build --release

# Stage 4: 最小运行镜像
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home relay
WORKDIR /app
COPY --from=backend-builder /app/target/release/provider-relay .
COPY --from=web-builder /app/static ./static
RUN mkdir -p /app/data && chown relay:relay /app/data
VOLUME ["/app/data"]
EXPOSE 3000
ENV LISTEN_PORT=3000
USER relay
CMD ["./provider-relay"]
