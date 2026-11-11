# How to...

## Run tests with coverage

Run coverage separately for each component:

```sh
just coverage modama-agent
just coverage modama-agent-gui
just coverage smbdrop
```

The equivalent aliases are `just coverage-modama-agent`,
`just coverage-modama-agent-gui`, and `just coverage-smbdrop`.

Reports are written to `target/llvm-cov/<component>/`:

- `summary.txt` contains the human-readable summary and missing line numbers.
- `html/index.html` contains annotated source for browsing.
- `coverage.json` is the full machine-readable LLVM export, including line and
  source-region coverage. Source regions are the closest available granularity
  to expression coverage. The text summary is intended for humans and should
  not be parsed by tools.

## Run and update visual tests

Check the `justfile`:

- `just visual-tests`      # runs visual tests and compares to snapshots
- `just update-snapshots`  # updates snapshots and compresses PNGs via oxipng
- `just clean`             # remove .old.png .new.png .diff.png snapshot files

In a pull request, CI comments with up to five failing snapshots per suite
(difference, accepted and new image). The artifact linked in the comment holds
all of them. Changes to committed snapshots show up in the PR's "Files changed"
tab. `.github/scripts/snapshot_review.py` describes how a test suite takes part.

## Run the integration tests

`test-env/` runs smbdrop, three agents, a session helper, the management API
(mgmtinterface-api) and the permissions service in containers, and the pytest
suite in `test-env/tests/` against them. They are not part of CI.

- `just integration-tests`          # build, start and test; arguments go to pytest
- `just integration-tests -k token` # only some tests
- `just integration-tests-cleanup`  # remove containers, test CA and test data

After pytest, `run.sh` runs the agent GUI's end-to-end tests,
`modama-agent-gui/tests/e2e_*.rs` (feature `e2e`), in the service `gui-e2e`. They
drive the GUI with egui_kittest against agent1, without rendering, as the user of
`session-1`, whose session bus and `XDG_RUNTIME_DIR` they share: `gio` there sees
the session helper's GVfs mounts. To run them again against the running
containers, with an optional test name filter:
`docker compose --profile e2e run --rm gui-e2e [FILTER]` (or `podman compose`) in
`test-env/`. They build in `test-env/target/gui-e2e/`, apart from the binary
`session-1` runs.

It uses docker compose if installed, podman compose otherwise; set
`MODAMA_COMPOSE=docker` or `MODAMA_COMPOSE=podman` to choose. Rootless podman
can't run the tests that create shares: smbdrop marks a session folder
immutable (`chattr +i`) while it bind-mounts it, and only root on the host may
do that, so those tests fail with `EPERM`. Use docker or rootful podman for
them.

The containers compile the workspace on first start, which takes several
minutes; the build cache is kept in `test-env/target/`.

The test CA and the agent configurations in `test-env/certs/` are created only
if that folder is missing. After changing them in `test-env/prepare.sh`, run
`just integration-tests-cleanup` first.

If a run hangs while starting the containers, a service has most likely crashed
and compose is waiting for it to become healthy. Look at
`podman ps -a` (or `docker ps -a`) and the logs of the service that exited.

## Linting and formatting

- `just fmt`               # run the correct `cargo fmt` command on the whole workspace
- `just clippy`            # run clippy

## Run the GUI with egui-mcp

The GUI can expose egui's inspection protocol for `egui-mcp` through the
`inspect-gui` recipe. It runs under Xvfb, so it also works in a headless
environment:

```sh
just inspect-gui
```

To make the installed `egui-mcp` server available to Codex, add it to
`~/.codex/config.toml`:

```toml
[mcp_servers.egui]
command = "egui-mcp"
args = []
```

Restart Codex, or reconnect the MCP server, after changing this configuration.
With the GUI running, ask Codex to call the `attach` tool. It uses the default
inspection endpoint `127.0.0.1:5719`; after attaching, Codex can inspect and
drive the widget tree.

Keep inspection on the loopback address unless the port is intentionally
secured: the inspection protocol grants control over the running application.
