#/bin/bash
docker run --rm -it -v /var/local/data-mgmt/CA/:/certs/ iffregistry.fz-juelich.de/weber/data-management-system/ca:latest "$@"
