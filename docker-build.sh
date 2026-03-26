#!/bin/bash
set -e

IMAGE_NAME="provider-relay"
IMAGE_TAG="${1:-latest}"

echo "[docker-build] Building image: ${IMAGE_NAME}:${IMAGE_TAG}"

docker buildx build \
    --tag "${IMAGE_NAME}:${IMAGE_TAG}" \
    --file Dockerfile \
    --load \
    .

echo "[docker-build] Done: ${IMAGE_NAME}:${IMAGE_TAG}"
