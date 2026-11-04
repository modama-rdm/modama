#!/bin/bash
# Runs once in a new VM of this project, as the VM user with passwordless sudo, after
# devvm's packages are installed (see .devvm/config.env). Everything of the system
# itself runs in Docker images, so the VM is a Docker host and little else.
set -euo pipefail

# OpenSearch (warehouse) refuses to start on a Docker host below this
echo 'vm.max_map_count = 262144' | sudo tee /etc/sysctl.d/90-opensearch.conf > /dev/null
sudo sysctl -q --system

# uv and uvx, from Astral's installer into ~/.local/bin, which the default Debian
# profile adds to the PATH of login shells once the folder exists
curl -LsSf https://astral.sh/uv/install.sh | env UV_NO_MODIFY_PATH=1 sh
# The git hook of this repository (.pre-commit-config.yaml: ruff) runs through
# pre-commit; a uv tool install rather than uvx, since the hook that pre-commit writes
# points at a persistent interpreter. The Debian package is too old for the config.
~/.local/bin/uv tool install pre-commit
