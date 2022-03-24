#!/bin/bash

set -e

gunicorn --bind 0.0.0.0:5050 permissions.server:app
