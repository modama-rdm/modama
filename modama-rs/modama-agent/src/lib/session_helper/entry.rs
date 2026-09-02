// startup sequence:
//
// - check singleton "lock"/"mutex" and exit if another session helper is running
// - if available, start a notification tray icon (and the event listeners for handling startup
//   races in case the task bar starts after us, and restarts of the task bar)
//   - if not possible, shrug and continue
// - connect to the local internal websocket API
// - start reconciliation loop
//
//
// implementation TODO:
//
// - [ ] mount reconciliation
// - [ ] singleton
// - [ ] notification tray icon



