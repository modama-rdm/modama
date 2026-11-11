#!/usr/bin/python3
"""Read and drive StatusNotifierItems on the session bus, as a notification area would.

    tray.py items                       registered items (from the watcher)
    tray.py show                        title, status, tooltip, icon sizes, menu
    tray.py menu                        the menu: id, label, enabled
    tray.py icon DIR                    save the icon pixmaps as DIR/icon-<size>.png
    tray.py activate [X Y]              left click: the item's Activate
    tray.py click LABEL                 click the menu entry LABEL
    tray.py wait-tooltip TEXT           wait until the tooltip description contains TEXT
    tray.py wait-item | wait-gone       wait until the item is registered / gone
    tray.py watch                       print the tooltip description whenever it changes,
                                        with the time, for --timeout seconds

The item is the one whose Id is --id (default modama-session-helper). Waits and watch
take --timeout (seconds, default 30). Works with any watcher: sni-watcher.py or a panel's.
"""

import argparse
import struct
import sys
import time
import zlib
from pathlib import Path

from gi.repository import Gio, GLib

WATCHER = (
    "org.kde.StatusNotifierWatcher",
    "/StatusNotifierWatcher",
    "org.kde.StatusNotifierWatcher",
)
ITEM_IFACE = "org.kde.StatusNotifierItem"
MENU_IFACE = "com.canonical.dbusmenu"

bus = Gio.bus_get_sync(Gio.BusType.SESSION)


def call(name, path, iface, method, params=None, reply=None):
    return bus.call_sync(
        name,
        path,
        iface,
        method,
        params,
        GLib.VariantType(reply) if reply else None,
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    )


def get(name, path, iface, prop):
    return call(
        name,
        path,
        "org.freedesktop.DBus.Properties",
        "Get",
        GLib.Variant("(ss)", (iface, prop)),
        "(v)",
    ).unpack()[0]


class NoWatcher(Exception):
    pass


def items():
    try:
        return get(*WATCHER[:2], WATCHER[2], "RegisteredStatusNotifierItems")
    except GLib.Error as e:
        raise NoWatcher(
            f"no StatusNotifierWatcher on the session bus: {e.message}"
        ) from e


def split(item):
    name, _, path = item.partition("/")
    return name, "/" + path


def find(item_id):
    for item in items():
        name, path = split(item)
        try:
            if get(name, path, ITEM_IFACE, "Id") == item_id:
                return name, path
        except GLib.Error:
            continue
    return None


def find_or_exit(item_id):
    found = find(item_id)
    if found is None:
        sys.exit(f"no item with Id {item_id!r}; registered: {items()}")
    return found


def tooltip(name, path):
    _icon, _pixmaps, title, description = get(name, path, ITEM_IFACE, "ToolTip")
    return title, description


def menu_entries(name, path):
    menu_path = get(name, path, ITEM_IFACE, "Menu")
    _rev, layout = call(
        name,
        menu_path,
        MENU_IFACE,
        "GetLayout",
        GLib.Variant("(iias)", (0, -1, [])),
        "(u(ia{sv}av))",
    ).unpack()

    def flatten(node):
        node_id, props, children = node
        out = [] if node_id == 0 else [(node_id, props)]
        for child in children:
            out += flatten(child)
        return out

    return menu_path, flatten(layout)


def describe_entry(node_id, props):
    if props.get("type") == "separator":
        return f"{node_id:3}  ----"
    enabled = props.get("enabled", True)
    visible = props.get("visible", True)
    flags = ("" if enabled else " [disabled]") + ("" if visible else " [hidden]")
    return f"{node_id:3}  {props.get('label', '')}{flags}"


def png(path, width, height, rgba):
    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data))
        )

    raw = b"".join(
        b"\0" + rgba[y * width * 4 : (y + 1) * width * 4] for y in range(height)
    )
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def wait(cond, what, timeout):
    """Wait for cond(); a watcher that isn't (yet) there counts as not yet"""
    deadline = time.monotonic() + timeout
    while True:
        try:
            if cond():
                return
        except NoWatcher:
            pass
        if time.monotonic() > deadline:
            sys.exit(f"timeout after {timeout}s: {what}")
        time.sleep(0.3)


