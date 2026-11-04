# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] re-examine polling for status updates
    - expose websocket from modama-agent and have a background thread continuously listen on that?
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

## GUI

- [ ] upgrade the egui stack to 0.36 once compatible releases of both
      `egui_taffy` and `egui-phosphor` are available
- [ ] workflow issues
    - [ ] if user A creates an ingest share at resource R1, and later user B uses the same resource, B must be able to act
        - this requires non-localized changes to the permission system probably?
        - must only be allowed to act if the resource of the ingest matches
        - maybe we need to have a "detach from resource" kind of API call, where a running ingest can be detached from the resource (the ingest itself will continue to be available, but not auto-mounted on any resource; current mounts can continue to exist etc.)
- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow: pixel peep a bit and check typographic baselines of horizontally aligned items, and horizontal guide lines where text starts (or stops at the right edge!!!!)

## Minimal HTML UI

- [ ] replace Basic authentication for the minimal HTML UI with cookie-backed sessions and a reliable logout action
- [ ] add hidden CSRF tokens to every state-changing minimal HTML form for legacy
      browsers that do not send `Origin` or `Sec-Fetch-Site`

## Session helper

- [ ] modama-agent: add internal API: websocket endpoint
    - basically mirrors the events we get from smbdrop, in addition to our own full-state-on-connect message
