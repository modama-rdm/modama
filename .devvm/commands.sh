# Project commands for devvm (sourced by devvm; see its README, "Project configuration").
#
# A virtual desktop in the VM for trying modama-agent-gui, its session helper and mount
# popup by hand or from scripts: Xvfb with openbox and xfce4-panel, widgets driven by
# name over AT-SPI (ui.py), the tray icon over D-Bus (tray.py), screenshots to the host.
# The tools live in .devvm/guest/desktop/ and are copied into the VM by every command.

DESKTOP_TOOLS_VM=.local/share/modama-desktop   # in the VM user's home

PROJECT_USAGE+="  desktop-setup     install the virtual desktop's packages and Rust in the VM (once)
  desktop-up [panel|watcher|none]
                    start the desktop :1 with a notification area: xfce4-panel (default),
                    a D-Bus stand-in for tray.py, or none
  desktop-modama    enroll an agent with the test deployment's CA (once) and start
                    modama-agent and modama-agent-gui on the desktop; needs deploy/uptest.sh
                    and a release build in the VM's checkout
  desktop-shot [FILE]
                    save a screenshot of the desktop on the host (default:
                    \${TMPDIR:-/tmp}/devvm-desktop.png)
  desktop-vnc       forward the desktop's VNC server to localhost:5900 on the host
  ui ARGS...        drive widgets by name over AT-SPI (ui.py; devvm ui --help)
  tray ARGS...      read and click the tray icon over D-Bus (tray.py; devvm tray --help)
  xdo ARGS...       xdotool on the desktop, e.g. devvm xdo mousemove 100 200 click 1
  desktop-drag X1 Y1 X2 Y2
                    drag with the left button from X1,Y1 to X2,Y2 in steps, as a hand does"

# Copy .devvm/guest/desktop/ to the VM, replacing the previous copy
desktop_copy_tools() {
    tar -C "$PROJECT_DIR/guest/desktop" -cf - . |
        vm_ssh "rm -rf ~/$DESKTOP_TOOLS_VM && mkdir -p ~/$DESKTOP_TOOLS_VM && tar -C ~/$DESKTOP_TOOLS_VM -xf -"
}

# Run a tool of the copy in the VM, with the arguments quoted for the remote shell
desktop_tool() {
    local tool=$1
    shift
    vm_ssh "DISPLAY=:1 REPO=~/$DEVVM_VM_REPO ~/$DESKTOP_TOOLS_VM/$tool $(printf '%q ' "$@")"
}

project_cmd_desktop_setup() {
    desktop_copy_tools
    desktop_tool setup.sh
}

project_cmd_desktop_up() {
    desktop_copy_tools
    desktop_tool start.sh "$@"
}

project_cmd_desktop_modama() {
    desktop_copy_tools
    desktop_tool start-modama.sh
}

project_cmd_desktop_shot() {
    local out=${1:-${TMPDIR:-/tmp}/devvm-desktop.png}
    # into a temporary file first, so a failure leaves no empty or truncated FILE behind; in a
    # subshell, since devvm exits right away when there is no VM
    if ! (vm_ssh "DISPLAY=:1 import -window root png:-") > "$out.part"; then
        rm -f "$out.part"
        return 1
    fi
    mv "$out.part" "$out"
    echo "$out"
}

project_cmd_desktop_vnc() {
    echo "VNC of $DEVVM_VM_NAME's desktop on localhost:5900 (no password); Ctrl-C ends it."
    echo "To watch, e.g. with TightVNC (no raising on a beep, no raw encoding on localhost):"
    echo '  xvncviewer -noraiseonbeep -encodings "copyrect tight hextile zlib corre rre" -compresslevel 9 localhost:5900'
    echo "Add -viewonly to only watch a session that runs without you: no mouse or keyboard input."
    vm_ssh -N -L 5900:localhost:5900
}

project_cmd_ui() {
    desktop_tool ui.py "$@"
}

project_cmd_tray() {
    desktop_tool tray.py "$@"
}

project_cmd_xdo() {
    vm_ssh "DISPLAY=:1 xdotool $(printf '%q ' "$@")"
}

# GTK starts a drag only after the pointer has moved a little with the button down, and drops
# only where it saw it arrive: one jump from start to end does neither
project_cmd_desktop_drag() {
    if [ $# -ne 4 ]; then
        echo "usage: devvm desktop-drag X1 Y1 X2 Y2" >&2
        return 2
    fi
    local x1=$1 y1=$2 x2=$3 y2=$4
    vm_ssh "export DISPLAY=:1
        xdotool mousemove $((x1)) $((y1)) && sleep 0.3 && xdotool mousedown 1 && sleep 0.3
        for i in 1 2 3 4 5 6 7 8 9 10; do
            xdotool mousemove \$(( $((x1)) + ($((x2)) - $((x1))) * i / 10 )) \$(( $((y1)) + ($((y2)) - $((y1))) * i / 10 ))
            sleep 0.1
        done
        sleep 0.4 && xdotool mouseup 1"
}
