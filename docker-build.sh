#!/bin/bash
set -e

IMAGE_NAME="provider-relay"
IMAGE_TAG="${1:-latest}"

echo "[docker-build] Building image: ${IMAGE_NAME}:${IMAGE_TAG}"

if docker buildx version &>/dev/null; then
    docker buildx build \
        -t "${IMAGE_NAME}:${IMAGE_TAG}" \
        -f server/Dockerfile \
        --load \
        .
else
    DOCKER_BUILDKIT=1 docker build \
        -t "${IMAGE_NAME}:${IMAGE_TAG}" \
        -f server/Dockerfile \
        .
fi

echo "[docker-build] Done: ${IMAGE_NAME}:${IMAGE_TAG}"
