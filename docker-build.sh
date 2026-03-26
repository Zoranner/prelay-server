#!/bin/bash
set -e

IMAGE_NAME="provider-relay"
IMAGE_TAG="${1:-latest}"

echo "[docker-build] Building image: ${IMAGE_NAME}:${IMAGE_TAG}"

docker buildx build \
    -t "${IMAGE_NAME}:${IMAGE_TAG}" \
    -f Dockerfile \
    --load \
    .

echo "[docker-build] Done: ${IMAGE_NAME}:${IMAGE_TAG}"
