#!/usr/bin/python3
"""A minimal StatusNotifierWatcher (org.kde.StatusNotifierWatcher) on the session bus.

It stands in for a desktop's notification area: items register with it, and tray.py reads
them. Unless --no-host is given it also reports a registered host, as a panel would.

    sni-watcher.py [--no-host]

Items are kept as "<bus name><object path>", as KDE's watcher does, and dropped when
their bus name goes away.
"""

import argparse
import sys

from gi.repository import Gio, GLib

NAME = "org.kde.StatusNotifierWatcher"
PATH = "/StatusNotifierWatcher"
XML = """
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg name="service" type="s" direction="in"/></method>
    <method name="RegisterStatusNotifierHost"><arg name="service" type="s" direction="in"/></method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
    <signal name="StatusNotifierHostUnregistered"/>
  </interface>
</node>
"""


def log(msg):
    print(msg, flush=True)


class Watcher:
    def __init__(self, host):
        self.host = host
        self.items = []  # "<bus name><path>", in registration order
        self.conn = None

    def emit(self, signal, params=None):
        self.conn.emit_signal(None, PATH, NAME, signal, params)

    def changed(self, prop, value):
        self.conn.emit_signal(
            None,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            GLib.Variant("(sa{sv}as)", (NAME, {prop: value}, [])),
        )

    def items_changed(self):
        self.changed("RegisteredStatusNotifierItems", GLib.Variant("as", self.items))

    def on_call(self, conn, sender, path, iface, method, params, invocation):
        if method == "RegisterStatusNotifierItem":
            (service,) = params.unpack()
            # The argument is a bus name, or (libappindicator) an object path on the sender
            if service.startswith("/"):
                item = sender + service
            else:
                item = service + "/StatusNotifierItem"
            if item not in self.items:
                self.items.append(item)
                log(f"item registered: {item} (from {sender})")
                self.emit("StatusNotifierItemRegistered", GLib.Variant("(s)", (item,)))
                self.items_changed()
            invocation.return_value(None)
        elif method == "RegisterStatusNotifierHost":
            invocation.return_value(None)
        else:
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod", method
            )

    def on_get(self, conn, sender, path, iface, prop):
        if prop == "RegisteredStatusNotifierItems":
            return GLib.Variant("as", self.items)
        if prop == "IsStatusNotifierHostRegistered":
            return GLib.Variant("b", self.host)
        if prop == "ProtocolVersion":
            return GLib.Variant("i", 0)
        return None

    def on_name_owner_changed(self, conn, sender, path, iface, signal, params):
        name, _old, new = params.unpack()
        if new:
            return
        gone = [i for i in self.items if i.split("/", 1)[0] == name]
        for item in gone:
            self.items.remove(item)
            log(f"item unregistered: {item}")
            self.emit("StatusNotifierItemUnregistered", GLib.Variant("(s)", (item,)))
        if gone:
            self.items_changed()

    def on_bus(self, conn, name):
        self.conn = conn
        info = Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0]
        conn.register_object(PATH, info, self.on_call, self.on_get, None)
        conn.signal_subscribe(
            "org.freedesktop.DBus",
            "org.freedesktop.DBus",
            "NameOwnerChanged",
            "/org/freedesktop/DBus",
            None,
            Gio.DBusSignalFlags.NONE,
            self.on_name_owner_changed,
        )

    def on_acquired(self, conn, name):
        log(f"watcher up on {name} (host registered: {self.host})")
        if self.host:
            self.emit("StatusNotifierHostRegistered")

    def on_lost(self, conn, name):
        sys.exit(f"could not own {name}: another watcher (a panel?) runs on this bus")


def main():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("--no-host", action="store_true", help="report no registered host")
    args = p.parse_args()
    w = Watcher(host=not args.no_host)
    Gio.bus_own_name(
        Gio.BusType.SESSION,
        NAME,
        Gio.BusNameOwnerFlags.NONE,
        w.on_bus,
        w.on_acquired,
        w.on_lost,
    )
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
