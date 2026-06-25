#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cleanup() {
  docker compose down --volumes --remove-orphans || true
}
trap cleanup EXIT

cd "$(dirname "$0")"

./prepare.sh

docker compose run --rm smbdrop-init
docker compose --progress=plain build --pull
docker compose up -d
docker compose run --rm test-runner bash -lc "/opt/venv/bin/python -m pytest -v /tests \"$@\""
