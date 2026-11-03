#!/bin/bash
# docker compose for the WSL test stack, with the base and test override files from deploy/;
# any arguments are passed through: ./compose.sh ps, ./compose.sh down, ...
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The stack publishes SMB and the management API on all addresses. Inside WSL2 with NAT
# networking only the Windows host can reach them; anywhere else they would be on the network.
if ! grep -qi microsoft /proc/sys/kernel/osrelease; then
    echo "$0: refusing to run outside WSL2; this stack publishes SMB and the management API" \
        "on all addresses" >&2
    exit 1
fi
# older WSL has no `wslinfo --networking-mode`; only NAT existed then
mode="$(wslinfo --networking-mode 2>/dev/null || true)"
if [[ -n "$mode" && "$mode" != "nat" ]]; then
    echo "$0: WSL networking mode is $mode, this stack needs nat" \
        "(in mirrored mode the Windows SMB server holds port 445)" >&2
    exit 1
fi

# deploy/ is the project directory, so relative paths, .env and the project name resolve as for
# the other compose files there
exec docker compose --project-directory "$here/.." \
    -f "$here/../docker-compose.yml" \
    -f "$here/../docker-compose.override.yml" \
    -f "$here/docker-compose.wsl.yml" \
    "$@"
