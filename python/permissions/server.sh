#!/bin/bash

set -e

FLASK_APP=permissions.server:app flask run "$@"
