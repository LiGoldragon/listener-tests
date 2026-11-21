#!/usr/bin/env python3 -I
"""Private org.freedesktop.Notifications service for the Listener rig.

A minimal D-Bus (wire protocol) server, stdlib only. It owns
org.freedesktop.Notifications on the bus named by DBUS_SESSION_BUS_ADDRESS and
answers Notify according to the mode in MODE_FILE:

  accept   reply with a u32 notification id (the real acceptance)
  refuse   reply with a D-Bus error (the real refusal)
  badtype  reply with a string where a u32 is required (a malformed acceptance)

Every Notify call is appended to LOG as one JSON line, with the decision the
service actually made. That log, not stderr, is the judge.

  notify_fixture.py RUNDIR_LOG MODE_FILE
"""
import json
import os
import socket
import struct
import sys
import time

LOG = sys.argv[1]
MODE_FILE = sys.argv[2]
NAME = "org.freedesktop.Notifications"
PATH = "/org/freedesktop/Notifications"
IFACE = "org.freedesktop.Notifications"

METHOD_CALL, METHOD_RETURN, ERROR, SIGNAL = 1, 2, 3, 4


# ---------- marshalling ----------
def align(n, a):
    return (n + a - 1) & ~(a - 1)


class Buf:
    def __init__(self):
        self.b = bytearray()

    def pad(self, a):
        self.b.extend(b"\0" * (align(len(self.b), a) - len(self.b)))

    def u8(self, v):
        self.b.append(v)

    def u32(self, v):
        self.pad(4)
        self.b.extend(struct.pack("<I", v))

    def s(self, v):  # string, also o
        data = v.encode()
        self.pad(4)
        self.b.extend(struct.pack("<I", len(data)))
        self.b.extend(data)
        self.b.append(0)

    def sig(self, v):  # signature, also g
        data = v.encode()
        self.b.append(len(data))
        self.b.extend(data)
        self.b.append(0)

    def variant(self, sig, value):
        self.sig(sig)
        if sig == "s" or sig == "o":
            self.s(value)
        elif sig == "u":
            self.u32(value)
        elif sig == "g":
            self.sig(value)
        else:
            raise ValueError("unsupported variant signature " + sig)


def encode_body(sig, values):
    b = Buf()
    for s, v in zip(sig, values):
        if s == "s" or s == "o":
            b.s(v)
        elif s == "u":
            b.u32(v)
        else:
            raise ValueError("unsupported body type " + s)
    return bytes(b.b)


def message(mtype, serial, fields, body_sig="", body_vals=()):
    body = encode_body(body_sig, body_vals) if body_sig else b""
    fb = Buf()
    for code, sig, value in fields:
        fb.pad(8)
        fb.u8(code)
        fb.variant(sig, value)
    fields_bytes = bytes(fb.b)
    head = bytearray()
    head += b"l"
    head.append(mtype)
    head.append(0)  # flags
    head.append(1)  # protocol version
    head += struct.pack("<I", len(body))
    head += struct.pack("<I", serial)
    head += struct.pack("<I", len(fields_bytes))
    head += fields_bytes
    head += b"\0" * (align(len(head), 8) - len(head))
    return bytes(head) + body


# ---------- unmarshalling ----------
class Reader:
    def __init__(self, data, pos=0):
        self.d = data
        self.p = pos

    def take(self, a, n):
        self.p = align(self.p, a)
        v = self.d[self.p:self.p + n]
        self.p += n
        return v

    def u8(self):
        v = self.d[self.p]
        self.p += 1
        return v

    def u32(self):
        return struct.unpack("<I", self.take(4, 4))[0]

    def i32(self):
        return struct.unpack("<i", self.take(4, 4))[0]

    def s(self):
        n = self.u32()
        v = self.d[self.p:self.p + n].decode()
        self.p += n + 1
        return v

    def sig(self):
        n = self.u8()
        v = self.d[self.p:self.p + n].decode()
        self.p += n + 1
        return v

    def value(self, sig):
        """Read one complete value of the given single type signature."""
        t = sig[0]
        if t in "sog":
            return self.sig() if t == "g" else self.s()
        if t in "ui":
            return self.u32() if t == "u" else self.i32()
        if t == "y":
            return self.u8()
        if t == "b":
            return bool(self.u32())
        if t == "v":
            return self.value(self.sig())
        if t == "a":
            inner = sig[1:]
            n = self.u32()
            elem_align = _align_of(inner)
            self.p = align(self.p, elem_align)
            end = self.p + n
            out = []
            while self.p < end:
                out.append(self.value(inner))
            self.p = end
            return out
        if t == "(":
            # struct up to the matching ')'
            self.p = align(self.p, 8)
            inner = _split_struct(sig)
            return tuple(self.value(x) for x in inner)
        if t == "{":
            self.p = align(self.p, 8)
            k, v = _split_dict(sig)
            return (self.value(k), self.value(v))
        raise ValueError("unsupported type " + sig)


def _align_of(sig):
    t = sig[0]
    return {"y": 1, "b": 4, "u": 4, "i": 4, "s": 4, "o": 4, "a": 4, "g": 1, "v": 1,
            "x": 8, "t": 8, "d": 8, "(": 8, "{": 8}.get(t, 1)


def _split_struct(sig):
    # sig begins with '(' ; return member signatures
    depth, members, cur = 0, [], ""
    for ch in sig[1:-1] if sig.endswith(")") else sig[1:]:
        if ch in "([{":
            depth += 1
        if ch in ")]}":
            depth -= 1
        if ch == "{" or ch == "(":
            pass
        cur += ch
        if depth == 0 and not cur.endswith("a"):
            members.append(cur)
            cur = ""
    return members


