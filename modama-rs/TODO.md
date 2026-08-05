# TODO

- [x] complete versioned local API adoption
    - [x] Update modama-agent endpoints and schemas
    - [x] Auto-generate a Rust client
    - [x] Have the GUI use the generated client
    - [x] Add deterministic API service fixtures for GUI tests
- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic
- [x] review retained versioned tests for overlap after omitting exploratory tests
- [x] clean up the versioned builder API: reconcile naming, state ownership,
      fallible validation, and metadata-required route registration into one
      coherent public interface
- [ ] BUG: while connecting to the agent, going to the overview or to the login page clears the "connecting..." state
    - the overview state needs an indication of the current connection status
    - when not connected, maybe display "broken plug" kinda icon in top bar
    - disable login button when not connected
- [ ] re-examine polling for status updates
    - expose websocket from modama-agent and have a background thread continuously listen on that?
- [ ] need to also get updates when we are not waiting for an ingest share to mount
    - someone else might create or delete an ingest share from another authenticated agent

## GUI

- [ ] review design-token usage
    - [ ] Check token naming and component-specific usage
    - [ ] Replace hardcoded pixel/point sizes where appropriate
    - [ ] Check disabled-element color consistency
- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow
    - [ ] Fix hovered card rows overlaying the top border
