#!/bin/bash
# Start the virtual desktop :1 as user units: Xvfb, x11vnc (VM localhost only), and one of
# three desktops on it. Run through `devvm desktop-up [MODE]`.
#
# An X11 desktop, openbox with:
#   panel        xfce4-panel at the top, with a notification area (default)
#   watcher      no panel; sni-watcher.py stands in for the notification area (tray.py reads it)
#   none         no notification area at all
# A Wayland desktop, GNOME Shell as a window that fills the screen :1 (so screenshots, VNC and
# xdotool work as on the X11 desktop), without Xwayland: programs can only use Wayland.
#   gnome        with the AppIndicator extension, which shows tray icons in the top bar
#   gnome-plain  GNOME as upstream ships it, without a notification area
# A Wayland desktop, KDE's KWin as a window that fills the screen :1 (as GNOME Shell above),
# without Xwayland:
#   plasma       with Plasma's panel: task manager and notification area
#   kwin         KWin alone, without a panel or notification area
#
# The panel has launchers for the GUI of REPO's release build ("modama-agent") and the file
# manager. The screen is DESKTOP_SIZE (default 1440x900, small enough that a screenshot needn't
# be scaled down to be looked at); a different size takes effect after
# `systemctl --user stop desktop-xvfb desktop-gnome desktop-kwin`.
# Idempotent; switching the mode stops the other one. Switching between X11, GNOME and KDE
# also stops the session helper and the GUI, which can't follow; `devvm desktop-modama` starts
# them on the new desktop.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
MODE=${1:-panel}
case "$MODE" in
    panel|watcher|none) KIND=x11 ;;
    gnome|gnome-plain) KIND=gnome ;;
    plasma|kwin) KIND=kde ;;
    *) echo "unknown mode '$MODE'; expected panel, watcher, none, gnome, gnome-plain, plasma or kwin" >&2
       exit 2 ;;
esac
export DISPLAY=:1
# also in start-modama.sh
WAYLAND_NAME=wayland-desktop
REPO=${REPO:-$HOME/modama}
SIZE=${DESKTOP_SIZE:-1440x900}

start() {  # unit, [--setenv=...]..., command...
    local unit=$1
    shift
    if systemctl --user is-active --quiet "$unit"; then return; fi
    systemctl --user reset-failed "$unit" 2> /dev/null || true
    systemd-run --user --quiet --unit="$unit" --setenv=DISPLAY=$DISPLAY "$@"
}

stop() {  # unit...
    systemctl --user stop "$@" 2> /dev/null || true
}

# Plasma's own units, which a Plasma session starts: the activity manager, kded6 (which serves
# the notification area's StatusNotifierWatcher) and plasmashell. They take their environment
# from the user's systemd manager.
PLASMA_UNITS=(plasma-plasmashell plasma-kded6 plasma-kactivitymanagerd)

# The variables a Plasma session sets. Without XDG_MENU_PREFIX, KWin doesn't find
# plasmashell's desktop file and refuses it the window list, which leaves the task manager
# empty.
KDE_ENV=(XDG_CURRENT_DESKTOP=KDE XDG_MENU_PREFIX=plasma-)

shell_eval() {  # JavaScript; needs gnome-shell --unsafe-mode
    gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
        --method org.gnome.Shell.Eval "$1"
}

# For programs started later, through D-Bus activation or by systemd-run: the window system
# (one variable only, so a program can't fall back to the other) and the KDE variables
wayland_environment() {
    systemctl --user unset-environment DISPLAY
    systemctl --user set-environment WAYLAND_DISPLAY="$WAYLAND_NAME"
    dbus-update-activation-environment --systemd WAYLAND_DISPLAY="$WAYLAND_NAME" DISPLAY=
}
kde_environment() {  # on | off
    if [ "$1" = on ]; then
        systemctl --user set-environment "${KDE_ENV[@]}"
        dbus-update-activation-environment --systemd "${KDE_ENV[@]}"
    else
        local names=("${KDE_ENV[@]%%=*}")
        # in this order: dbus-update-activation-environment sets them empty in systemd too
        dbus-update-activation-environment --systemd "${names[@]/%/=}"
        systemctl --user unset-environment "${names[@]}"
    fi
}

running=x11
if systemctl --user is-active --quiet desktop-gnome; then running=gnome; fi
if systemctl --user is-active --quiet desktop-kwin; then running=kde; fi
if [ "$running" != "$KIND" ]; then
    # windows of the other window system: the session helper's mount popup, the GUI and the
    # file manager, which would otherwise keep serving "open folder" where nobody sees it
    stop modama-helper modama-gui
    thunar --quit 2> /dev/null || true
fi

start desktop-xvfb Xvfb $DISPLAY -screen 0 "${SIZE}x24" -nolisten tcp
for _ in $(seq 50); do xdpyinfo > /dev/null 2>&1 && break; sleep 0.1; done

# Start the accessibility bus before any app. AccessKit (accesskit_unix 0.21) only exposes
# its tree while a screen reader is reported, and follows changes of that flag.
busctl --user call org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress > /dev/null
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status ScreenReaderEnabled b true

