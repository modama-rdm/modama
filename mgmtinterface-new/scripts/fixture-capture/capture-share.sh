#!/usr/bin/env bash
# Capture a REAL SerializedShare shape by creating a throwaway ingest share,
# capturing it, then deleting it. Unlike capture.sh this one mutates state —
# only ever run it against a test stack (the UTM VM), never prod. It cleans up
# after itself (DELETE) even on failure (trap), but treat the target project as
# disposable regardless.
set -euo pipefail

BASE="${BASE_URL:?set BASE_URL, e.g. http://localhost:5001 (via the VM SSH tunnel) — never point this at prod}"
TOKEN_FILE="${TOKEN_FILE:?set TOKEN_FILE to a chmod-600 file holding base64(username:password)}"
OUT="$(dirname "$0")/raw"

PROJECT="${1:?usage: BASE_URL=... TOKEN_FILE=... capture-share.sh <project-name> <session-name>}"
DATASET="${2:?usage: BASE_URL=... TOKEN_FILE=... capture-share.sh <project-name> <session-name>}"

[ -r "$TOKEN_FILE" ] || { echo "missing $TOKEN_FILE" >&2; exit 1; }
mkdir -p "$OUT"
AUTH=(--header "Authorization: Basic $(cat "$TOKEN_FILE")" --header 'Accept: application/json')

SHARE_ID=""
cleanup() {
  if [ -n "$SHARE_ID" ]; then
    echo "cleaning up: DELETE /api/share/${SHARE_ID}/"
    curl --silent --show-error "${AUTH[@]}" -X DELETE "${BASE}/api/share/${SHARE_ID}/" -o /dev/null || true
  fi
}
trap cleanup EXIT

echo "creating throwaway share for ${PROJECT}/${DATASET}..."
curl --fail --silent --show-error "${AUTH[@]}" \
     --header 'Content-Type: application/json' \
     --data "{\"project\":\"${PROJECT}\",\"dataset\":\"${DATASET}\"}" \
     "${BASE}/api/share/" -o "${OUT}/share-created.json"
SHARE_ID="$(jq -r '.share_id' "${OUT}/share-created.json")"
echo "created share_id=${SHARE_ID}"

curl --fail --silent --show-error --get "${AUTH[@]}" "${BASE}/api/share/" -o "${OUT}/shares-with-active.json"
echo "captured raw/share-created.json and raw/shares-with-active.json"
echo "(cleanup runs automatically on exit — DELETE /api/share/${SHARE_ID}/)"
