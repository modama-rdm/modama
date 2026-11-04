#!/bin/bash
# Run a command against the WSL test stack: the test deployment with the ports of
# docker-compose.wsl.yml. It sets COMPOSE_FILE, which every `docker compose` call in the
# command picks up, the ones in deploy/uptest.sh, runtest.sh and downtest.sh included:
#
#   deploy/wsl/run.sh deploy/uptest.sh
#   deploy/wsl/run.sh deploy/runtest.sh
#   deploy/wsl/run.sh docker compose logs -f smbdrop
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
deploy="$(cd "$here/.." && pwd)"

if [ $# -eq 0 ]; then
    echo "usage: $0 <command> [arguments...], for example: $0 deploy/uptest.sh" >&2
    exit 2
fi

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

# Absolute paths, so it works from any directory. The project directory is the folder of the
# first file, deploy/, so relative paths, .env and the project name resolve as without this.
export COMPOSE_FILE="$deploy/docker-compose.yml:$deploy/docker-compose.override.yml:$here/docker-compose.wsl.yml"
export COMPOSE_PATH_SEPARATOR=:
exec "$@"
