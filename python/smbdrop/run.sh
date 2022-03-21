#!/bin/bash

set -e

DATA_DIR=$1
if [ -z "$DATA_DIR" ]; then
    echo "please pass data directory as argument; exiting..."
    exit 1
fi

BASE_DIR=$(dirname "$(readlink -f "${0}")")/..

cd "$BASE_DIR"

BUILDKIT_PROGRESS=plain DOCKER_BUILDKIT=1 docker build . -f smbdrop/Dockerfile -t smbdrop-test

# need SYS_ADMIN for acl support:
docker run --rm -it -p 5051:5050 -p 445:445 --cap-add SYS_ADMIN  -v "$DATA_DIR:/data/:rw" smbdrop-test:latest
