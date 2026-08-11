# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] re-examine polling for status updates
    - expose websocket from modama-agent and have a background thread continuously listen on that?
- [x] handle stacked bind mounts when cleaning up stale smbdrop mounts
- [x] refactor `MountManager::set_state()` to reconcile bind mounts by managed mountpoint
      instead of using the incorrect bind mount source
- [x] associate failed smbdrop mounts with shares by managed mountpoint/share ID rather than
      `dest_path`; multiple shares can export the same destination, so one failed mount currently
      removes healthy shares for that destination from the generated Samba configuration
- [x] continuously reconcile desired and mounted shares in modama-agent so failed mount and unmount
      operations retry without requiring another WebSocket state update
- [x] avoid restoring stale GUI pending-start state when the foreground monitor
      observes the matching share before the start task completes
- [ ] goal: make modama agent plus the gui work well in the test-env
    - right now, after the integration tests have run, the GUI shows that the agent is continuously trying to
      mount some share. quite possibly fixed by state reconciliation in modama-agent?
    - reproduce: `./run.sh` on ptycho, forward 3003 to laptop, run modama-agent-gui
- [x] smbdrop readiness checks must explicitly close the connection (!)
    - currently we are leaking FDs/sockets like a sieve

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
- [ ] show a lil indicator/spinner in overview or top bar if last foreground share polling failed

## Minimal HTML UI

- [x] allow creating a new session before opening its ingest share
- [ ] replace Basic authentication for the minimal HTML UI with cookie-backed sessions and a reliable logout action
- [ ] add hidden CSRF tokens to every state-changing minimal HTML form for legacy
      browsers that do not send `Origin` or `Sec-Fetch-Site`
