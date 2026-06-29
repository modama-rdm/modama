#!/bin/bash
set -ex
rm -rf certs/
docker compose down --volumes --remove-orphans
