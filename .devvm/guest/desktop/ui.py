#!/usr/bin/python3
"""Drive desktop apps through AT-SPI (egui exposes its widgets via AccessKit).

    ui.py apps                  list the processes with windows: PID, kind (gui, popup or
                                other), age, application; all applications unless --app
    ui.py tree                  print the widget tree (role, name, states, position)
    ui.py text                  print every widget name and text, one per line
    ui.py click NAME            run the default action of the widget named NAME
    ui.py focus NAME            activate the widget's window and give the widget the
                                keyboard focus (then: xdotool type)
    ui.py wait NAME             wait until a widget named NAME exists
    ui.py panel-icons           centers of the tray icons in xfce4-panel, one "X Y" per line
                                (for xdotool; the panel names no icon)

Every command but panel-icons acts on the processes chosen by:
    --app APP     a substring of the application name (default: modama)
    --pid PID     one process
    --newest      the process started last; for click, focus and wait, of those that have
                  the widget NAME

Several modama-agent-gui processes can be open at once: GUI windows (a click on the tray
icon starts another one) and mount popups, all with the same application name. If NAME is
found in more than one of them, click and focus refuse and list them; choose one with
--pid or --newest. Within one window the first match in tree order wins, with --last the
last one: a dialog's button comes after the page's of the same name. NAME matches exactly;
--role ROLE (as printed by tree) narrows it, e.g. --role "password text".
"""

import argparse
import os
import subprocess
import sys
import time

import pyatspi
from gi.repository import GLib

# libatspi warns on every call that AccessKit has no AT-SPI cache object ("dbind-WARNING
# ... Unknown object '/org/a11y/atspi/cache'"); that is expected and drowns real errors
GLib.log_set_handler("dbind", GLib.LogLevelFlags.LEVEL_WARNING, lambda *_: None)

DEFAULT_APP = "modama"
CLOCK_TICKS = os.sysconf("SC_CLK_TCK")


class Instance:
    """One process on the accessibility bus, with its windows"""

    def __init__(self, app):
        self.app = app
        self.pid = app.get_process_id()
        try:
            with open(f"/proc/{self.pid}/cmdline", "rb") as f:
                self.cmdline = f.read().split(b"\0")
            with open(f"/proc/{self.pid}/stat") as f:
                # field 22, after the parenthesized command name
                self.started = int(f.read().rsplit(")", 1)[1].split()[19]) / CLOCK_TICKS
        except OSError:
            self.cmdline, self.started = [], 0.0

    @property
    def kind(self):
        if b"mount-popup" in self.cmdline:
            return "popup"
        if self.app.name == "modama-agent-gui":
            return "gui"
        return "other"

    def age(self):
        with open("/proc/uptime") as f:
            seconds = float(f.read().split()[0]) - self.started
        if seconds < 120:
            return f"{seconds:.0f}s"
        if seconds < 7200:
            return f"{seconds / 60:.0f}m"
        return f"{seconds / 3600:.0f}h"

    def __str__(self):
        return f"{self.pid} {self.kind} {self.age()} {self.app.name}"


def instances(app_match=None, pid=None):
    desktop = pyatspi.Registry.getDesktop(0)
    for i in range(desktop.childCount):
        app = desktop.getChildAtIndex(i)
        if app is None:
            continue
        if app_match is not None and app_match.lower() not in (app.name or "").lower():
            continue
        instance = Instance(app)
        if pid is None or instance.pid == pid:
            yield instance


def walk(node, depth=0):
    yield node, depth
    for i in range(node.childCount):
        child = node.getChildAtIndex(i)
        if child is not None:
            yield from walk(child, depth + 1)


def describe(node):
    states = node.getState().getStates()
    flags = [
        pyatspi.stateToString(s)
        for s in states
        if s
        in (
            pyatspi.STATE_FOCUSED,
            pyatspi.STATE_ENABLED,
            pyatspi.STATE_SENSITIVE,
            pyatspi.STATE_SHOWING,
            pyatspi.STATE_CHECKED,
            pyatspi.STATE_PRESSED,
            pyatspi.STATE_SELECTED,
        )
    ]
    pos = ""
    try:
        ext = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        pos = f" @{ext.x},{ext.y} {ext.width}x{ext.height}"
    except NotImplementedError:
        pass
    text = ""
    try:
        t = node.queryText()
        content = t.getText(0, t.characterCount)
        if content and content != node.name:
            text = f" text={content!r}"
    except NotImplementedError:
        pass
    return f"{node.getRoleName()} {node.name!r}{text} [{','.join(flags)}]{pos}"


