#!/bin/bash
# Health and consistency checks for the running test deployment; bring it up with
# ./uptest.sh first. Prints one line per check and exits with 1 if any failed.
#
# The checks of the test users assume the test directory (deploy/ldap). They are skipped
# if sssd is set up for another one, see MODAMA_SECRETS_DIR in the guide.
set -uo pipefail
cd "$(dirname "$0")"

PASSED=0
FAILED=0
SKIPPED=0

# check "what" command...   passes if the command succeeds
check() {
    local what=$1
    shift
    local out
    if out=$("$@" 2>&1); then
        echo "PASS  $what"
        PASSED=$((PASSED + 1))
    else
        echo "FAIL  $what"
        [ -n "$out" ] && echo "$out" | head -n 8 | sed 's/^/        /'
        FAILED=$((FAILED + 1))
    fi
}

skip() {
    echo "SKIP  $1"
    SKIPPED=$((SKIPPED + 1))
}

dc() { docker compose "$@"; }

# --- helpers; each one succeeds or fails, and says why on stderr --------------------

http_status() {  # url expected-status [curl options...]
    local url=$1 expected=$2
    shift 2
    local got
    got=$(curl -s -o /dev/null -m 60 -w '%{http_code}' "$@" "$url") || true
    [ "$got" = "$expected" ] || { echo "$url: expected $expected, got $got"; return 1; }
}

service_running() {
    local state
    state=$(dc ps -a --format '{{.State}}' "$1")
    [ "$state" = "running" ] || { echo "state is '${state:-not created}'"; return 1; }
}

service_healthy() {
    local health
    health=$(dc ps --format '{{.Health}}' "$1")
    [ "$health" = "healthy" ] || { echo "health is '$health'"; return 1; }
}

init_succeeded() {
    local status
    status=$(dc ps -a --format '{{.State}} {{.ExitCode}}' testenv-init)
    [ "$status" = "exited 0" ] || { echo "testenv-init: $status"; return 1; }
}

# Running the init again must find everything in place
init_is_idempotent() {
    local out
    out=$(dc run --rm testenv-init 2>&1) || { echo "$out"; return 1; }
    if grep -q "initializing CA\|enrolling server" <<< "$out"; then
        echo "$out"
        return 1
    fi
}

out_equals() {  # expected command...
    local expected=$1 got
    shift
    got=$("$@" 2>&1 | tr -d '\r') || true
    [ "$got" = "$expected" ] || { echo "expected: $expected"; echo "got:      $got"; return 1; }
}

# The smbdrop token API speaks TLS with a certificate of the test CA
tls_verifies() {  # host port ca-file
    curl -sS -m 10 --cacert "$3" --resolve "$1:$2:127.0.0.1" -o /dev/null "https://$1:$2/"
}

# ...and nobody else's: the check above is only worth something if this fails
tls_rejects_without_ca() {  # host port
    ! curl -sS -m 10 --resolve "$1:$2:127.0.0.1" -o /dev/null "https://$1:$2/" 2> /dev/null
}

pam() {  # user password: prints OK, or the PAM error
    dc exec -T permissions /venv/bin/python3 -c "
import sys, pamela
try:
    pamela.authenticate(sys.argv[1], sys.argv[2])
    print('OK')
except pamela.PAMError as e:
    print(e)
" "$1" "$2"
}

# Nextcloud user name (the directory's entryUUID) for a display name
nextcloud_user() {
    dc exec -T nextcloud sudo -u www-data php -d memory_limit=-1 /var/www/html/occ user:list 2> /dev/null \
        | sed -n "s/^ *- \([^:]*\): $1\$/\1/p" | head -n 1 | tr -d '\r'
}

dav() {  # user password path -> output of PROPFIND, status last
    curl -s -m 90 -u "$1:$2" -X PROPFIND -H 'Depth: 1' -w '\n%{http_code}' \
        "http://localhost:1890/nextcloud/remote.php/dav/files/$3"
}

nextcloud_login_works() {
    local id out
    id=$(nextcloud_user "Alice Anderson")
    [ -n "$id" ] || { echo "Alice is not a Nextcloud user: was ./uptest.sh run (applyconfig.py)?"; return 1; }
    out=$(dav alice testpassword "$id/")
    [ "${out##*$'\n'}" = "207" ] || { echo "PROPFIND as alice: ${out##*$'\n'}"; return 1; }
}

nextcloud_rejects_wrong_password() {
    local id out
    id=$(nextcloud_user "Alice Anderson")
    [ -n "$id" ] || { echo "Alice is not a Nextcloud user"; return 1; }
    out=$(dav alice wrong "$id/")
    [ "${out##*$'\n'}" = "401" ] || { echo "PROPFIND with wrong password: ${out##*$'\n'}"; return 1; }
}

# ER-C-Data is the WebDAV gateway: Nextcloud -> webdav -> webdav-backend -> PAM -> sssd -> ldap
nextcloud_external_storage() {
    local id out
    id=$(nextcloud_user "Alice Anderson")
    [ -n "$id" ] || { echo "Alice is not a Nextcloud user"; return 1; }
    out=$(dav alice testpassword "$id/ER-C-Data/")
    [ "${out##*$'\n'}" = "207" ] || { echo "PROPFIND on ER-C-Data: ${out##*$'\n'}"; return 1; }
    grep -q "ER-C-Data/adhoc/" <<< "$out" || { echo "no adhoc/ in ER-C-Data"; return 1; }
}

