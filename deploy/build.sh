#!/bin/bash
export DOCKER_BUILDKIT=1
export COMPOSE_DOCKER_CLI_BUILD=1

# Once override, once production
# They contain different images in their configurations
docker compose build
docker compose -f docker-compose.yml -f docker-compose.prod.yml build
