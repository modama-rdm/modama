#!/bin/bash
# Start the WSL test stack and follow its logs
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$here/compose.sh" up -d
exec "$here/compose.sh" logs -f --tail=100
