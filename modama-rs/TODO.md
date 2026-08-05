# TODO

- [x] complete versioned local API adoption
    - [x] Update modama-agent endpoints and schemas
    - [x] Auto-generate a Rust client
    - [x] Have the GUI use the generated client
    - [x] Add deterministic API service fixtures for GUI tests
- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [x] review retained versioned tests for overlap after omitting exploratory tests
- [x] clean up the versioned builder API: reconcile naming, state ownership,
      fallible validation, and metadata-required route registration into one
      coherent public interface
- [x] BUG: keep the connecting state active and automatically retry until the agent is available
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
    - [x] Fix hovered card rows overlaying the top border
- [x] set up logging in the main entry point
- [ ] ensure important errors are logged (any of the errors that result in a "dismiss"/"retry" dialog for example)
- [x] BUG: scrollbar color is not distinguishable from background
- [x] extra binary for generating screnshots for the user documentation
- [ ] port the busy indicator from the react UI - the lil spinner is too subtle IMHO
