#!/bin/bash
docker compose -f docker-compose.yml -f docker-compose.prod.yml pull
docker pull ghcr.io/modama-rdm/ca:latest
