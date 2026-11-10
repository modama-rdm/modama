#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
source ./compose.sh

compose down --remove-orphans

./prepare.sh

# docker, unlike podman, doesn't create a missing bind mount source
mkdir -p testfolder/data

compose run --rm smbdrop-init
compose build "${build_flags[@]}" --pull
compose up -d

compose exec permissions bash -c 'useradd weber; useradd clausen; groupadd iff; adduser weber iff; adduser clausen iff;'
echo -e "weber:1234\nclausen:1234" | compose exec -T permissions chpasswd
compose exec permissions bash -c 'mkdir -p /data/adhoc/testproject/session{1,2,3,4,5}/raw/'
compose exec permissions bash -c 'chown clausen -R /data/adhoc/testproject/'
compose exec permissions bash -c 'chgrp iff -R /data/adhoc/testproject/'
# The test runner runs as uid 1000, the session helper's user, and the stale-share test makes
# its own session folders in testproject
compose exec permissions bash -c 'setfacl -Rm user:1000:rwx,default:user:1000:rwx /data/adhoc/testproject/'

compose exec permissions bash -c 'mkdir -p /data/adhoc/weberproject/wsession{1,2,3}/raw/'
compose exec permissions bash -c 'chown weber -R /data/adhoc/weberproject/'
compose exec permissions bash -c 'chgrp iff -R /data/adhoc/weberproject/'
compose exec permissions bash -c 'setfacl -m default:user:clausen:rwx /data/adhoc/weberproject/'
compose exec permissions bash -c 'setfacl -Rm user:clausen:rwx /data/adhoc/weberproject/'

compose run --rm test-runner /opt/venv/bin/python -m pytest -v tests "$@"
