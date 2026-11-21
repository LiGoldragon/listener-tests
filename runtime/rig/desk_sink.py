#!/usr/bin/env python3 -I
"""Observable desktop sink: the paste or copy program of the rig.

It reads the exact text from standard input (as the Translator supplies it),
records what it received, and then behaves as CONTROL says for its kind:

  ok    exit 0, the text is delivered
  fail  exit 3, the program reports a failure (after recording the receipt)
  gate  record the receipt, then block until the harness opens the FIFO
        GATE_<kind> (a writer releases it); then exit 0

Every receipt is one JSON line in SINK_LOG: kind, sequence, pid, time, text,
the planned outcome and the exit status. A record is the sink's own account;
the harness reads it alongside the Translator's observed outcome.

  desk_sink.py KIND STATE_DIR
"""
import json
import os
import sys
import time

KIND = sys.argv[1]
STATE = sys.argv[2]
CONTROL = os.path.join(STATE, "desk-control.json")
SINK_LOG = os.path.join(STATE, "desk.jsonl")
GATE = os.path.join(STATE, "gate-" + KIND)


def control_for(kind):
    try:
        return json.load(open(CONTROL)).get(kind, "ok")
    except FileNotFoundError:
        return "ok"


def append(entry):
    entry["time"] = time.time()
    with open(SINK_LOG, "a") as f:
        f.write(json.dumps(entry, sort_keys=True) + "\n")


def main():
    data = sys.stdin.buffer.read()
    text = data.decode("utf-8")
    planned = control_for(KIND)
    seq = sum(1 for _ in open(SINK_LOG)) + 1 if os.path.exists(SINK_LOG) else 1
    append({"event": "received", "kind": KIND, "seq": seq, "pid": os.getpid(),
            "text": text, "bytes": len(data), "planned": planned})
    if planned == "fail":
        append({"event": "exit", "kind": KIND, "seq": seq, "pid": os.getpid(), "status": 3})
        sys.exit(3)
    if planned == "gate":
        # Block until the harness opens the gate for writing.
        with open(GATE, "r") as gate:
            gate.read(1)
        append({"event": "released", "kind": KIND, "seq": seq, "pid": os.getpid()})
    append({"event": "exit", "kind": KIND, "seq": seq, "pid": os.getpid(), "status": 0})
    sys.exit(0)


if __name__ == "__main__":
    main()
