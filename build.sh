#!/bin/bash
set -e

echo "=== Building frontend ==="
cd frontend
bun install
bun run build
cd ..

echo ""
echo "=== Building backend ==="
cargo build --release

echo ""
echo "=== Build complete ==="
echo "Run: ./target/release/provider-relay"
echo "Or:  PORT=8080 ./target/release/provider-relay"
