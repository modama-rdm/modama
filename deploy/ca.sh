#/bin/bash
docker run --rm -it -v /var/local/data-mgmt/CA/:/certs/ ghcr.io/modama-rdm/ca:latest "$@"
