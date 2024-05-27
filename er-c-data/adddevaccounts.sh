#!/bin/bash

# This script adds accounts for "weber" and "clausen" in the
# containers that require these accounts to test on systems
# that are not set up with Linux IFFLDAP authentication
# The Unix names should match the IFFLDAP account names
docker compose exec permissions bash -c 'useradd weber; useradd clausen; groupadd iff; adduser weber iff; adduser clausen iff;'
echo -e "weber:1234\nclausen:1234" | docker compose exec -T permissions chpasswd

docker compose exec webdav-backend bash -c 'useradd weber; useradd clausen; groupadd iff; adduser weber iff; adduser clausen iff;'
echo -e "weber:1234\nclausen:1234" | docker compose exec -T webdav-backend chpasswd
