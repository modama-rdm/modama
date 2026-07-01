#!/bin/bash
set -ex
rm -rf ./certs/
docker compose run --rm smbdrop-init bash -c 'rm -rf /data/adhoc/'
docker compose down --volumes --remove-orphans
