#!/bin/bash
# Create a local test user for the WSL test deployment, where sssd has no LDAP.
# NSS and PAM look at local files before sssd, so a local user is enough for
# login (permissions) and for switching to the user's uid (webdav-backend).
# Containers lose the user when they are recreated; just run this again.
#
# usage: ./testuser.sh <username> <password> [uid]
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
username=${1:?usage: $0 <username> <password> [uid]}
password=${2:?usage: $0 <username> <password> [uid]}
uid=${3:-10001}
# ADHOC_GROUP in ../permissions/config.py
group=iff
gid=10000

for service in permissions webdav-backend; do
    # shellcheck disable=SC2016 # expanded by the shell in the container
    "$here/compose.sh" exec -T -e U="$username" -e P="$password" -e UID_="$uid" -e G="$group" -e GID_="$gid" "$service" sh -c '
        getent group "$G" >/dev/null || groupadd -g "$GID_" "$G"
        id "$U" >/dev/null 2>&1 || useradd -M -u "$UID_" -g "$G" -G "$G" -s /usr/sbin/nologin "$U"
        echo "$U:$P" | chpasswd
        echo "$(hostname): $(id "$U")"
    ' < /dev/null
done
