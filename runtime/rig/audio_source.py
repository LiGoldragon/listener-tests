#!/usr/bin/env -S python3 -I
"""audio-source: a PRIVATE PulseAudio daemon with a null sink whose monitor is a capture source.

  audio_source.py start RUNDIR [--sink NAME] [--wav FILE [--loop N]]
  audio_source.py play  RUNDIR WAV [--background]
  audio_source.py await-capture RUNDIR [--timeout S]   (exit 0 once a capture stream is connected)
  audio_source.py env   RUNDIR            (JSON: env vars a client needs)
  audio_source.py status RUNDIR
  audio_source.py stop  RUNDIR
  audio_source.py selftest

The daemon never contacts a system or user daemon: HOME, XDG_*, PULSE_* all point
inside RUNDIR, no default.pa, no udev/dbus modules, only its own unix socket.
The capture source to configure is  <sink>.monitor  (default: listener_test.monitor)
and the client environment is  PULSE_SERVER=unix:RUNDIR/s
(plus XDG_RUNTIME_DIR=RUNDIR/xdg).  RUNDIR must be an absolute path short
enough that the socket path is under 108 bytes.
"""
import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SINK = "listener_test"
_BIN_CACHE = {}


def pulse_bin_dir():
    """Directory holding pulseaudio, paplay, parecord, pactl: $PULSE_BIN_DIR, PATH, else nix."""
    if "d" in _BIN_CACHE:
        return _BIN_CACHE["d"]
    cand = os.environ.get("PULSE_BIN_DIR")
    if not cand:
        p = shutil.which("pulseaudio")
        cand = os.path.dirname(p) if p else None
    if not cand or not all(os.path.exists(os.path.join(cand, t)) for t in ("pulseaudio", "paplay", "parecord", "pactl")):
        r = subprocess.run(["nix", "build", "nixpkgs#pulseaudio", "--no-link", "--print-out-paths"],
                           capture_output=True, text=True)
        cand = None
        for line in r.stdout.split():
            if os.path.exists(os.path.join(line, "bin", "pulseaudio")):
                cand = os.path.join(line, "bin")
        if not cand:
            sys.exit("cannot find pulseaudio: set PULSE_BIN_DIR or put it on PATH (nix build said: %s)" % r.stderr[-300:])
    _BIN_CACHE["d"] = cand
    return cand


def paths(rundir):
    rundir = os.path.abspath(rundir)
    rt = os.path.join(rundir, "xdg")
    return {
        "rundir": rundir, "runtime": rt, "sock": os.path.join(rundir, "s"),
        "home": os.path.join(rundir, "home"), "state": os.path.join(rundir, "state"),
        "pid": os.path.join(rundir, "pulse.pid"), "log": os.path.join(rundir, "pulse.log"),
        "client": os.path.join(rundir, "client.conf"), "meta": os.path.join(rundir, "meta.json"),
    }


def rundir_abs(rundir):
    return os.path.abspath(rundir)


def client_env(rundir, base=None):
    """A clean environment for a client (or the daemon) bound only to the private daemon."""
    p = paths(rundir)
    env = {k: v for k, v in (base or os.environ).items()
           if not k.startswith(("PULSE_", "PIPEWIRE_", "DBUS_", "XDG_", "WAYLAND", "DISPLAY"))}
    env.update({
        "HOME": p["home"], "XDG_RUNTIME_DIR": p["runtime"],
        "XDG_CONFIG_HOME": os.path.join(p["home"], ".config"), "XDG_CACHE_HOME": os.path.join(p["home"], ".cache"),
        "XDG_DATA_HOME": os.path.join(p["home"], ".local", "share"),
        "PULSE_SERVER": "unix:" + p["sock"], "PULSE_RUNTIME_PATH": os.path.join(rundir_abs(rundir), "rt"),
        "PULSE_STATE_PATH": p["state"], "PULSE_CLIENTCONFIG": p["client"],
    })
    return env


def _alive(pid):
    try:
        os.kill(pid, 0)
        with open("/proc/%d/cmdline" % pid, "rb") as f:
            return b"pulseaudio" in f.read()
    except (OSError, ProcessLookupError):
        return False


def _pid(rundir):
    try:
        return int(open(paths(rundir)["pid"]).read())
    except (OSError, ValueError):
        return None


def run_client(rundir, tool, *args, **kw):
    return subprocess.run([os.path.join(pulse_bin_dir(), tool), *args], env=client_env(rundir), **kw)


def popen_client(rundir, tool, *args, **kw):
    return subprocess.Popen([os.path.join(pulse_bin_dir(), tool), *args], env=client_env(rundir), **kw)