ldap_search() {  # filter attributes... as the bind account
    local filter=$1
    shift
    dc exec -T ldap ldapsearch -x -LLL -H ldap://localhost \
        -D uid=ldapbind,cn=sysaccounts,cn=etc,dc=iff,dc=kfa-juelich,dc=de -w bind-test-password \
        -b cn=accounts,dc=iff,dc=kfa-juelich,dc=de "$filter" "$@"
}

# Note on the following: capture the output, then match it. `command | grep -q` makes grep
# exit at the first match, and with pipefail the command can then fail on SIGPIPE.
ldap_has_member_of() {
    local out
    out=$(ldap_search '(uid=alice)' memberOf) || { echo "$out"; return 1; }
    grep -q "^memberOf: cn=iff,cn=groups,cn=accounts" <<< "$out" || { echo "no memberOf in: $out"; return 1; }
}

ldap_hides_passwords() {
    local out
    out=$(ldap_search '(uid=alice)' userPassword) || { echo "$out"; return 1; }
    ! grep -qi "^userPassword" <<< "$out" || { echo "userPassword is readable"; return 1; }
}

ldap_needs_bind() {
    local out
    out=$(dc exec -T ldap ldapsearch -x -LLL -H ldap://localhost \
        -b dc=iff,dc=kfa-juelich,dc=de '(uid=alice)' uid 2> /dev/null)
    ! grep -q "^uid:" <<< "$out" || { echo "anonymous search returned entries"; return 1; }
}

sssd_uses_test_directory() {
    dc exec -T sssd grep -q '^ldap_uri = ldap://ldap$' /etc/sssd/sssd.conf
}

# --- the checks ----------------------------------------------------------------------

echo "== Services"
for service in $(dc config --services | grep -v '^testenv-init$'); do
    check "$service is running" service_running "$service"
done
for service in $(dc ps --format '{{.Service}} {{.Health}}' | awk 'NF == 2 {print $1}'); do
    check "$service is healthy" service_healthy "$service"
done
check "testenv-init exited successfully" init_succeeded
check "testenv-init finds everything in place when run again" init_is_idempotent

echo "== Endpoints"
check "management interface redirects to login" http_status http://localhost:5001/ 302
check "new management interface" http_status http://localhost:5001/new/ 200
check "management API health" http_status http://localhost:5001/mgmt-api/healthz 200
check "management API docs" http_status http://localhost:5001/mgmt-api/docs 200
check "smbdrop health" http_status http://localhost:5051/health/ 200
check "landing page" http_status http://localhost:1890/ 200
check "Nextcloud is installed" out_equals '"installed":true' \
    bash -c "curl -s -m 30 http://localhost:1890/nextcloud/status.php | grep -o '\"installed\":true'"
check "WebDAV gateway asks for a login" http_status http://localhost:8081/ 401

echo "== Certificates"
CA=testconf/secrets/CA/ca/pub/ca.cert.pem
if [ -r "$CA" ]; then
    check "smbdrop's certificate verifies against the test CA" tls_verifies smbdrop 5053 "$CA"
    check "smbdrop's certificate is not trusted without it" tls_rejects_without_ca smbdrop 5053
    check "smbdrop's certificate is for smbdrop only" bash -c \
        "! curl -sS -m 10 --cacert $CA --resolve wrong:5053:127.0.0.1 -o /dev/null https://wrong:5053/ 2>/dev/null"
else
    skip "certificates: $CA not readable, the CA is not the test CA"
fi

echo "== Directory and users"
if sssd_uses_test_directory 2> /dev/null; then
    check "directory: members have memberOf" ldap_has_member_of
    check "directory: passwords are not readable" ldap_hides_passwords
    check "directory: nothing without a bind" ldap_needs_bind
    for service in permissions webdav-backend; do
        check "$service: alice is known" out_equals \
            "alice:*:10001:10000:Alice Anderson:/home/alice:/bin/bash" dc exec -T "$service" getent passwd alice
        check "$service: group iff is alice, bob, carol" out_equals \
            "iff:*:10000:alice,bob,carol" dc exec -T "$service" getent group iff
    done
    check "login: alice with her password" out_equals OK pam alice testpassword
    check "login: alice with a wrong password" out_equals \
        "[PAM Error 7] Authentication failure" pam alice wrong
    check "login: mallory is outside iff and denied" out_equals \
        "[PAM Error 6] Permission denied" pam mallory testpassword

    echo "== Nextcloud"
    check "alice logs in via LDAP" nextcloud_login_works
    check "a wrong password is rejected" nextcloud_rejects_wrong_password
    check "alice reaches ER-C-Data through the WebDAV gateway" nextcloud_external_storage
else
    skip "directory and users: sssd does not use the test directory (MODAMA_SECRETS_DIR?)"
fi

echo
echo "$PASSED passed, $FAILED failed, $SKIPPED skipped"
[ "$FAILED" -eq 0 ]
