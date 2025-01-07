#!/bin/bash
set -eux
set -o pipefail

# if there is a certificate, enable HTTPS:
/usr/bin/maybe-enable-https.sh

exec /usr/sbin/nginx -g "daemon off;"
