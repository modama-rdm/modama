# TODO

- [ ] complete versioned local API adoption
    - [ ] Update modama-agent endpoints and schemas
    - [ ] Auto-generate a Rust client
    - [ ] Have the GUI use the generated client
    - [ ] Add API mocks for GUI tests
- [ ] review retained versioned tests for overlap after omitting exploratory tests
- [ ] clean up the versioned builder API: reconcile naming, state ownership,
      fallible validation, and metadata-required route registration into one
      coherent public interface

## GUI

- [ ] review design-token usage
    - [ ] Check token naming and component-specific usage
    - [ ] Replace hardcoded pixel/point sizes where appropriate
    - [ ] Check disabled-element color consistency
- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow
    - [ ] Fix hovered card rows overlaying the top border

