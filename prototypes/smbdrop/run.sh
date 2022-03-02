#!/bin/bash

DATA_DIR=$1
if [ -z "$DATA_DIR" ]; then
    echo "please pass data directory as argument; exiting..."
    exit 1
fi

BUILDKIT_PROGRESS=plain DOCKER_BUILDKIT=1 docker build . -t smbdrop-test

# need SYS_ADMIN for acl support:
docker run --rm -it -p 5050:5050 -p 445:445 --cap-add SYS_ADMIN  -v "$DATA_DIR:/data/:rw" smbdrop-test:latest