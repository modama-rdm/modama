#!/usr/bin/env bash
# Read-only capture of mgmtinterface API responses, for building/refreshing the
# journey-test fixtures in ../../src/test/fixtures/. GET only — never mutates
# anything on the target. See README.md in this directory for the full
# capture -> scrub -> write-fixtures workflow and its safety rules.
set -euo pipefail

BASE="${BASE_URL:?set BASE_URL, e.g. https://er-c-data-mgmt.fz-juelich.de or http://localhost:5001}"
TOKEN_FILE="${TOKEN_FILE:?set TOKEN_FILE to a chmod-600 file holding base64(username:password) — never pass the credential on the command line}"
OUT="$(dirname "$0")/raw"

PROJECT="${1:?usage: BASE_URL=... TOKEN_FILE=... capture.sh <project-name> [browse-path ...]}"
shift || true
BROWSE_PATHS=("$@")

[ -r "$TOKEN_FILE" ] || { echo "missing $TOKEN_FILE" >&2; exit 1; }
mkdir -p "$OUT"

get() {
  local path="$1" dest="$2"
  curl --fail --silent --show-error --get \
       --header "Authorization: Basic $(cat "$TOKEN_FILE")" \
       --header 'Accept: application/json' \
       "${BASE}${path}" -o "${OUT}/${dest}"
  echo "captured ${path} -> raw/${dest}"
}

get "/api/project/"                                          projects.json
get "/api/project/$(printf %s "$PROJECT" | jq -sRr @uri)/"    project-details.json
get "/api/users/list/"                                       users.json
get "/api/share/"                                             shares.json

i=0
for p in ${BROWSE_PATHS+"${BROWSE_PATHS[@]}"}; do
  i=$((i + 1))
  curl --fail --silent --show-error --get \
       --header "Authorization: Basic $(cat "$TOKEN_FILE")" \
       --header 'Accept: application/json' \
       --data-urlencode "path=${p}" \
       "${BASE}/api/browse/" -o "${OUT}/browse-${i}.json"
  echo "captured /api/browse/?path=${p} -> raw/browse-${i}.json"
done
