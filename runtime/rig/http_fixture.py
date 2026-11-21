#!/usr/bin/env python3 -I
"""Loopback transcription services for the Listener rig, protocol-faithful in shape.

  OpenAI   POST /v1/audio/transcriptions   multipart: model, language, prompt,
           response_format, file (audio/ogg); Authorization: Bearer <key>
  Gemini   POST /v1beta/models/<model>:generateContent   JSON with one user
           content: text prompt and inlineData (audio/ogg, base64); x-goog-api-key

The "recognition" is a substitute: each upload is decoded with the independent
oracle (fixture_audio.decode_ids), and the reply text is those utterance ids.
The uploaded bytes themselves are the thing under test (they must decode).

Control, on the OpenAI port only:
  POST /__control/plan   JSON {"openai": [actions...], "gemini": [actions...]}
                        each request on a route pops the next action; empty = ok
  GET  /__control/log   the request log so far (JSON lines as a JSON array)

Actions:  {"do":"ok"}  {"do":"status","code":N}  {"do":"malformed"}
          {"do":"delay","ms":N,"then":ACTION}

Every request is logged to LOG (JSON lines) with the action actually taken.

  http_fixture.py OPENAI_PORT GEMINI_PORT STATE_DIR
"""
import base64
import hashlib
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fixture_audio  # noqa: E402  (the independent oracle)

OPENAI_PORT = int(sys.argv[1])
GEMINI_PORT = int(sys.argv[2])
STATE = sys.argv[3]
os.makedirs(os.path.join(STATE, "uploads"), exist_ok=True)
LOG = os.path.join(STATE, "http.jsonl")
KEY_FILE = os.path.join(STATE, "expected-key")

lock = threading.Lock()
plan = {"openai": [], "gemini": []}
counter = {"n": 0}


def expected_key():
    try:
        return open(KEY_FILE).read().strip()
    except FileNotFoundError:
        return None


def next_action(route):
    with lock:
        q = plan[route]
        return q.pop(0) if q else {"do": "ok"}


def record(entry):
    with lock:
        with open(LOG, "a") as f:
            f.write(json.dumps(entry, sort_keys=True) + "\n")


def next_number():
    with lock:
        counter["n"] += 1
        return counter["n"]


def decode_ids_from_bytes(data, n):
    path = os.path.join(STATE, "uploads", "%04d.opus" % n)
    with open(path, "wb") as f:
        f.write(data)
    try:
        return fixture_audio.decode_ids(path), None
    except Exception as e:  # undecodable upload
        return None, repr(e)[:300]


