# How to...

## Run tests with coverage

`cargo llvm-cov --html` - then check `target/llvm-cov/html/index.html`

## Run and update visual tests

Check the `justfile`:

- `just visual-tests`      # runs visual tests and compares to snapshots
- `just update-snapshots`  # updates snapshots and compresses PNGs via oxipng
- `just clean`             # remove .old.png .new.png .diff.png snapshot files
- `just fmt`               # run the correct `cargo fmt` command on the whole workspace

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