def start(rundir, sink=DEFAULT_SINK, wav=None, loop=1):
    p = paths(rundir)
    if len(p["sock"].encode()) > 107:
        sys.exit("socket path too long (%d bytes, limit 107): use a shorter RUNDIR: %s" % (len(p["sock"].encode()), p["sock"]))
    if _pid(rundir) and _alive(_pid(rundir)):
        sys.exit("a private daemon already runs in %s (pid %d)" % (rundir, _pid(rundir)))
    for k in ("runtime", "home", "state"):
        os.makedirs(p[k], exist_ok=True)
    os.chmod(p["runtime"], 0o700)
    os.makedirs(os.path.join(p["rundir"], "rt"), exist_ok=True)
    if os.path.exists(p["sock"]):
        os.unlink(p["sock"])
    with open(p["client"], "w") as f:
        f.write("autospawn = no\ndefault-server = unix:%s\nenable-shm = no\n" % p["sock"])
    cmd = [os.path.join(pulse_bin_dir(), "pulseaudio"), "-n", "--daemonize=no", "--use-pid-file=no",
           "--system=no", "--exit-idle-time=-1", "--disable-shm", "--log-level=info",
           "--log-target=file:" + p["log"], "--disallow-exit=no",
           "-L", "module-native-protocol-unix socket=%s auth-anonymous=1" % p["sock"],
           "-L", "module-null-sink sink_name=%s rate=48000 channels=2 sink_properties=device.description=ListenerTestSink" % sink]
    proc = subprocess.Popen(cmd, env=client_env(rundir), stdin=subprocess.DEVNULL,
                            stdout=open(os.path.join(rundir, "pulse.stdout"), "w"), stderr=subprocess.STDOUT,
                            start_new_session=True)
    with open(p["pid"], "w") as f:
        f.write(str(proc.pid))
    with open(p["meta"], "w") as f:
        json.dump({"sink": sink, "source": sink + ".monitor", "server": "unix:" + p["sock"]}, f)
    deadline = time.time() + 20
    while time.time() < deadline:
        if proc.poll() is not None:
            sys.exit("private pulseaudio exited with %s; log:\n%s" % (proc.returncode, open(p["log"]).read()[-2000:] if os.path.exists(p["log"]) else ""))
        if os.path.exists(p["sock"]):
            r = run_client(rundir, "pactl", "info", capture_output=True, text=True, timeout=10)
            if r.returncode == 0:
                break
        time.sleep(0.1)
    else:
        sys.exit("private pulseaudio did not come up within 20 s")
    info = {"pid": proc.pid, "sink": sink, "source": sink + ".monitor", "PULSE_SERVER": "unix:" + p["sock"],
            "XDG_RUNTIME_DIR": p["runtime"]}
    if wav:
        play_cmd(rundir, wav, background=True, loops=loop)
    print(json.dumps(info))
    return info


def meta(rundir):
    return json.load(open(paths(rundir)["meta"]))


def play_cmd(rundir, wav, background=False, loops=1):
    sink = meta(rundir)["sink"]
    if background:
        script = "for i in $(seq %d); do %s --device=%s %s || exit 1; done" % (
            loops, os.path.join(pulse_bin_dir(), "paplay"), sink, subprocess.list2cmdline([os.path.abspath(wav)]))
        pr = subprocess.Popen(["sh", "-c", script], env=client_env(rundir), start_new_session=True)
        return pr
    for _ in range(loops):
        r = run_client(rundir, "paplay", "--device=" + sink, os.path.abspath(wav))
        if r.returncode:
            sys.exit("paplay failed (%d)" % r.returncode)


