# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] re-examine polling for status updates
    - expose websocket from modama-agent and have a background thread continuously listen on that?
- [ ] handle stacked bind mounts when cleaning up stale smbdrop mounts
- [x] refactor `MountManager::set_state()` to reconcile bind mounts by managed mountpoint
      instead of using the incorrect bind mount source
- [x] continuously reconcile desired and mounted shares in modama-agent so failed mount and unmount
      operations retry without requiring another WebSocket state update
- [ ] avoid restoring stale GUI pending-start state when the foreground monitor
      observes the matching share before the start task completes
- [ ] goal: make modama agent plus the gui work well in the test-env
    - right now, after the integration tests have run, the GUI shows that the agent is continuously trying to
      mount some share. quite possibly fixed by state reconciliation in modama-agent?
    - reproduce: `./run.sh` on ptycho, forward 3003 to laptop, run modama-agent-gui

## GUI

- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow: pixel peep a bit and check typographic baselines of horizontally aligned items, and horizontal guide lines where text starts (or stops at the right edge!!!!)
- [ ] logged out: clicking on the "Close session" button "redirects" to create flow; need a "next page" kinda state which determines which action is emitted when the login succeeds
- [ ] workflow issues
    - [ ] if user A creates an ingest share at resource R1, and later user B uses the same resource, B must be able to act
