#!/bin/bash
BUILDKIT_PROGRESS=plain DOCKER_BUILDKIT=1 docker build . -t smbdrop-test

# need SYS_ADMIN for acl support:
docker run --rm -it -p 5050:5050 -p 445:445 --cap-add SYS_ADMIN smbdrop-test:latest
