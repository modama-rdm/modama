#!/bin/bash
# Bring up the test deployment and bring it into the state that runtest.sh expects:
# wait until the services are healthy and Nextcloud has installed itself, then apply the
# configuration (LDAP, external storage) that Nextcloud's installation does not.
#
# Extra arguments go to `docker compose up`, for example --build. Unlike upprod.sh this
# does not follow the logs, so it can be used in a script. Idempotent.
set -euo pipefail
cd "$(dirname "$0")"

TIMEOUT=${UPTEST_TIMEOUT:-900}  # seconds, per wait; the first start is slow

docker compose up -d "$@"

wait_for() {  # description, command...
    local what=$1
    shift
    local deadline=$((SECONDS + TIMEOUT))
    echo -n "waiting for $what "
    until "$@" > /dev/null 2>&1; do
        if [ $SECONDS -ge $deadline ]; then
            echo "- timeout after ${TIMEOUT}s" >&2
            exit 1
        fi
        echo -n "."
        sleep 3
    done
    echo " ok"
}

all_healthy() {
    # Every service that has a health check; none may be unhealthy or still starting
    local states
    states=$(docker compose ps --format '{{.Health}}' | grep -v '^$' | sort -u)
    [ "$states" = "healthy" ]
}

nextcloud_installed() {
    # Not `curl | grep -q`: with pipefail, grep quitting early can fail the pipe
    local status
    status=$(curl -fs -m 5 http://localhost:1890/nextcloud/status.php) || return 1
    [[ $status == *'"installed":true'* ]]
}

wait_for "healthy services" all_healthy
wait_for "Nextcloud to install itself" nextcloud_installed

# Not run by Nextcloud's own entrypoint, see "Apply configuration" in the guide
echo "applying Nextcloud configuration"
docker compose exec -T nextcloud python3 /applyconfig.py > /dev/null

echo "test deployment is up; run ./runtest.sh to check it"
