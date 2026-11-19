#!/usr/bin/python3
"""Ask KWin about its windows, on the KDE desktop (`devvm desktop-up kwin` or `plasma`).

    kwin.py windows            the windows from bottom to top: PID, wayland or x11,
                               position and size of the frame, "focus", title
    kwin.py raise TITLE        bring the window titled TITLE to the front and focus it
    kwin.py eval JAVASCRIPT    run JAVASCRIPT as a KWin script; it answers with
                               result(VALUE), which is printed as JSON

A program on Wayland doesn't know where its window is, so `ui.py tree` prints positions
relative to the window's content there. `windows` gives the frame, title bar included.

KWin runs scripts given over D-Bus but sends nothing back; a script here answers by calling
a D-Bus method of this program (callDBus).
"""

import argparse
import json
import os
import sys
import tempfile

from gi.repository import Gio, GLib

KWIN = "org.kde.KWin"
REPLY_PATH = "/KwinPy"
REPLY_IFACE = "local.KwinPy"
REPLY_XML = f"""
<node>
  <interface name="{REPLY_IFACE}">
    <method name="Result"><arg type="s" name="json" direction="in"/></method>
  </interface>
</node>
"""
TIMEOUT_MS = 5000

# workspace.stackingOrder is bottom to top; frameGeometry includes the title bar
WINDOWS = """
result(workspace.stackingOrder.filter(w => w.normalWindow || w.dialog).map(w => ({
    pid: w.pid, wayland: !w.x11, x: w.frameGeometry.x, y: w.frameGeometry.y,
    width: w.frameGeometry.width, height: w.frameGeometry.height,
    focus: w === workspace.activeWindow, title: w.caption})));
"""

RAISE = """
const found = workspace.stackingOrder.filter(w => w.caption === %s);
found.forEach(w => { workspace.activeWindow = w; });
result(found.length);
"""


def run_script(body):
    """The value the script `body` passes to result(), or exit with what went wrong"""
    bus = Gio.bus_get_sync(Gio.BusType.SESSION)
    answers = []
    loop = GLib.MainLoop()

    def on_call(_conn, _sender, _path, _iface, _method, params, invocation):
        answers.append(params.unpack()[0])
        invocation.return_value(None)
        loop.quit()

    node = Gio.DBusNodeInfo.new_for_xml(REPLY_XML)
    registration = bus.register_object(
        REPLY_PATH, node.interfaces[0], on_call, None, None
    )
    # Exceptions come back as an answer too, instead of only in KWin's log
    script = (
        "function result(value) {"
        f" callDBus({json.dumps(bus.get_unique_name())}, {json.dumps(REPLY_PATH)},"
        f" {json.dumps(REPLY_IFACE)}, 'Result', JSON.stringify({{value: value}})); }}\n"
        f"try {{\n{body}\n}} catch (e) {{"
        f" callDBus({json.dumps(bus.get_unique_name())}, {json.dumps(REPLY_PATH)},"
        f" {json.dumps(REPLY_IFACE)}, 'Result', JSON.stringify({{error: String(e)}})); }}\n"
    )
    # KWin reads the script from a file; the plugin name must be unique while it is loaded
    plugin = f"kwinpy-{os.getpid()}"
    with tempfile.NamedTemporaryFile(
        "w", suffix=".js", dir=os.environ.get("XDG_RUNTIME_DIR")
    ) as f:
        f.write(script)
        f.flush()
        try:
            (script_id,) = bus.call_sync(
                KWIN,
                "/Scripting",
                "org.kde.kwin.Scripting",
                "loadScript",
                GLib.Variant("(ss)", (f.name, plugin)),
                GLib.VariantType("(i)"),
                Gio.DBusCallFlags.NONE,
                TIMEOUT_MS,
                None,
            ).unpack()
        except GLib.Error as e:
            sys.exit(
                f"KWin doesn't answer (is `devvm desktop-up kwin` or `plasma` running?): {e.message}"
            )
        try:
            if script_id < 0:
                sys.exit("KWin did not load the script")
            bus.call_sync(
                KWIN,
                f"/Scripting/Script{script_id}",
                "org.kde.kwin.Script",
                "run",
                None,
                None,
                Gio.DBusCallFlags.NONE,
                TIMEOUT_MS,
                None,
            )
            GLib.timeout_add(TIMEOUT_MS, loop.quit)
            loop.run()
        finally:
            bus.call_sync(
                KWIN,
                "/Scripting",
                "org.kde.kwin.Scripting",
                "unloadScript",
                GLib.Variant("(s)", (plugin,)),
                None,
                Gio.DBusCallFlags.NONE,
                TIMEOUT_MS,
                None,
            )
            bus.unregister_object(registration)
    if not answers:
        sys.exit(
            f"the KWin script did not call result() within {TIMEOUT_MS // 1000} s;"
            " its log: journalctl --user -u desktop-kwin"
        )
    answer = json.loads(answers[0])
    if "error" in answer:
        sys.exit(f"the KWin script failed: {answer['error']}")
    return answer.get("value")


def main():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("windows")
    sub.add_parser("raise").add_argument("title")
    sub.add_parser("eval").add_argument("javascript")
    args = p.parse_args()

    if args.cmd == "windows":
        for w in run_script(WINDOWS):
            print(
                w["pid"],
                "wayland" if w["wayland"] else "x11",
                f"@{w['x']},{w['y']} {w['width']}x{w['height']}",
                "focus" if w["focus"] else "-",
                w["title"],
            )
    elif args.cmd == "raise":
        if run_script(RAISE % json.dumps(args.title)) == 0:
            sys.exit(f"no window titled {args.title!r}")
    elif args.cmd == "eval":
        print(json.dumps(run_script(args.javascript)))


if __name__ == "__main__":
    main()
