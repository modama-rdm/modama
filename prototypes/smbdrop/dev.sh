#!/bin/bash

set -e

mkdir -p /data/drop/new-dataset/
chmod 755 /data/drop/new-dataset/
chown 1000:1000 /data/drop/new-dataset/

FLASK_APP=smbdrop.server:app flask run "$@"