def matches(args):
    """The first (--last: the last) widget named args.name in each chosen process, as
    (instance, node)"""
    found = []
    for instance in instances(args.app, args.pid):
        named = [
            node
            for node, _ in walk(instance.app)
            if node.name == args.name
            and (args.role is None or node.getRoleName() == args.role)
        ]
        if named:
            found.append((instance, named[-1] if args.last else named[0]))
    if args.newest and found:
        found = [max(found, key=lambda m: m[0].started)]
    return found


def the_one(args):
    found = matches(args)
    if not found:
        sys.exit(f"no widget named {args.name!r} in {where(args)}")
    if len(found) > 1:
        listing = "\n".join(f"  {instance}" for instance, _ in found)
        sys.exit(
            f"{args.name!r} is in {len(found)} windows; choose one with --pid PID "
            f"or --newest:\n{listing}"
        )
    return found[0]


def where(args):
    return f"process {args.pid}" if args.pid else f"applications matching {args.app!r}"


def main():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )

    def chooser(name, app_default=DEFAULT_APP):
        # Each command gets its own options (a shared parent parser shares their defaults)
        s = sub.add_parser(name)
        s.add_argument("--app", default=app_default)
        s.add_argument("--pid", type=int)
        s.add_argument("--newest", action="store_true")
        return s

    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("panel-icons")
    chooser("apps", app_default=None)
    for cmd in ("tree", "text"):
        chooser(cmd)
    for cmd in ("click", "focus", "wait"):
        s = chooser(cmd)
        s.add_argument("name")
        s.add_argument("--role")
        s.add_argument(
            "--last", action="store_true", help="the last match in the window"
        )
        s.add_argument("--timeout", type=float, default=20, help="for wait, seconds")
    args = p.parse_args()

    if args.cmd == "apps":
        for instance in instances(args.app, args.pid):
            print(instance)
    elif args.cmd == "panel-icons":
        # In the layout of xfce4-panel.xml the tray icons are the panel's only unnamed,
        # visible push buttons (window buttons are named toggle buttons)
        for instance in instances("xfce4-panel"):
            for node, _ in walk(instance.app):
                if (
                    node.getRoleName() == "button"
                    and not node.name
                    and node.getState().contains(pyatspi.STATE_SHOWING)
                ):
                    ext = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
                    print(ext.x + ext.width // 2, ext.y + ext.height // 2)
    elif args.cmd in ("tree", "text"):
        chosen = list(instances(args.app, args.pid))
        if args.newest and chosen:
            chosen = [max(chosen, key=lambda i: i.started)]
        for instance in chosen:
            print(f"# {instance}")
            for node, depth in walk(instance.app):
                if args.cmd == "tree":
                    print("  " * depth + describe(node))
                elif node.name:
                    print(node.name)
    elif args.cmd == "wait":
        deadline = time.monotonic() + args.timeout
        while not matches(args):
            if time.monotonic() > deadline:
                sys.exit(
                    f"timeout: no widget named {args.name!r} in {where(args)} "
                    f"after {args.timeout}s"
                )
            time.sleep(0.3)
    elif args.cmd == "click":
        _, node = the_one(args)
        action = node.queryAction()
        if action.nActions == 0:
            sys.exit(f"{describe(node)} has no action")
        action.doAction(0)
    elif args.cmd == "focus":
        instance, node = the_one(args)
        # Keys typed with xdotool go to the active X window, so activate this process's window
        # first; the AT-SPI focus alone only moves the focus inside it. `search` exits with 1
        # when no window matches, which leaves `windows` empty.
        windows = subprocess.run(
            ["xdotool", "search", "--all", "--onlyvisible", "--pid", str(instance.pid)],
            check=False,
            capture_output=True,
            text=True,
        ).stdout.split()
        if windows:
            subprocess.run(
                ["xdotool", "windowactivate", "--sync", windows[0]], check=False
            )
        node.queryComponent().grabFocus()


if __name__ == "__main__":
    main()
