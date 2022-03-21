#!/bin/bash

set -x
set -e

/code/permissions/server.sh --port 5050 --host 0.0.0.0

# TODO: start server in the background and run some tests
