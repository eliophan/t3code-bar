#!/usr/bin/env python3
"""Owned SNI protocol fixture. Run only on an explicitly supplied private bus."""
import argparse
from pathlib import Path
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--address", required=True)
parser.add_argument("--host-file", required=True, type=Path, help="File containing 1 or 0")
args = parser.parse_args()
connection = Gio.DBusConnection.new_for_address_sync(
    args.address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
    None, None)
PATH = "/StatusNotifierWatcher"
IFACE = "org.kde.StatusNotifierWatcher"
xml = '''<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<property name="ProtocolVersion" type="i" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
<signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
<signal name="StatusNotifierHostRegistered"/>
<signal name="StatusNotifierHostUnregistered"/>
</interface></node>'''
items = []


def host():
    return args.host_file.read_text().strip() == "1"


def emit(name, value=None):
    connection.emit_signal(None, PATH, IFACE, name,
                           GLib.Variant("(s)", (value,)) if value else None)


def method(conn, sender, path, interface, name, parameters, invocation):
    service = parameters.unpack()[0]
    if name == "RegisterStatusNotifierItem":
        item = sender + service if service.startswith("/") else service + "/StatusNotifierItem"
        if item not in items:
            items.append(item)
            emit("StatusNotifierItemRegistered", item)
        print("REGISTER", item, flush=True)
    invocation.return_value(None)


def prop(conn, sender, path, interface, name):
    return {"RegisteredStatusNotifierItems": GLib.Variant("as", items),
            "IsStatusNotifierHostRegistered": GLib.Variant("b", host()),
            "ProtocolVersion": GLib.Variant("i", 0)}[name]


def owner_changed(conn, sender, path, interface, signal, parameters):
    name, old, new = parameters.unpack()
    if old and not new:
        for item in list(items):
            if item.split("/")[0] == name:
                items.remove(item)
                emit("StatusNotifierItemUnregistered", item)


connection.register_object(PATH, Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], method, prop, None)
connection.signal_subscribe("org.freedesktop.DBus", "org.freedesktop.DBus", "NameOwnerChanged",
                            "/org/freedesktop/DBus", None, Gio.DBusSignalFlags.NONE, owner_changed)
connection.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                     "RequestName", GLib.Variant("(su)", (IFACE, 4)), None, Gio.DBusCallFlags.NONE, 3000, None)
last = host()


def tick():
    global last
    current = host()
    if current != last:
        last = current
        emit("StatusNotifierHostRegistered" if current else "StatusNotifierHostUnregistered")
        connection.emit_signal(None, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged",
                               GLib.Variant("(sa{sv}as)", (IFACE, {"IsStatusNotifierHostRegistered": GLib.Variant("b", current)}, [])))
        print("HOST", current, flush=True)
    return True


GLib.timeout_add(100, tick)
print("READY private protocol fixture, not a visible tray host", flush=True)
GLib.MainLoop().run()
