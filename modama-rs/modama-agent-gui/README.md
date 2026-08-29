# modama-agent-gui

The GUI connects to the local `modama-agent` HTTP API. It can be optionally
configured via a `.toml` file:

```toml
agent_url = "http://localhost:3003"
```

Configuration is resolved from `--config-path`, the per-user configuration
directory for `modama/modama-agent-gui`, the platform machine-wide
configuration directories, and finally the built-in default above. The
`--agent-url` argument overrides the resolved value. Missing discovered files
are ignored, while malformed existing files stop startup.