def parse_multipart(body, boundary):
    fields, file_bytes, file_name, file_type = {}, None, None, None
    for part in body.split(b"--" + boundary):
        if not part or part in (b"--\r\n", b"--"):
            continue
        part = part[2:] if part.startswith(b"\r\n") else part
        head, sep, content = part.partition(b"\r\n\r\n")
        if not sep:
            continue
        if content.endswith(b"\r\n"):
            content = content[:-2]
        disp = ""
        ctype = ""
        for line in head.decode("utf-8", "replace").split("\r\n"):
            low = line.lower()
            if low.startswith("content-disposition:"):
                disp = line
            if low.startswith("content-type:"):
                ctype = line.split(":", 1)[1].strip()
        name = disp.split('name="', 1)[1].split('"', 1)[0] if 'name="' in disp else None
        if 'filename="' in disp:
            file_name = disp.split('filename="', 1)[1].split('"', 1)[0]
            file_bytes = content
            file_type = ctype
        elif name:
            fields[name] = content.decode("utf-8", "replace")
    return fields, file_bytes, file_name, file_type


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "ListenerFixture/1"

    def log_message(self, *args):
        return

    def send_json(self, status, payload, extra=None):
        raw = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(raw)

    def route_of(self):
        if self.server.server_port == OPENAI_PORT:
            return "openai"
        return "gemini"

    def do_GET(self):
        if self.path == "/__control/log":
            with lock:
                lines = open(LOG).read().splitlines() if os.path.exists(LOG) else []
            self.send_json(200, [json.loads(x) for x in lines])
            return
        self.send_json(404, {"error": "not found"})

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        if self.path == "/__control/plan":
            new = json.loads(body or b"{}")
            with lock:
                for k in ("openai", "gemini"):
                    plan[k] = list(new.get(k, []))
            self.send_json(200, {"planned": {k: len(v) for k, v in plan.items()}})
            return
        if self.path == "/__control/reset":
            with lock:
                plan["openai"].clear()
                plan["gemini"].clear()
                if os.path.exists(LOG):
                    os.remove(LOG)
            self.send_json(200, {"reset": True})
            return
        route = self.route_of()
        n = next_number()
        action = next_action(route)
        t0 = time.time()
        record({"n": n, "route": route, "phase": "arrival", "planned": action.get("do"),
                "planned_ms": action.get("ms"), "time": t0})
        if action.get("do") == "delay":
            time.sleep(action.get("ms", 0) / 1000.0)
            action = action.get("then", {"do": "ok"})
        auth_header = self.headers.get("Authorization", "")
        goog_key = self.headers.get("x-goog-api-key", "")
        key = expected_key()
        entry = {"n": n, "route": route, "path": self.path, "action": action.get("do"),
                 "content_type": self.headers.get("Content-Type", ""),
                 "body_bytes": len(body)}
        if route == "openai":
            entry["bearer_ok"] = bool(key) and auth_header == "Bearer " + key
            ctype = self.headers.get("Content-Type", "")
            boundary = ctype.split("boundary=", 1)[1].encode() if "boundary=" in ctype else b""
            fields, file_bytes, file_name, file_type = parse_multipart(body, boundary)
            entry["fields"] = {k: v for k, v in fields.items()}
            entry["file_name"] = file_name
            entry["file_type"] = file_type
            entry["file_bytes"] = len(file_bytes or b"")
            entry["file_sha256"] = hashlib.sha256(file_bytes or b"").hexdigest()
            audio = file_bytes or b""
        else:
            entry["api_key_ok"] = bool(key) and goog_key == key
            try:
                doc = json.loads(body)
                parts = doc["contents"][0]["parts"]
                entry["prompt_text"] = parts[0].get("text")
                inline = parts[1]["inlineData"]
                entry["mime"] = inline.get("mimeType")
                audio = base64.b64decode(inline["data"])
                entry["file_bytes"] = len(audio)
                entry["file_sha256"] = hashlib.sha256(audio).hexdigest()
            except Exception as e:
                entry["parse_error"] = repr(e)[:200]
                audio = b""
        if action.get("do") == "status":
            code = int(action.get("code", 500))
            entry.update({"status": code, "reply": "status"})
            record(entry)
            self.send_json(code, {"error": {"message": "fixture status %d" % code}})
            return
        if action.get("do") == "redirect":
            target = action.get("location", "/elsewhere")
            entry.update({"status": 302, "reply": "redirect", "location": target})
            record(entry)
            self.send_response(302)
            self.send_header("Location", target)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if action.get("do") == "malformed":
            entry.update({"status": 200, "reply": "malformed"})
            record(entry)
            self.send_json(200, b"<html>not json</html>")
            return
        ids, decode_error = decode_ids_from_bytes(audio, n)
        entry["heard_ids"] = ids
        if decode_error:
            entry["decode_error"] = decode_error
        text = "utterance %s" % " ".join(str(i) for i in ids) if ids else "no speech"
        if route == "openai":
            entry.update({"status": 200, "reply": "ok", "text": text,
                          "elapsed_s": round(time.time() - t0, 3)})
            record(entry)
            self.send_json(200, {"text": text}, {"x-request-id": "oai-req-%d" % n})
        else:
            payload = {"candidates": [{"content": {"role": "model", "parts": [{"text": text}]},
                                       "finishReason": "STOP", "index": 0}],
                       "responseId": "gem-resp-%d" % n}
            entry.update({"status": 200, "reply": "ok", "text": text,
                          "elapsed_s": round(time.time() - t0, 3)})
            record(entry)
            self.send_json(200, payload)


def serve(port):
    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    srv.daemon_threads = True
    srv.serve_forever()


if __name__ == "__main__":
    threading.Thread(target=serve, args=(GEMINI_PORT,), daemon=True).start()
    print("listening openai=%d gemini=%d state=%s" % (OPENAI_PORT, GEMINI_PORT, STATE), flush=True)
    serve(OPENAI_PORT)
