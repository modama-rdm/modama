#!/bin/bash
# Runs once in a new VM of this project, as the VM user with passwordless sudo, after
# devvm's packages are installed (see .devvm/config.env). Everything of the system
# itself runs in Docker images, so the VM is a Docker host and little else.
set -euo pipefail

# OpenSearch (warehouse) refuses to start on a Docker host below this
echo 'vm.max_map_count = 262144' | sudo tee /etc/sysctl.d/90-opensearch.conf > /dev/null
sudo sysctl -q --system
