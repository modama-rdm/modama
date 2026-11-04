# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] Monitor critical background tasks and restart them internally or propagate their failure.

## smbdrop event channel

- [ ] stale share list after a connection change: `broadcast_change` (`smbdrop/src/lib/events.rs`)
      sends `last_state.inner_state`, which a separate task updates only after a new state from the
      HTTP API went out on the same channel; a connect, disconnect or status report in between
      sends the old shares after the new ones, and agents keep them until the next change.
      Update `last_state` before broadcasting (and broadcast only a "changed" signal).
- [ ] a connection that falls more than 16 updates behind is closed: the broadcast channel holds
      16 states (since 059ff2e, 2024-01-12), and `RecvError::Lagged` ends the connection as
      `Disconnected` (since 477b4dd, 2025-09-08). Its receiver is subscribed at TCP accept, before
      the TLS handshake and login; a mass reconnect after an smbdrop restart can cascade. On lag,
      send the current state instead.
- [ ] no keepalive: without pings, a vanished peer stays listed until a send fails or TCP gives up

## Agent: local network token proxy

- [ ] tasks for vanished interface addresses never end: `token_publisher_loop`
      (`modama-agent/src/lib/local_net_token_proxy.rs`) starts an `advertisement_loop` per private
      address but never stops one when its address goes away (the comment says the tasks end by
      themselves; they retry socket creation every 5 s forever). Seen on Windows after sleep and
      network changes: 127 "error creating multicast socket … 10049" lines overnight for two
      vanished addresses. Abort the tasks (advertisement and token server) of addresses that are
      no longer listed.

## GUI

- [ ] upgrade the egui stack to 0.36 once compatible releases of both
      `egui_taffy` and `egui-phosphor` are available
- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow: pixel peep a bit and check typographic baselines of horizontally aligned items, and horizontal guide lines where text starts (or stops at the right edge!!!!)

## Minimal HTML UI

- [ ] add hidden CSRF tokens to every state-changing minimal HTML form for legacy
      browsers that do not send `Origin` or `Sec-Fetch-Site`
