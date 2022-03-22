#!/bin/bash

set -e

gunicorn --bind 0.0.0.0:5000 mgmtinterface:app
