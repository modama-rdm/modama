# TODO

- [ ] expose structured project/session metadata from the management API while
      keeping smbdrop project-agnostic (deferred until the new mgmtinterface API exists)
- [ ] Monitor critical background tasks and restart them internally or propagate their failure.

## smbdrop event channel

- [ ] no keepalive: without pings, a vanished peer stays listed until a send fails or TCP gives up

## Agent: local network token proxy

- [ ] tasks for vanished interface addresses never end: `token_publisher_loop`
      (`modama-agent/src/lib/local_net_token_proxy.rs`) starts an `advertisement_loop` per private
      address but never stops one when its address goes away (the comment says the tasks end by
      themselves; they retry socket creation every 5 s forever). Seen on Windows after sleep and
      network changes: 127 "error creating multicast socket … 10049" lines overnight for two
      vanished addresses. Abort the tasks (advertisement and token server) of addresses that are
      no longer listed.
- [ ] discovered token proxies are never forgotten (`RemoteAgents` in
      `local_net_token_provider.rs`): one on our networks that stopped announcing, or announced
      with a port from before its agent restarted, is tried on every token request.
      `report_failure` is never called (and would refresh the entry instead of demoting it);
      `success` does nothing. Drop proxies not heard from for a few announcement periods, and
      try recently failed ones last.
- [ ] a token proxy is asked if it is on any private network of this computer, not only on the
      network of the interface its announcement arrived on; a PC with its own 10.x network
      elsewhere (VPN) would still ask a 10.x proxy announced from another link. Remember the
      arrival interface per proxy (on Linux, one socket hears all interfaces: needs
      `IP_PKTINFO`).
- [ ] token requests try the proxies one after another, so each unreachable one before a working
      one costs up to the 2 s connect timeout; try them concurrently (happy eyeballs).

## GUI

- [ ] upgrade the egui stack to 0.36 once compatible releases of both
      `egui_taffy` and `egui-phosphor` are available
- [ ] test long project and ingest-share session names against backend limits
- [ ] general design fixes
    - [ ] Review vertical and horizontal flow: pixel peep a bit and check typographic baselines of horizontally aligned items, and horizontal guide lines where text starts (or stops at the right edge!!!!)

## Minimal HTML UI

- [ ] add hidden CSRF tokens to every state-changing minimal HTML form for legacy
      browsers that do not send `Origin` or `Sec-Fetch-Site`