def await_capture(rundir, timeout=15.0, count=1):
    """Block until `count` capture streams are connected to the private daemon (parecord needs about a second)."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        r = run_client(rundir, "pactl", "list", "short", "source-outputs", capture_output=True, text=True)
        if len([l for l in r.stdout.splitlines() if l.strip()]) >= count:
            time.sleep(0.3)
            return True
        time.sleep(0.1)
    return False


def stop(rundir):
    pid = _pid(rundir)
    if pid and _alive(pid):
        os.kill(pid, signal.SIGTERM)
        for _ in range(100):
            if not _alive(pid):
                break
            time.sleep(0.1)
        else:
            os.kill(pid, signal.SIGKILL)
    p = paths(rundir)
    if os.path.exists(p["sock"]):
        os.unlink(p["sock"])
    print(json.dumps({"stopped": bool(pid)}))


def selftest():
    sys.path.insert(0, HERE)
    import fixture_audio as fa
    ok = True
    def check(name, cond, extra=""):
        nonlocal ok
        print(("PASS " if cond else "FAIL ") + name + (" " + str(extra)[:600] if not cond and extra else ""))
        ok = ok and cond
    base = tempfile.mkdtemp(prefix="r-")
    rundir = base
    ids = [3, 7, 12, 0]
    wav = os.path.join(base, "in.wav")
    fa.write_wav(wav, fa.synth(ids))
    info = None
    try:
        info = start(rundir)
        # the client talks to our socket and to nothing else
        r = run_client(rundir, "pactl", "info", capture_output=True, text=True)
        srv = [l for l in r.stdout.splitlines() if l.startswith("Server String")]
        check("pactl reaches private socket", srv and srv[0].endswith(paths(rundir)["sock"]), r.stdout + r.stderr)
        r = run_client(rundir, "pactl", "list", "short", "sources", capture_output=True, text=True)
        check("source %s listed" % info["source"], info["source"] in r.stdout, r.stdout)
        check("only the null sink's monitor exists (no hardware)", len([l for l in r.stdout.splitlines() if l.strip()]) == 1, r.stdout)
        # capture from the monitor while the WAV plays
        cap = os.path.join(base, "cap.wav")
        rec = popen_client(rundir, "parecord", "--device=" + info["source"], "--rate=16000", "--channels=1",
                           "--format=s16le", "--file-format=wav", cap)
        check("capture stream connected", await_capture(rundir))
        play_cmd(rundir, wav)
        time.sleep(0.5)
        rec.send_signal(signal.SIGINT)
        rec.wait(timeout=10)
        got = fa.decode_ids(cap)
        check("oracle on parecord WAV hears %s" % ids, got == ids, fa.decode_file(cap))
        # the form a daemon would upload: Ogg Opus of the captured audio
        ogg = os.path.join(base, "cap.ogg")
        subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-i", cap, "-c:a", "libopus", "-b:a", "24k", ogg], check=True)
        check("oracle on Ogg Opus of capture", fa.decode_ids(ogg) == ids)
        # the real user daemon was not involved: our log has the connection, the env had no foreign PULSE/XDG vars
        env = client_env(rundir, {"PULSE_SERVER": "unix:/nonexistent/foreign/native", "XDG_RUNTIME_DIR": "/nonexistent/foreign", "PATH": "x"})
        check("client env overrides foreign PULSE_SERVER/XDG_RUNTIME_DIR",
              env["PULSE_SERVER"] == "unix:" + paths(rundir)["sock"] and env["XDG_RUNTIME_DIR"] == paths(rundir)["runtime"])
    finally:
        stop(rundir)
        check("daemon stopped", not (_pid(rundir) and _alive(_pid(rundir))))
        shutil.rmtree(base, ignore_errors=True)
    print("audio_source selftest:", "OK" if ok else "FAILED")
    return 0 if ok else 1


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("start")
    s.add_argument("rundir")
    s.add_argument("--sink", default=DEFAULT_SINK)
    s.add_argument("--wav")
    s.add_argument("--loop", type=int, default=1)
    pl = sub.add_parser("play")
    pl.add_argument("rundir")
    pl.add_argument("wav")
    pl.add_argument("--background", action="store_true")
    aw = sub.add_parser("await-capture")
    aw.add_argument("rundir")
    aw.add_argument("--timeout", type=float, default=15.0)
    for n in ("env", "status", "stop"):
        sub.add_parser(n).add_argument("rundir")
    sub.add_parser("selftest")
    a = ap.parse_args(argv)
    if a.cmd == "start":
        start(a.rundir, a.sink, a.wav, a.loop)
    elif a.cmd == "play":
        pr = play_cmd(a.rundir, a.wav, a.background)
        if pr:
            print(json.dumps({"player_pid": pr.pid}))
    elif a.cmd == "await-capture":
        ok = await_capture(a.rundir, a.timeout)
        print(json.dumps({"capture_connected": ok}))
        return 0 if ok else 1
    elif a.cmd == "env":
        e = client_env(a.rundir, {})
        print(json.dumps({k: e[k] for k in ("XDG_RUNTIME_DIR", "PULSE_SERVER")} | {"source": meta(a.rundir)["source"]}))
    elif a.cmd == "status":
        pid = _pid(a.rundir)
        print(json.dumps({"running": bool(pid and _alive(pid)), "pid": pid, **(meta(a.rundir) if os.path.exists(paths(a.rundir)["meta"]) else {})}))
    elif a.cmd == "stop":
        stop(a.rundir)
    else:
        return selftest()
    return 0


if __name__ == "__main__":
    sys.exit(main())
