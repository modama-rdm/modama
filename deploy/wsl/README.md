# Test stack in WSL2, for agents on the Windows host

The test deployment (`docker-compose.yml` + `docker-compose.override.yml`) running in WSL2,
with `modama-agent`, `modama-agent-gui` and the session helper running on the Windows host.
This allows testing the Windows side (WNet drive mappings, the tray icon, hand-offs) on a
single Windows machine.

## Why WSL, and why a separate compose file

The Windows SMB client only connects to port 445. The test override publishes SMB on
`127.0.0.1:1446`, which is fine for Linux (GVfs takes a port) but not for Windows, and the
Windows SMB server already holds port 445 on the host's own addresses. WSL2 with NAT
networking has an address of its own with a free port 445, so Windows can map
`\\<wsl-address>\<share>`.

`docker-compose.wsl.yml` replaces (`!override`, Docker Compose 2.24 or later) the port lists
of the override file:

| Service | Port | Published on |
| --- | --- | --- |
| smbdrop | 445 (SMB) | all WSL addresses |
| smbdrop | 5052 (events, wss), 5053 (token API, https) | all WSL addresses |
| smbdrop | 1446 (SMB), 5051 (unauthenticated API) | 127.0.0.1 only, as before |
| mgmtinterface-proxy | 5001 (management API) | all WSL addresses |

With NAT networking, only the Windows host can reach the WSL addresses. Anywhere else these
ports would be open to the network, so `compose.sh` refuses to run outside WSL2, and also
in mirrored networking mode, which can't work anyway because Windows holds port 445.

## Running it

All scripts work from any directory; `compose.sh` passes its arguments to `docker compose`
with `deploy/` as the project directory, so `.env` and the bind mounts resolve as for the
other compose files.

```bash
deploy/wsl/up.sh                        # up -d, then follow the logs
deploy/wsl/compose.sh ps                # anything else docker compose does
deploy/wsl/testuser.sh alice secret 10001
deploy/wsl/testuser.sh bob secret 10002
```

sssd has no LDAP in WSL. `testuser.sh <user> <password> [uid]` creates a local user (group
`iff`, `ADHOC_GROUP` in `../permissions/config.py`) in the `permissions` container (login)
and the `webdav-backend` container (switching to the user's uid). Containers lose these
users when they are recreated; run it again then. The password ends up on the
`docker exec` command line, so use throwaway test passwords only.

## On Windows

1. Find the WSL address (`hostname -I` in WSL) and add a hosts entry for it in
   `C:\Windows\System32\drivers\etc\hosts`, for example `172.22.188.187 modama-wsl`. The
   address can change when WSL restarts; update the entry then.
2. The server certificate for the token and events APIs must name `modama-wsl`: enroll it
   with that name as an extra subject alternative name (see `docs/guide/src/agent.md`,
   `enroll-server`).
3. Generate the agent configuration with every URL on `modama-wsl`:

   ```bash
   ca generate-agent-config --event-url wss://modama-wsl:5052 \
       --token-service-url https://modama-wsl:5053 \
       --mgmt-api-url http://modama-wsl:5001 --smb-hostname modama-wsl
   ```

   plus `--ca-path`, and `--agent-name`/`--resource-name` for a certified agent, as for any
   other agent.

4. Run the agent and the GUI on Windows with that configuration.

The Windows checks to run against this stack are in the "Session helper platform tests"
checklist.
