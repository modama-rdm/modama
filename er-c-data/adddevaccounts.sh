#!/bin/bash

# This script adds accounts for "weber" and "clausen" in the
# containers that require these accounts to test on systems
# that are not set up with Linux IFFLDAP authentication
# The Unix names should match the IFFLDAP account names
docker-compose exec permissions useradd weber
docker-compose exec permissions useradd clausen
docker-compose exec permissions groupadd iff
docker-compose exec permissions adduser weber iff
docker-compose exec permissions adduser clausen iff
echo -e "weber:1234\nclausen:1234" | docker-compose exec -T permissions chpasswd

docker-compose exec webdav-backend useradd weber
docker-compose exec webdav-backend useradd clausen
docker-compose exec webdav-backend groupadd iff
docker-compose exec webdav-backend adduser weber iff
docker-compose exec webdav-backend adduser clausen iff
echo -e "weber:1234\nclausen:1234" | docker-compose exec -T webdav-backend chpasswd
