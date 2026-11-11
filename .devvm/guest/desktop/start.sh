#!/bin/bash
# Start the virtual desktop :1 as user units: Xvfb, openbox, x11vnc (VM localhost only),
# and a notification area. Run through `devvm desktop-up [panel|watcher|none]`:
#   panel    xfce4-panel at the top, with a notification area (default)
#   watcher  no panel; sni-watcher.py stands in for the notification area (tray.py reads it)
#   none     no notification area at all
# The panel has launchers for the GUI of REPO's release build ("modama-agent") and the file manager. The screen is
# DESKTOP_SIZE (default 1440x900, small enough that a screenshot needn't be scaled down to
# be looked at); a different size takes effect after `systemctl --user stop desktop-xvfb`.
# Idempotent; switching the mode stops the other one.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
MODE=${1:-panel}
case "$MODE" in
    panel|watcher|none) ;;
    *) echo "unknown mode '$MODE'; expected panel, watcher or none" >&2; exit 2 ;;
esac
export DISPLAY=:1
REPO=${REPO:-$HOME/modama}
SIZE=${DESKTOP_SIZE:-1440x900}

start() {  # unit, command...
    local unit=$1
    shift
    if systemctl --user is-active --quiet "$unit"; then return; fi
    systemctl --user reset-failed "$unit" 2> /dev/null || true
    systemd-run --user --quiet --unit="$unit" --setenv=DISPLAY=$DISPLAY "$@"
}

start desktop-xvfb Xvfb $DISPLAY -screen 0 "${SIZE}x24" -nolisten tcp
for _ in $(seq 50); do xdpyinfo > /dev/null 2>&1 && break; sleep 0.1; done
# D-Bus activated services (Thunar as FileManager1, the AT-SPI bus) need the display
systemctl --user set-environment DISPLAY=$DISPLAY
dbus-update-activation-environment --systemd DISPLAY

# Start the accessibility bus before any app. AccessKit (accesskit_unix 0.21) only exposes
# its tree while a screen reader is reported, and follows changes of that flag.
busctl --user call org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress > /dev/null
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status ScreenReaderEnabled b true

start desktop-openbox openbox
start desktop-vnc x11vnc -display $DISPLAY -localhost -forever -shared -nopw -quiet

# The panel's layout, which xfconfd reads when it starts, and the launcher's item. xfconfd
# writes the layout file itself, so a copy of what was installed tells whether it changed;
# replacing it restarts the panel, and with it what was started from the launcher.
panel_config=~/.config/xfce4/xfconf/xfce-perchannel-xml/xfce4-panel.xml
installed_layout=~/.config/xfce4/panel/modama-layout.xml
launcher=~/.config/xfce4/panel/launcher-4/modama.desktop
if [ "$MODE" = panel ]; then
    mkdir -p "$(dirname "$launcher")"
    sed -e "s#@BIN@#$REPO/modama-rs/target/release#" -e "s#@TOOLS@#$HERE#" \
        "$HERE/modama.desktop.in" > "$launcher"
    mkdir -p ~/.config/xfce4/panel/launcher-5
    cp /usr/share/applications/thunar.desktop ~/.config/xfce4/panel/launcher-5/
    if ! cmp -s "$HERE/xfce4-panel.xml" "$installed_layout"; then
        # one after the other: a panel that exits saves its settings, which starts xfconfd
        # again through D-Bus, and a panel started while xfconfd stops can't reach it
        systemctl --user stop desktop-panel 2> /dev/null || true
        systemctl --user stop xfconfd.service 2> /dev/null || true
        mkdir -p "$(dirname "$panel_config")"
        cp "$HERE/xfce4-panel.xml" "$panel_config"
        cp "$HERE/xfce4-panel.xml" "$installed_layout"
        systemctl --user start xfconfd.service
    fi
fi

case "$MODE" in
    panel)
        systemctl --user stop desktop-sni-watcher 2> /dev/null || true
        start desktop-panel xfce4-panel ;;
    watcher)
        systemctl --user stop desktop-panel 2> /dev/null || true
        start desktop-sni-watcher python3 "$HERE/sni-watcher.py" ;;
    none)
        systemctl --user stop desktop-panel desktop-sni-watcher 2> /dev/null || true ;;
esac
echo "desktop $DISPLAY is up, notification area: $MODE"
