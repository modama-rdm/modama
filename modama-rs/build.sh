#!/bin/bash
DOCKER_BUILDKIT=1 docker build --progress=plain -f Dockerfile-smbdrop -t smbdrop-rs .
