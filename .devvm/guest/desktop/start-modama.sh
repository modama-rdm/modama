#!/bin/bash
# Start modama-agent and modama-agent-gui on the virtual desktop :1, against the test
# deployment (deploy/uptest.sh) of the checkout REPO. Run through `devvm desktop-modama`,
# after a release build of both in REPO/modama-rs. The agent is enrolled with the test CA
# once, its config kept in ~/agent.toml. The GUI starts the session helper. Idempotent.
set -euo pipefail
REPO=${REPO:-$HOME/modama}
cd "$REPO"
BIN=$REPO/modama-rs/target/release
for bin in modama-agent modama-agent-gui; do
    [ -x "$BIN/$bin" ] || { echo "$BIN/$bin is missing; build it first:" \
        "cd $REPO/modama-rs && cargo build --release -p modama-agent -p modama-agent-gui" >&2; exit 1; }
done
AGENT_NAME=${AGENT_NAME:-desktop}
RESOURCE_NAME=${RESOURCE_NAME:-desktop-lab}

if [ ! -s ~/agent.toml ]; then
    ca() { (cd deploy && docker compose run --rm -T --entrypoint ca testenv-init \
        --ca-path /ca --insecure-no-password "$@"); }
    ca enroll-agent "$AGENT_NAME" "$RESOURCE_NAME" > /dev/null
    ca generate-agent-config --event-url wss://smbdrop:5052 \
        --token-service-url https://smbdrop:5053 \
        --mgmt-api-url http://smbdrop:5001 --smb-hostname smbdrop --smb-port 1446 \
        --agent-name "$AGENT_NAME" --resource-name "$RESOURCE_NAME" > ~/agent.toml.new
    mv ~/agent.toml.new ~/agent.toml
fi

start() {  # unit, command...
    local unit=$1
    shift
    if systemctl --user is-active --quiet "$unit"; then return; fi
    systemctl --user reset-failed "$unit" 2> /dev/null || true
    systemd-run --user --quiet --unit="$unit" --setenv=DISPLAY=:1 \
        --setenv=MODAMA_LOG_LEVEL="${MODAMA_LOG_LEVEL:-debug}" "$@"
}

start modama-agent "$BIN/modama-agent" --config-path "$HOME/agent.toml"
for _ in $(seq 60); do
    curl -fs -m 2 -H 'X-API-Version: 1.0.0' http://localhost:3003/api/health/ > /dev/null && break
    sleep 1
done
curl -fsS -m 2 -H 'X-API-Version: 1.0.0' http://localhost:3003/api/health/; echo
# The session helper in a unit of its own, so its log ends up in the journal: the one the
# GUI starts writes to /dev/null on Linux, and leaves again since this one runs
start modama-helper "$BIN/modama-agent-gui" --agent-url http://localhost:3003/ session-helper
start modama-gui "$BIN/modama-agent-gui" --agent-url http://localhost:3003/
echo "agent, session helper and GUI started; logs:" \
    "journalctl --user -u modama-agent -u modama-helper -u modama-gui"