def main():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("--id", default="modama-session-helper")
    p.add_argument("--timeout", type=float, default=30)
    sub = p.add_subparsers(dest="cmd", required=True)
    for cmd in ("items", "show", "menu", "wait-item", "wait-gone", "watch"):
        sub.add_parser(cmd)
    sub.add_parser("icon").add_argument("dir", type=Path)
    a = sub.add_parser("activate")
    a.add_argument("x", type=int, nargs="?", default=0)
    a.add_argument("y", type=int, nargs="?", default=0)
    sub.add_parser("click").add_argument("label")
    sub.add_parser("wait-tooltip").add_argument("text")
    args = p.parse_args()

    if args.cmd == "items":
        for item in items():
            print(item)
    elif args.cmd == "show":
        name, path = find_or_exit(args.id)
        for prop in ("Id", "Title", "Category", "Status"):
            print(f"{prop}: {get(name, path, ITEM_IFACE, prop)}")
        title, description = tooltip(name, path)
        print(f"ToolTip: {title!r} / {description!r}")
        sizes = [f"{w}x{h}" for w, h, _ in get(name, path, ITEM_IFACE, "IconPixmap")]
        print(f"IconPixmap: {', '.join(sizes)}")
        print("Menu:")
        for entry in menu_entries(name, path)[1]:
            print("  " + describe_entry(*entry))
    elif args.cmd == "menu":
        for entry in menu_entries(*find_or_exit(args.id))[1]:
            print(describe_entry(*entry))
    elif args.cmd == "icon":
        name, path = find_or_exit(args.id)
        args.dir.mkdir(parents=True, exist_ok=True)
        for w, h, data in get(name, path, ITEM_IFACE, "IconPixmap"):
            argb = bytes(data)
            rgba = b"".join(
                argb[i + 1 : i + 4] + argb[i : i + 1] for i in range(0, len(argb), 4)
            )
            out = args.dir / f"icon-{w}.png"
            png(out, w, h, rgba)
            print(out)
    elif args.cmd == "activate":
        name, path = find_or_exit(args.id)
        call(name, path, ITEM_IFACE, "Activate", GLib.Variant("(ii)", (args.x, args.y)))
    elif args.cmd == "click":
        name, path = find_or_exit(args.id)
        menu_path, entries = menu_entries(name, path)
        match = [(i, props) for i, props in entries if props.get("label") == args.label]
        if not match:
            sys.exit(f"no menu entry {args.label!r}")
        node_id, props = match[0]
        if not props.get("enabled", True):
            sys.exit(f"menu entry {args.label!r} is disabled")
        call(
            name,
            menu_path,
            MENU_IFACE,
            "Event",
            GLib.Variant(
                "(isvu)", (node_id, "clicked", GLib.Variant("i", 0), int(time.time()))
            ),
        )
    elif args.cmd == "wait-tooltip":

        def has_text():
            found = find(args.id)
            return found is not None and args.text in tooltip(*found)[1]

        wait(has_text, f"tooltip with {args.text!r}", args.timeout)
        title, description = tooltip(*find(args.id))
        print(f"ToolTip: {title!r} / {description!r}")
    elif args.cmd == "wait-item":
        wait(lambda: find(args.id) is not None, f"item {args.id!r}", args.timeout)
    elif args.cmd == "wait-gone":
        wait(lambda: find(args.id) is None, f"item {args.id!r} to go", args.timeout)
    elif args.cmd == "watch":
        deadline = time.monotonic() + args.timeout
        last = None
        while time.monotonic() < deadline:
            try:
                found = find(args.id)
                now = tooltip(*found)[1] if found else "(no item)"
            except NoWatcher:
                now = "(no notification area)"
            if now != last:
                print(f"{time.strftime('%H:%M:%S')} {now}", flush=True)
                last = now
            time.sleep(0.1)


if __name__ == "__main__":
    try:
        main()
    except NoWatcher as e:
        sys.exit(str(e))
