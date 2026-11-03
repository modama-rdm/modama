#!/bin/bash
# Stop the test deployment. The state stays, `uptest.sh` continues where it left off.
#
#   downtest.sh           stop the containers
#   downtest.sh --reset   also delete all state: the directory, the Nextcloud installation
#                         and data, the database, the CA. The next start is a fresh one.
#
# Both steps of --reset belong together: Nextcloud remembers the users of the directory
# by ID, and a new directory has new IDs.
set -euo pipefail
cd "$(dirname "$0")"

case "${1:-}" in
    "")
        docker compose down
        ;;
    --reset)
        docker compose down --volumes
        docker compose run --rm testenv-init clean
        ;;
    *)
        echo "usage: $0 [--reset]" >&2
        exit 2
        ;;
esac
