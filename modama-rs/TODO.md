# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] re-examine polling for status updates
    - expose websocket from modama-agent and have a background thread continuously listen on that?

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
