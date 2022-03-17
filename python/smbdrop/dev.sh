#!/bin/bash

set -e

FLASK_APP=smbdrop.server:app flask run "$@"
