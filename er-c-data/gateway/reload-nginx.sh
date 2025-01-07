#!/bin/bash
set -eu
set -o pipefail

# first, possibly enable the HTTPS config...
/usr/bin/maybe-enable-https.sh

# then do the reload:
supervisorctl signal HUP nginx