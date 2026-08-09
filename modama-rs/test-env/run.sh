#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

docker compose down --remove-orphans

# cleanup() {
#   docker compose down --volumes --remove-orphans || true
# }
# trap cleanup EXIT

cd "$(dirname "$0")"

./prepare.sh

docker compose run --rm smbdrop-init
docker compose --progress=plain build --pull
docker compose up -d

docker compose exec permissions bash -c 'useradd weber; useradd clausen; groupadd iff; adduser weber iff; adduser clausen iff;'
echo -e "weber:1234\nclausen:1234" | docker compose exec -T permissions chpasswd
docker compose exec permissions bash -c 'mkdir -p /data/adhoc/testproject/session{1,2,3,4,5}/raw/'
docker compose exec permissions bash -c 'chown clausen -R /data/adhoc/testproject/'
docker compose exec permissions bash -c 'chgrp iff -R /data/adhoc/testproject/'

docker compose exec permissions bash -c 'mkdir -p /data/adhoc/weberproject/wsession{1,2,3}/raw/'
docker compose exec permissions bash -c 'chown weber -R /data/adhoc/weberproject/'
docker compose exec permissions bash -c 'chgrp iff -R /data/adhoc/weberproject/'
docker compose exec permissions bash -c 'setfacl -m default:user:clausen:rwx /data/adhoc/weberproject/'
docker compose exec permissions bash -c 'setfacl -Rm user:clausen:rwx /data/adhoc/weberproject/'

docker compose run --rm test-runner /opt/venv/bin/python -m pytest -v tests "$@"