def _split_dict(sig):
    inner = sig[1:-1]
    return inner[0], inner[1:]


def read_header_fields(d, pos, end):
    r = Reader(d, pos)
    n = r.u32()
    r.p = align(r.p, 8)
    stop = r.p + n
    fields = {}
    while r.p < stop:
        r.p = align(r.p, 8)
        code = r.u8()
        vsig = r.sig()
        fields[code] = r.value(vsig)
    return fields


def parse_message(raw):
    endian = raw[0:1]
    if endian != b"l":
        raise ValueError("big-endian message not supported")
    mtype = raw[1]
    body_len, serial, fields_len = struct.unpack("<III", raw[4:16])
    fields = read_header_fields(raw, 12, 12 + 4 + fields_len)
    body_start = align(16 + fields_len, 8)
    body = raw[body_start:body_start + body_len]
    return mtype, serial, fields, body


# ---------- transport ----------
def recv_exact(sock, n):
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise EOFError
        buf.extend(chunk)
    return bytes(buf)


def read_message(sock):
    head = recv_exact(sock, 16)
    body_len, _serial, fields_len = struct.unpack("<III", head[4:16])
    total_head = align(16 + fields_len, 8)
    rest = recv_exact(sock, total_head - 16 + body_len)
    return head + rest


def authenticate(sock):
    uid_hex = ("%d" % os.getuid()).encode().hex().encode()
    sock.sendall(b"\0AUTH EXTERNAL " + uid_hex + b"\r\n")
    line = b""
    while not line.endswith(b"\r\n"):
        line += recv_exact(sock, 1)
    if not line.startswith(b"OK"):
        raise RuntimeError("auth refused: " + repr(line))
    sock.sendall(b"BEGIN\r\n")


class Client:
    def __init__(self, address):
        if not address.startswith("unix:path="):
            raise SystemExit("need unix:path= address, got " + address)
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(address[len("unix:path="):].split(",")[0])
        authenticate(self.sock)
        self.serial = 0
        self.lock_serial = 0

    def next_serial(self):
        self.serial += 1
        return self.serial

    def call(self, dest, path, iface, member, sig="", vals=()):
        serial = self.next_serial()
        fields = [(1, "o", path), (2, "s", iface), (3, "s", member), (6, "s", dest)]
        if sig:
            fields.append((8, "g", sig))
        self.sock.sendall(message(METHOD_CALL, serial, fields, sig, vals))
        while True:
            raw = read_message(self.sock)
            mtype, s, fields, body = parse_message(raw)
            if fields.get(5) == serial and mtype in (METHOD_RETURN, ERROR):
                return mtype, fields, body

    def reply(self, serial_of_call, sender, body_sig="", body_vals=()):
        fields = [(6, "s", sender)]
        if body_sig:
            fields.append((8, "g", body_sig))
        fields.append((5, "u", serial_of_call))
        self.sock.sendall(message(METHOD_RETURN, self.next_serial(), fields, body_sig, body_vals))

    def error(self, serial_of_call, sender, name, text):
        fields = [(4, "s", name), (6, "s", sender), (8, "g", "s"), (5, "u", serial_of_call)]
        self.sock.sendall(message(ERROR, self.next_serial(), fields, "s", (text,)))


def log(entry):
    entry["time"] = time.time()
    with open(LOG, "a") as f:
        f.write(json.dumps(entry, default=str) + "\n")


def mode():
    try:
        return open(MODE_FILE).read().strip() or "accept"
    except FileNotFoundError:
        return "accept"


def main():
    client = Client(os.environ["DBUS_SESSION_BUS_ADDRESS"])
    mtype, fields, body = client.call("org.freedesktop.DBus", "/org/freedesktop/DBus",
                                      "org.freedesktop.DBus", "Hello")
    unique = Reader(body).s()
    mtype, fields, body = client.call("org.freedesktop.DBus", "/org/freedesktop/DBus",
                                      "org.freedesktop.DBus", "RequestName", "su", (NAME, 0))
    if mtype != METHOD_RETURN or Reader(body).u32() not in (1, 4):
        raise SystemExit("could not own " + NAME)
    log({"event": "owned", "name": NAME, "unique": unique})
    print("owned", NAME, unique, flush=True)
    next_id = 1
    while True:
        raw = read_message(client.sock)
        mtype, serial, fields, body = parse_message(raw)
        if mtype != METHOD_CALL:
            continue
        sender = fields.get(7, "")
        member = fields.get(3)
        iface = fields.get(2)
        if member == "Notify" and iface == IFACE:
            r = Reader(body)
            args = {}
            try:
                args["app_name"] = r.s()
                args["replaces_id"] = r.u32()
                args["app_icon"] = r.s()
                args["summary"] = r.s()
                args["body"] = r.s()
            except Exception as e:  # keep the raw record even if a field fails to parse
                args["parse_error"] = repr(e)
            current = mode()
            if current == "refuse":
                client.error(serial, sender, "org.freedesktop.DBus.Error.Failed",
                             "fixture refused the notification")
                log({"event": "notify", "decision": "refused", "mode": current, **args})
            elif current == "badtype":
                client.reply(serial, sender, "s", ("not-an-id",))
                log({"event": "notify", "decision": "badtype", "mode": current, **args})
            else:
                client.reply(serial, sender, "u", (next_id,))
                log({"event": "notify", "decision": "accepted", "mode": current,
                     "id": next_id, **args})
                next_id += 1
        else:
            if fields.get(1) is not None and mtype == METHOD_CALL:
                client.error(serial, sender, "org.freedesktop.DBus.Error.UnknownMethod",
                             "no such method")
            log({"event": "other", "member": member, "iface": iface})


if __name__ == "__main__":
    main()
