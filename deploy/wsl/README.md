# Test stack in WSL2, for agents on the Windows host

The [test deployment](../../docs/guide/src/development.md#test-deployment) running in WSL2,
with `modama-agent`, `modama-agent-gui` and the session helper running on the Windows host.
This allows testing the Windows side (WNet drive mappings, the tray icon, hand-offs) on a
single Windows machine, with the test directory's users `alice`, `bob` and `carol`.

## Why WSL, and why a separate compose file

The Windows SMB client only connects to port 445. The test deployment publishes SMB on
`127.0.0.1:1446`, which is fine for Linux (GVfs takes a port) but not for Windows, and the
Windows SMB server already holds port 445 on the host's own addresses. WSL2 with NAT
networking has an address of its own with a free port 445, so Windows can map
`\\<wsl-address>\<share>`.

`docker-compose.wsl.yml` replaces (`!override`, Docker Compose 2.24 or later) the port lists
of `docker-compose.override.yml`:

| Service | Port | Published on |
| --- | --- | --- |
| smbdrop | 445 (SMB) | all WSL addresses |
| smbdrop | 5052 (events, wss), 5053 (token API, https) | all WSL addresses |
| smbdrop | 1446 (SMB), 5051 (unauthenticated API) | 127.0.0.1 only, as before |
| mgmtinterface-proxy | 5001 (management API) | all WSL addresses |

The other ports stay as they are, on `127.0.0.1`. With NAT networking, only the Windows host
can reach the WSL addresses. Anywhere else these ports would be open to the network, so
`run.sh` refuses to run outside WSL2, and also in mirrored networking mode, which can't work
anyway because Windows holds port 445.

## Running it

`run.sh <command>` runs the command with `COMPOSE_FILE` set to the two files of the test
deployment plus `docker-compose.wsl.yml`. The scripts of the test deployment and any
`docker compose` call pick that up:

```bash
deploy/wsl/run.sh deploy/uptest.sh          # up, wait, apply the Nextcloud configuration
deploy/wsl/run.sh deploy/runtest.sh         # check it
deploy/wsl/run.sh docker compose logs -f smbdrop
deploy/wsl/run.sh deploy/downtest.sh        # stop; --reset deletes the state
```

Without `run.sh`, `uptest.sh` brings the stack up with the ports on `127.0.0.1` only, which
the Windows host can't reach. Commands that don't create containers (`ps`, `logs`, `exec`,
`down`) work either way, since the project is the same.

The users are the ones of the test deployment: `alice`, `bob` and `carol` in the group
`iff`, and `mallory` outside it, all with the password `testpassword` (see "Users and
logins" in the development guide).

## On Windows

1. Find the WSL address (`hostname -I` in WSL) and add a hosts entry `smbdrop` for it in
   `C:\Windows\System32\drivers\etc\hosts`, for example `172.22.188.187 smbdrop`. The
   address can change when WSL restarts; update the entry then.

   The name is `smbdrop` because that's the name in the certificate that the test
   deployment's `testenv-init` issues for the token and events APIs; with any other name,
   the agent would reject the certificate.
2. Enroll an agent with the test CA, in `testconf/secrets/CA/`, and generate its
   configuration with every URL on `smbdrop`. The `ca` tool is in the `testenv-init` image:

   ```bash
   ca() { deploy/wsl/run.sh docker compose run --rm -T --entrypoint ca testenv-init \
       --ca-path /ca --insecure-no-password "$@"; }
   ca enroll-agent <agent-name> <resource-name>
   ca generate-agent-config --event-url wss://smbdrop:5052 \
       --token-service-url https://smbdrop:5053 \
       --mgmt-api-url http://smbdrop:5001 --smb-hostname smbdrop \
       --agent-name <agent-name> --resource-name <resource-name> > agent.toml
   ```

   The configuration contains the CA certificate and the agent's certificate and key; leave
   out `--agent-name` and `--resource-name` for an agent without a certificate (see
   `docs/guide/src/agent.md`).

3. Run the agent and the GUI on Windows with that configuration, and log in as `alice`.

The Windows checks to run against this stack are in the "Session helper platform tests"
checklist.
