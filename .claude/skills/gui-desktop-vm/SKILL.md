---
name: gui-desktop-vm
description: Run modama-agent-gui, its session helper, the mount popup and the tray icon on a virtual Linux desktop in a devvm VM, against the test deployment, and drive them from the command line (widgets by name over AT-SPI, the tray icon over D-Bus, xdotool, screenshots).
when_to_use: Trying a GUI or session helper change in the running program rather than in the visual tests; running a scenario of the platform checklist on Linux (mounting, mount popup, tray icon, hand-off); reproducing a bug that needs a real desktop, GVfs and the backend.
---

# The agent GUI on a desktop in a VM

`.devvm/commands.sh` adds `devvm desktop-*`, `ui`, `tray` and `xdo` (see `devvm` without
arguments). The tools behind them are in `.devvm/guest/desktop/`; every `desktop-*` command
copies them into the VM, so changes to them need no commit.

What this is not for: layout and look of single pages are checked by the visual tests
(`modama-rs/AGENTS.md`); automated tests across the components run in `modama-rs/test-env/`.

## Setting up a VM (once)

The test deployment needs a VM that doesn't share the checkout (`chattr` and ACLs don't work
on a shared folder):

```bash
DEVVM_SHARE_WORKSPACE=0 devvm create      # or reuse one: devvm ls -a, then devvm use VM
devvm push                                # commits only; the VM's checkout is ~/modama
devvm desktop-setup                       # Xvfb, openbox, xfce4-panel, AT-SPI, GVfs, Rust
devvm ssh 'deploy/uptest.sh --build'      # the test deployment; the first build takes long
```

Run the long commands in the background with a time limit, and look at the containers
(`devvm ssh 'cd deploy && docker compose ps -a'`) before saying they are still running.

## Each time

```bash
devvm push                                # after committing the change
devvm ssh 'cd modama-rs && ~/.cargo/bin/cargo build --release -p modama-agent -p modama-agent-gui'
devvm desktop-up                          # desktop :1 with xfce4-panel; idempotent
devvm desktop-modama                      # agent, session helper and GUI
```

`desktop-modama` enrolls the agent `desktop` for the resource `desktop-lab` once (config in
`~/agent.toml` in the VM) and leaves running processes alone. After a rebuild, restart them:
`devvm ssh 'systemctl --user restart modama-agent modama-helper modama-gui'`. The units are
transient: one that was stopped is gone, and `devvm desktop-modama` starts it again. Logs:
`devvm ssh 'journalctl --user -u modama-agent -u modama-helper -u modama-gui'`.

The session helper's tray icon shows in xfce4-panel's notification area and can be used there
as on a real desktop. `devvm desktop-up watcher` replaces the panel by a D-Bus stand-in for
the notification area, `devvm desktop-up none` leaves it out (a desktop without one).

## Test data

Users `alice`, `bob` and `carol`, password `testpassword` (`docs/guide/src/development.md`,
"Users and logins"). The test deployment starts without projects; create a project folder
owned by its owner and the group `iff`:

```bash
devvm ssh 'cd deploy && docker compose exec -T permissions bash -c \
  "mkdir -p /data/adhoc/aliceproject && chown alice:iff /data/adhoc/aliceproject"'
```

## Driving it

- `devvm ui …` finds widgets by their accessible name: `apps` (processes with windows),
  `tree`, `text`, `click NAME`, `focus NAME`, `wait NAME`. Names match exactly, as `tree`
  prints them; `--role` narrows, e.g. `--role "password text"`. If a name is in more than one
  window, `click` refuses and lists them; choose with `--pid PID` or `--newest`. `click` runs
  the widget's action, it is not a pointer event.
- `devvm tray …` reads and drives the session helper's tray icon over D-Bus, whichever
  notification area runs: `show`, `menu`, `click LABEL`, `activate` (a left click),
  `wait-tooltip TEXT`, `wait-item`, `wait-gone`, `watch` (prints each change of the tooltip
  with the time), `icon DIR`. `--timeout` goes before the command: `devvm tray --timeout 60
  watch`.
- `devvm xdo …` is xdotool on the desktop: typing, and what needs a real pointer (hover, the
  panel's menu). Coordinates are those `ui tree` prints; `devvm ui panel-icons` gives the
  tray icons' positions, which the panel doesn't name.
- `devvm desktop-shot FILE` saves a screenshot on the host. Look at it before judging how
  something looks; text and positions are better checked with `ui tree`.
- `devvm desktop-vnc` lets a person watch: VNC on `localhost:5900`. Offer the user this viewer
  command (TightVNC's `xvncviewer`), which keeps a beep from raising the viewer and taking the
  focus (typing there would disturb what the agent drives), and leaves out the raw encoding,
  which the viewer would otherwise pick for a connection to localhost:

  ```bash
  xvncviewer -noraiseonbeep -encodings "copyrect tight hextile zlib corre rre" -compresslevel 9 localhost:5900
  ```

  To only watch a session the agent drives on its own, add `-viewonly`: the viewer then sends
  no mouse or keyboard input. Leave it out when the user wants to try things on the desktop
  themselves.

Signing in and starting an ingest share, with the GUI's labels at the time of writing
(`devvm ui tree` shows the current ones):

```bash
devvm ui click "Log in" && devvm ui wait Username
devvm ui focus Username && devvm xdo type alice
devvm ui focus Password --role "password text" && devvm xdo type testpassword
devvm ui click "Sign in" && devvm ui wait "Start Ingest"
devvm ui click "Start Ingest"               # then the project, "Select Project",
devvm ui click "Create a new session"       # type a name after the date, "Create Session"
devvm ui wait Dismiss --timeout 60          # the mount popup
devvm ssh 'gio mount -l'                    # the GVfs mount of the ingest share
```

## A tester at the desktop

For a hallway test, a fresh agent tries the GUI as a user would, knowing only what a user
knows, such as the user guide. `.devvm/desk` gives it the instrument computer's screen, mouse
and keyboard and nothing else: screenshots, clicks, typing, keys and the screen reader's view
(`desk ui`), but no `devvm ssh`, logs or containers. Put a symlink to it into the tester's
folder, next to what the tester may read:

```bash
ln -s "$PWD/.devvm/desk" TESTER/desk     # acts on this checkout's VM from any folder
```

Prepare the desktop first (`desktop-up`, `desktop-modama`, test data). For a tester who
starts the GUI on their own, stop the one `desktop-modama` started:
`devvm ssh 'systemctl --user stop modama-gui'`; the session helper keeps running, with its
tray icon. The panel's launcher only reacts to a real click
(`desk click`), not to `ui click` or Enter.

## Pitfalls

- AccessKit only exposes the GUI's widgets while `org.a11y.Status ScreenReaderEnabled` is
  true on the session bus; `desktop-up` sets it. If `ui apps` doesn't list
  `modama-agent-gui`, check that flag and that the GUI runs (`systemctl --user status
  modama-gui`).
- Widget names can contain changing parts, such as dates or counts; read them from `ui tree`
  instead of writing them from memory.
- `xdotool search` matches windows that meet *any* of its conditions unless given `--all`:
  `search --pid P --name .` finds every titled window. Close a window as a user would,
  `devvm xdo windowactivate --sync W key alt+F4`, not with `windowclose`, which destroys the
  window instead of asking the program to close it.
- The GUI renders with software Vulkan (llvmpipe); it is slower than on real hardware.