start desktop-vnc x11vnc -display $DISPLAY -localhost -forever -shared -nopw -quiet

if [ "$KIND" != kde ]; then
    # kded6 would keep the StatusNotifierWatcher's name, which the panel, sni-watcher.py and
    # GNOME's extension take
    stop "${PLASMA_UNITS[@]}" desktop-kwin
    # also one that D-Bus started outside its unit
    busctl --user call org.kde.kded6 /MainApplication org.qtproject.Qt.QCoreApplication quit \
        2> /dev/null || true
    kde_environment off
fi

if [ "$KIND" = x11 ]; then
    stop desktop-gnome
    # D-Bus activated services (Thunar as FileManager1) need the display
    systemctl --user unset-environment WAYLAND_DISPLAY
    systemctl --user set-environment DISPLAY=$DISPLAY
    dbus-update-activation-environment --systemd DISPLAY WAYLAND_DISPLAY=
    start desktop-openbox openbox
elif [ "$KIND" = gnome ]; then
    stop desktop-openbox desktop-panel desktop-sni-watcher
    # The extension is the notification area; GNOME Shell follows the setting while it runs.
    # No idle blanking or lock screen in the way of a screenshot.
    if [ "$MODE" = gnome ]; then extensions="['ubuntu-appindicators@ubuntu.com']"; else extensions="[]"; fi
    gsettings set org.gnome.shell enabled-extensions "$extensions"
    gsettings set org.gnome.desktop.session idle-delay 0
    gsettings set org.gnome.desktop.screensaver lock-enabled false
    # Nested, GNOME Shell takes its monitor's size from this variable. Without a window
    # manager on :1 its window sits at 0,0, so positions on the monitor are those on :1.
    # --unsafe-mode lets gnome.py ask the shell for its windows.
    start desktop-gnome --setenv=MUTTER_DEBUG_DUMMY_MODE_SPECS="$SIZE" \
        gnome-shell --nested --no-x11 --wayland-display="$WAYLAND_NAME" --unsafe-mode
    # GNOME Shell ends its start-up in the overview, which hides the windows
    for _ in $(seq 100); do
        shell_eval 'Main.layoutManager._startingUp' 2> /dev/null | grep -q false && break
        sleep 0.2
    done
    shell_eval 'Main.overview.hide()' > /dev/null
    wayland_environment
else
    stop desktop-openbox desktop-panel desktop-sni-watcher desktop-gnome
    if [ "$MODE" = kwin ]; then stop "${PLASMA_UNITS[@]}"; fi
    # Before KWin, which reads them too
    kde_environment on
    # Nested, KWin takes its window's size from the options; without a window manager on :1
    # the window sits at 0,0. No lock screen in the way of a screenshot.
    start desktop-kwin kwin_wayland --x11-display "$DISPLAY" --socket "$WAYLAND_NAME" \
        --width "${SIZE%x*}" --height "${SIZE#*x}" --no-lockscreen
    for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/$WAYLAND_NAME" ] && break; sleep 0.1; done
    wayland_environment
    if [ "$MODE" = plasma ]; then
        systemctl --user start plasma-kactivitymanagerd plasma-kded6
        # A Plasma session's start-up has kded6 load the watcher; nothing else asks for it
        busctl --user call org.kde.kded6 /kded org.kde.kded6 loadModule s statusnotifierwatcher \
            > /dev/null
        systemctl --user start plasma-plasmashell
    fi
fi

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
        stop desktop-panel
        stop xfconfd.service
        mkdir -p "$(dirname "$panel_config")"
        cp "$HERE/xfce4-panel.xml" "$panel_config"
        cp "$HERE/xfce4-panel.xml" "$installed_layout"
        systemctl --user start xfconfd.service
    fi
fi

case "$MODE" in
    panel)
        stop desktop-sni-watcher
        start desktop-panel xfce4-panel
        echo "desktop $DISPLAY is up (X11), notification area: xfce4-panel" ;;
    watcher)
        stop desktop-panel
        start desktop-sni-watcher python3 "$HERE/sni-watcher.py"
        echo "desktop $DISPLAY is up (X11), notification area: sni-watcher.py" ;;
    none)
        stop desktop-panel desktop-sni-watcher
        echo "desktop $DISPLAY is up (X11), no notification area" ;;
    gnome)
        echo "desktop $DISPLAY is up (GNOME on Wayland), notification area: AppIndicator extension" ;;
    gnome-plain)
        echo "desktop $DISPLAY is up (GNOME on Wayland), no notification area" ;;
    plasma)
        echo "desktop $DISPLAY is up (KDE Plasma on Wayland), notification area: Plasma's panel" ;;
    kwin)
        echo "desktop $DISPLAY is up (KWin on Wayland), no notification area" ;;
esac
if [ "$running" != "$KIND" ]; then
    echo "the session helper and the GUI were stopped; start them here with: devvm desktop-modama"
fi
