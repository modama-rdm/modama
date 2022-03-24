set -e

gunicorn --worker-class sync --workers 1 --bind 0.0.0.0:5051 smbdrop.server:app
