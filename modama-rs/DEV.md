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
