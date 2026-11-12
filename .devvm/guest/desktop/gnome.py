#!/usr/bin/python3
"""Ask GNOME Shell about its windows, on the Wayland desktop (`devvm desktop-up gnome`).

    gnome.py windows            the windows from bottom to top: PID, wayland or x11,
                                position and size of the frame, "focus", title
    gnome.py raise TITLE        bring the window titled TITLE to the front and focus it
    gnome.py eval JAVASCRIPT    evaluate JAVASCRIPT in GNOME Shell and print the result

A program on Wayland doesn't know where its window is, so `ui.py tree` prints positions
relative to the window's content there. `windows` gives the frame, title bar included.
"""

import argparse
import json
import sys

from gi.repository import Gio, GLib

WINDOWS = """
JSON.stringify(global.get_window_actors().map(actor => {
    const w = actor.meta_window, r = w.get_frame_rect();
    return {pid: w.get_pid(), wayland: w.get_client_type() === 0, x: r.x, y: r.y,
            width: r.width, height: r.height, focus: w.has_focus(), title: w.get_title()};
}))
"""

RAISE = """
(() => {
    const found = global.get_window_actors().map(actor => actor.meta_window)
        .filter(w => w.get_title() === %s);
    found.forEach(w => w.activate(global.get_current_time()));
    return found.length;
})()
"""


def shell_eval(script):
    """The value of `script`, as GNOME Shell's Eval returns it: JSON text"""
    try:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION)
        ok, result = bus.call_sync(
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell",
            "Eval",
            GLib.Variant("(s)", (script,)),
            GLib.VariantType("(bs)"),
            Gio.DBusCallFlags.NONE,
            5000,
            None,
        ).unpack()
    except GLib.Error as e:
        sys.exit(
            f"GNOME Shell doesn't answer (is `devvm desktop-up gnome` running?): {e.message}"
        )
    if not ok:
        # Eval answers false both for a script that throws and when the shell runs
        # without --unsafe-mode
        sys.exit(
            f"GNOME Shell could not evaluate the script: {result or 'no reason given'}"
        )
    return result


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
        # the value is a string of JSON, which Eval returns as JSON once more
        for w in json.loads(json.loads(shell_eval(WINDOWS))):
            print(
                w["pid"],
                "wayland" if w["wayland"] else "x11",
                f"@{w['x']},{w['y']} {w['width']}x{w['height']}",
                "focus" if w["focus"] else "-",
                w["title"],
            )
    elif args.cmd == "raise":
        if json.loads(shell_eval(RAISE % json.dumps(args.title))) == 0:
            sys.exit(f"no window titled {args.title!r}")
    elif args.cmd == "eval":
        print(shell_eval(args.javascript))


if __name__ == "__main__":
    main()
