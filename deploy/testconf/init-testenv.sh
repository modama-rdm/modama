#!/bin/bash
# Prepare what a test deployment needs before its services can start:
#
#   * the state folders in testfolder/ with the ownership the services expect
#   * the mTLS certificate authority and the certificates of the smbdrop and ldap servers
#
# Usage:  init-testenv.sh [init|clean]
#
#   init   (default) Create what is missing. Existing files are never
#          overwritten, so running it again is harmless.
#   clean  Delete the state that `init` created: the test folders and, unless
#          KEEP_CA is set, the CA. Run `docker compose down` first.
#
# This runs as root inside the `testenv-init` service of docker-compose.override.yml,
# which `docker compose up` starts before the services that use the results. Root in
# the container is what makes the chown calls work without sudo on the host. To run
# it on its own:
#
#   docker compose run --rm testenv-init [init|clean]
#
# Environment (set by docker-compose.override.yml):
#   TESTFOLDER  the test state folder (default: /testfolder)
#   CA_PATH     where the CA goes (default: /ca). This is testconf/secrets/CA on
#               the host, or CA/ in MODAMA_SECRETS_DIR if that is set, in which
#               case KEEP_CA is set too: that CA may not be ours to delete.
set -euo pipefail

TESTFOLDER=${TESTFOLDER:-/testfolder}
CA_PATH=${CA_PATH:-/ca}

# State folders that are bind-mounted into the services
STATE_DIRS="data nextcloud-data nextcloud-html postgres sampledb etc"

NEXTCLOUD_UID=33  # www-data in the Nextcloud image

init_folders() {
    cd "$TESTFOLDER"

    mkdir -p data/adhoc nextcloud-data nextcloud-html postgres sampledb etc/smbdrop

    # Nextcloud runs as www-data. Chown recursively only the first time, the
    # html folder holds the whole Nextcloud source tree once it is installed.
    for dir in nextcloud-data nextcloud-html; do
        if [ "$(stat -c %u "$dir")" != "$NEXTCLOUD_UID" ]; then
            chown -R "$NEXTCLOUD_UID:$NEXTCLOUD_UID" "$dir"
        fi
    done
    chmod -R o-rwx nextcloud-data

    # The permissions service manages ACLs below data/, as root. Everyone else
    # has to be able to traverse it.
    chown root:root data data/adhoc
    chmod ugo+rwx data

    if [ ! -e etc/smbdrop/shares.json ]; then
        echo '{ "shares": {}, "users": {} }' > etc/smbdrop/shares.json
    fi
}

init_ca() {
    # Same layout as production, see docs/guide/src/install.md ("mTLS CA setup").
    # No password: the key of this CA protects nothing but a test deployment.
    if [ ! -e "$CA_PATH/ca/pub/ca.cert.pem" ]; then
        echo "initializing CA in $CA_PATH"
        ca --ca-path "$CA_PATH" --insecure-no-password initialize-ca
    fi
    # smbdrop expects a server called smbdrop. The name is also the hostname
    # that clients check against the certificate; the same goes for the test
    # directory server, called ldap.
    # A CA that is not ours (KEEP_CA) gets nothing for the test directory.
    servers="smbdrop"
    [ -n "${KEEP_CA:-}" ] || servers="smbdrop ldap"
    for server in $servers; do
        if [ ! -e "$CA_PATH/servers/$server/$server.chain.pem" ]; then
            echo "enrolling server $server"
            ca --ca-path "$CA_PATH" --insecure-no-password enroll-server "$server" "$server"
        fi
    done
    # The services run as different users, and the public part is public
    chmod -R go+rX "$CA_PATH/ca/pub"
}

clean() {
    cd "$TESTFOLDER"
    # The permissions service makes files immutable
    chattr -R -i data 2> /dev/null || true
    # shellcheck disable=SC2086  # the word splitting is the point
    rm -rf $STATE_DIRS
    if [ -z "${KEEP_CA:-}" ]; then
        # The folder itself is a mount point
        find "$CA_PATH" -mindepth 1 -delete
    fi
}

case "${1:-init}" in
    init)
        init_folders
        init_ca
        echo "test environment is ready"
        ;;
    clean)
        clean
        ;;
    *)
        echo "usage: $0 [init|clean]" >&2
        exit 2
        ;;
esac
