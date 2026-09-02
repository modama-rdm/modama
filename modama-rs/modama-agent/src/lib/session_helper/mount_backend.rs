// - mount backend here similar to the one we already have in modama-agent for the main process
// - mount backend might need to actually run in the background _on its own thread_, for GIO to work
// - ... so we keep an instance of the mount backend around, initialize it once, then interact with
//   that instance (under the hood, it might then spawn a thread, send messages, clean up that
//   thread on shutdown)
