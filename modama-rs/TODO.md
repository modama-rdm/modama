# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] re-examine polling for status updates
    - expose websocket from modama-agent and have a background thread continuously listen on that?
- [ ] continuously reconcile desired and mounted shares so failed mount and unmount
      operations retry without requiring another WebSocket state update
- [ ] avoid restoring stale GUI pending-start state when the foreground monitor
      observes the matching share before the start task completes

## GUI

- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow: pixel peep a bit and check typographic baselines of horizontally aligned items, and horizontal guide lines where text starts (or stops at the right edge!!!!)
- [ ] logged out: clicking on the "Close session" button "redirects" to create flow; need a "next page" kinda state which determines which action is emitted when the login succeeds
