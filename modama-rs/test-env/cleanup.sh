#!/bin/bash
set -ex

cd "$(dirname "$0")"
source ./compose.sh

rm -rf ./certs/
compose run --rm smbdrop-init bash -c 'rm -rf /data/adhoc/'
compose down --volumes --remove-orphans
