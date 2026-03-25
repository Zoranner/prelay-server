# Stage 1: 构建前端
FROM oven/bun:1 AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile
COPY frontend/ .
RUN bun run build

# Stage 2: 构建 Rust 后端
FROM rust:1.86-slim AS backend-builder
WORKDIR /app
RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*
# 先只复制依赖文件，利用 Docker 层缓存
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
# 再复制真正的源码并编译
COPY src/ ./src/
RUN touch src/main.rs && cargo build --release
# 复制前端构建产物
COPY --from=frontend-builder /app/static ./static

# Stage 3: 最小运行镜像
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=backend-builder /app/target/release/provider-relay .
COPY --from=backend-builder /app/static ./static
RUN mkdir -p /app/data
VOLUME ["/app/data"]
EXPOSE 3000
ENV PORT=3000
CMD ["./provider-relay"]
