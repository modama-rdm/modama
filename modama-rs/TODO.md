# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] Monitor critical background tasks and restart them internally or propagate their failure.

## smbdrop event channel

- [ ] no keepalive: without pings, a vanished peer stays listed until a send fails or TCP gives up

## GUI

- [ ] upgrade the egui stack to 0.36 once compatible releases of both
      `egui_taffy` and `egui-phosphor` are available
- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow: pixel peep a bit and check typographic baselines of horizontally aligned items, and horizontal guide lines where text starts (or stops at the right edge!!!!)

## Minimal HTML UI

- [ ] add hidden CSRF tokens to every state-changing minimal HTML form for legacy
      browsers that do not send `Origin` or `Sec-Fetch-Site`
