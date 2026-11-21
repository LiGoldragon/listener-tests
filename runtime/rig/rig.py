"""Isolated rig for the Listener runtime: each scenario starts a private stack and tears it down.

Everything the rig starts, it starts with an explicit environment: a private session bus, the
notification fixture on that bus, loopback OpenAI and Gemini fixtures, a private PulseAudio
null-sink daemon, the Translator and the Nexus. Every client call carries its own XDG_RUNTIME_DIR,
XDG_CONFIG_HOME and HOME, so no live listener socket and no real bus or Pulse daemon is addressed.

The driver (runtime/run.sh) sets, and this module requires:

  LISTENER_BIN_DIR    directory holding listener, listener-meta, listener-nexus, listener-translator
  PULSE_BIN_DIR       directory holding pulseaudio, paplay, parecord, pactl (PulseAudio 17.0 was used)
  LISTENER_RIG_WORK   directory for run logs and receipts, one subdirectory per scenario
  LISTENER_RIG_SOCK   a fresh, short directory for sockets (a Unix socket path must stay under 108
                      bytes), one subdirectory per scenario
  LISTENER_RIG_SCENARIO  the scenario name (set by scenarios.py for each scenario process)
"""
import errno
import json
import os
import shutil
import signal
import subprocess
import sys
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))


def require(name):
    value = os.environ.get(name)
    if not value:
        raise SystemExit("%s must be set; see runtime/README.md" % name)
    return value


BIN = require("LISTENER_BIN_DIR")
PULSE_BIN = require("PULSE_BIN_DIR")
WORK = require("LISTENER_RIG_WORK")
SOCK = require("LISTENER_RIG_SOCK")
SCENARIO = os.environ.get("LISTENER_RIG_SCENARIO", "scratch")

PY = sys.executable
DBUS_DAEMON = shutil.which("dbus-daemon")
OPUSINFO = shutil.which("opusinfo")

PLACE = "rig"
OPENAI_PORT = 18123
GEMINI_PORT = 18124
# A fixture key for the loopback services only; it is not a credential.
KEY = "rig-dummy-key-not-a-credential"

RUN = os.path.join(WORK, SCENARIO)  # this scenario's files: state, audio, logs, receipts
SOCKS = os.path.join(SOCK, SCENARIO)  # this scenario's sockets
RT_ROOT = os.path.join(SOCKS, "xdg")  # the clients' XDG_RUNTIME_DIR
RUNTIME = os.path.join(RT_ROOT, "nexus", PLACE)  # the place runtime the clients route to
STATE = os.path.join(RUN, "state")
AUDIO = os.path.join(STATE, "audio")
PA = os.path.join(SOCKS, "pa")  # the private Pulse runtime; its socket is PA/s
DBUS_SOCK = os.path.join(SOCKS, "bus")
DBUS_ADDR = "unix:path=" + DBUS_SOCK
DESK = os.path.join(RUN, "desk")
FX = os.path.join(RUN, "fx")
KEY_FILE = os.path.join(FX, "expected-key")
LOGS = os.path.join(RUN, "logs")
RECEIPTS = os.path.join(RUN, "receipts.jsonl")  # this scenario's own receipts


def clean_env():
    return {
        "PATH": os.environ["PATH"],
        "HOME": os.path.join(RUN, "home"),
        "LANG": "C.UTF-8",
        "XDG_CONFIG_HOME": os.path.join(RUN, "config"),
        "XDG_RUNTIME_DIR": RT_ROOT,
    }


def service_env():
    """The environment of the services that reach the private bus and the desk."""
    env = clean_env()
    env["DBUS_SESSION_BUS_ADDRESS"] = DBUS_ADDR
    env["LISTENER_RIG_DESK"] = DESK
    env["LISTENER_RIG_PYTHON"] = PY
    env["LISTENER_RIG_KEY_FILE"] = KEY_FILE
    return env


def check_tools():
    """Refuse to start a run whose tools or binaries are missing, naming each one."""
    missing = []
    if not DBUS_DAEMON:
        missing.append("dbus-daemon on PATH")
    if not OPUSINFO:
        missing.append("opusinfo (opus-tools) on PATH")
    for name in ("pulseaudio", "paplay", "parecord", "pactl"):
        if not os.path.exists(os.path.join(PULSE_BIN, name)):
            missing.append("%s in PULSE_BIN_DIR" % name)
    for name in ("listener", "listener-meta", "listener-nexus", "listener-translator"):
        if not os.path.exists(os.path.join(BIN, name)):
            missing.append("%s in LISTENER_BIN_DIR" % name)
    if missing:
        raise SystemExit("missing: " + "; ".join(missing))
    if len(os.path.join(SOCKS, "xdg", "nexus", PLACE, "listener-translator.sock").encode()) > 107:
        raise SystemExit("LISTENER_RIG_SOCK is too long for Unix socket paths: " + SOCK)


def now():
    return time.time()


class Rig:
    def __init__(self, label):
        self.label = label
        self.procs = {}
        self.nexus = None
        self.dbus_pid = None
        self.pulse_pid = None
        os.makedirs(RUN, exist_ok=True)
        os.makedirs(LOGS, exist_ok=True)

    # ---------- receipts ----------
    def receipt(self, **kw):
        # The scenario is recorded as its own key; a step's label, when it has one, stays the label.
        kw["scenario"] = self.label
        kw["t"] = round(now(), 3)
        with open(RECEIPTS, "a") as f:
            f.write(json.dumps(kw, sort_keys=True) + "\n")

    # ---------- setup ----------
    def prepare(self):
        # A fresh run: nothing from an earlier run of this scenario survives.
        shutil.rmtree(RUN, ignore_errors=True)
        shutil.rmtree(SOCKS, ignore_errors=True)
        for d in (RUN, LOGS, DESK, FX, STATE, AUDIO, os.path.join(RUN, "home"),
                  os.path.join(RUN, "config", "nexus")):
            os.makedirs(d, exist_ok=True)
        os.makedirs(RT_ROOT, mode=0o700, exist_ok=True)
        os.chmod(RT_ROOT, 0o700)
        with open(os.path.join(RUN, "config", "nexus", "listener-nexus.datom"), "w") as f:
            f.write("{ %s }\n" % PLACE)
        with open(KEY_FILE, "w") as f:
            f.write(KEY + "\n")
        with open(os.path.join(DESK, "desk-control.json"), "w") as f:
            json.dump({"paste": "ok", "copy": "ok"}, f)
        with open(os.path.join(FX, "notify-mode"), "w") as f:
            f.write("accept\n")

    def start_dbus(self):
        out = subprocess.run(
            [DBUS_DAEMON, "--session", "--address=" + DBUS_ADDR, "--fork", "--print-pid=1", "--nopidfile"],
            capture_output=True, text=True, env=clean_env(),
        )
        self.dbus_pid = int(out.stdout.strip() or 0) if out.returncode == 0 else None
        self.receipt(event="dbus-start", pid=self.dbus_pid, address=DBUS_ADDR, rc=out.returncode, err=out.stderr)
        if out.returncode or not out.stdout.strip():
            raise RuntimeError("dbus-daemon did not start: " + out.stderr)
        return self.dbus_pid

    def start_notify(self):
        log = os.path.join(FX, "notify.jsonl")
        self.proc("notify", [PY, "-I", os.path.join(HERE, "notify_fixture.py"), log,
                             os.path.join(FX, "notify-mode")], service_env())
        self.wait_file_contains(log, "owned", 10)
        return log

    def start_http(self):
        self.proc("http", [PY, "-I", os.path.join(HERE, "http_fixture.py"), str(OPENAI_PORT), str(GEMINI_PORT),
                           FX], service_env())
        for _ in range(100):
            try:
                urllib.request.urlopen("http://127.0.0.1:%d/__control/log" % OPENAI_PORT, timeout=1).read()
                return True
            except Exception:
                time.sleep(0.1)
        raise RuntimeError("http fixture did not come up")

    def start_pulse(self):
        env = dict(os.environ)
        env["PULSE_BIN_DIR"] = PULSE_BIN
        out = subprocess.run([PY, "-I", os.path.join(HERE, "audio_source.py"), "start", PA, "--sink", "fixture"],
                             capture_output=True, text=True, env=env, timeout=120)
        self.receipt(event="pulse-start", rc=out.returncode, out=out.stdout, err=out.stderr[-800:])
        if out.returncode:
            raise RuntimeError("pulse start failed: " + out.stderr[-800:])
        info = json.loads(out.stdout.strip().splitlines()[-1])
        self.pulse_pid = info["pid"]
        return info

    def set_desk(self, paste=None, copy=None):
        path = os.path.join(DESK, "desk-control.json")
        cur = json.load(open(path))
        if paste:
            cur["paste"] = paste
        if copy:
            cur["copy"] = copy
        with open(path, "w") as f:
            json.dump(cur, f)
        for kind in ("paste", "copy"):
            fifo = os.path.join(DESK, "gate-" + kind)
            if not os.path.exists(fifo):
                os.mkfifo(fifo)

    def set_notify(self, mode):
        with open(os.path.join(FX, "notify-mode"), "w") as f:
            f.write(mode + "\n")

    def set_http_plan(self, openai=None, gemini=None):
        body = json.dumps({"openai": openai or [], "gemini": gemini or []}).encode()
        req = urllib.request.Request("http://127.0.0.1:%d/__control/plan" % OPENAI_PORT, data=body,
                                     headers={"Content-Type": "application/json"}, method="POST")
        r = urllib.request.urlopen(req, timeout=5).read().decode()
        self.receipt(event="http-plan", openai=openai or [], gemini=gemini or [], reply=r)
        return r

    def http_log(self):
        try:
            return json.loads(urllib.request.urlopen("http://127.0.0.1:%d/__control/log" % OPENAI_PORT,
                                                     timeout=5).read().decode())
        except Exception:
            return []

    def release_gate(self, kind, timeout=15):
        """Open the gate of one blocked desk program: a write to its FIFO, which must find a reader.

        A non-blocking open succeeds only when a program is blocked reading the gate, so a gate
        with no blocked program is refused after the timeout instead of hanging the run."""
        fifo = os.path.join(DESK, "gate-" + kind)
        end = now() + timeout
        while True:
            try:
                fd = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
                break
            except OSError as error:
                if error.errno != errno.ENXIO or now() > end:
                    self.receipt(event="gate-release", kind=kind, rc=1)
                    raise RuntimeError("no program is blocked at the %s gate" % kind)
                time.sleep(0.05)
        try:
            os.write(fd, b"g")
        finally:
            os.close(fd)
        self.receipt(event="gate-release", kind=kind, rc=0)
        return 0

    def release_all_gates(self):
        """Teardown: let any desk program still blocked at a gate exit, so none outlives its run."""
        for kind in ("paste", "copy"):
            fifo = os.path.join(DESK, "gate-" + kind)
            if not os.path.exists(fifo):
                continue
            try:
                fd = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
            except OSError as error:
                if error.errno == errno.ENXIO:  # no program is blocked on this gate
                    continue
                raise
            try:
                os.write(fd, b"g")
            finally:
                os.close(fd)

    def desk_log(self):
        path = os.path.join(DESK, "desk.jsonl")
        if not os.path.exists(path):
            return []
        return [json.loads(x) for x in open(path).read().splitlines() if x.strip()]

    def notify_log(self):
        path = os.path.join(FX, "notify.jsonl")
        if not os.path.exists(path):
            return []
        return [json.loads(x) for x in open(path).read().splitlines() if x.strip()]

    def wait_file_contains(self, path, needle, timeout):
        end = now() + timeout
        while now() < end:
            if os.path.exists(path) and needle in open(path).read():
                return True
            time.sleep(0.1)
        raise RuntimeError("timeout waiting for %s in %s" % (needle, path))

    # ---------- processes ----------
    def proc(self, name, argv, env, log=None):
        logfile = open(os.path.join(LOGS, "%s.log" % name), "ab") if log is None else log
        p = subprocess.Popen(argv, env=env, stdout=logfile, stderr=subprocess.STDOUT,
                             stdin=subprocess.DEVNULL, start_new_session=True)
        self.procs[name] = p
        self.receipt(event="spawn", name=name, pid=p.pid, argv=argv)
        return p

    def start_translator(self):
        sock = os.path.join(RUNTIME, "listener-translator.sock")
        place = "{ %s %s %s }" % (PLACE, RUNTIME, STATE)
        p = self.proc("translator", [os.path.join(BIN, "listener-translator"), place], service_env())
        self.wait_path(sock, 20)
        self.receipt(event="translator-up", pid=p.pid, socket=sock)
        return p

    def kill_translator(self, sig=signal.SIGKILL):
        p = self.procs.get("translator")
        if p and p.poll() is None:
            os.kill(p.pid, sig)
            p.wait(timeout=10)
        self.receipt(event="translator-killed", signal=int(sig))
        try:
            os.unlink(os.path.join(RUNTIME, "listener-translator.sock"))
        except FileNotFoundError:
            pass

    def nexus_argv(self, verb, capacity, config=None):
        place = "{ %s %s %s }" % (PLACE, RUNTIME, STATE)
        if verb == "Create":
            datom = "Create.{ %s %d %s }" % (place, capacity, config)
        else:
            datom = "Open.{ %s %d }" % (place, capacity)
        return [os.path.join(BIN, "listener-nexus"), datom]

    def start_nexus(self, verb, capacity=64, config=None):
        argv = self.nexus_argv(verb, capacity, config)
        self.receipt(event="nexus-argv", argv=argv)
        p = self.proc("nexus-%s-%d" % (verb.lower(), len(self.procs)), argv, clean_env())
        self.nexus = p
        self.wait_path(os.path.join(RUNTIME, "listener.sock"), 20)
        self.receipt(event="nexus-up", pid=p.pid, verb=verb, capacity=capacity)
        return p

    def kill_nexus(self, sig=signal.SIGKILL):
        # The launcher listener-nexus execs listener-nexus-engine: the same process, whose name the
        # kernel truncates to 15 bytes ("listener-nexus-"). The kill is refused for any other process.
        p = self.nexus
        comm = None
        if os.path.exists("/proc/%d/comm" % p.pid):
            comm = open("/proc/%d/comm" % p.pid).read().strip()
        if comm is not None and not comm.startswith("listener-nexus"):
            raise RuntimeError("refusing to signal pid %d: it is %r, not the Nexus" % (p.pid, comm))
        if p.poll() is None:
            os.kill(p.pid, sig)
            p.wait(timeout=15)
        self.receipt(event="nexus-killed", pid=p.pid, comm=comm, signal=int(sig))
        for name in ("listener.sock", "listener-meta.sock"):
            try:
                os.unlink(os.path.join(RUNTIME, name))
            except FileNotFoundError:
                pass

    def wait_path(self, path, timeout):
        end = now() + timeout
        while now() < end:
            if os.path.exists(path):
                return True
            time.sleep(0.05)
        raise RuntimeError("timeout waiting for " + path)

    # ---------- clients ----------
    def call(self, binary, datom, timeout=60, label=None):
        argv = [os.path.join(BIN, binary), datom]
        t0 = now()
        p = subprocess.run(argv, env=clean_env(), capture_output=True, text=True, timeout=timeout)
        dt = round(now() - t0, 3)
        self.receipt(event="client", binary=binary, argv=argv, rc=p.returncode, out=p.stdout.strip(),
                     err=p.stderr.strip()[-600:], seconds=dt, label=label)
        return p.returncode, p.stdout.strip(), p.stderr.strip()

    def listener(self, datom, **kw):
        return self.call("listener", datom, **kw)

    def meta(self, datom, **kw):
        return self.call("listener-meta", datom, **kw)

    def subscribe(self, outfile):
        """Long-lived ordinary Subscribe; frames append to outfile."""
        f = open(outfile, "ab")
        p = subprocess.Popen([os.path.join(BIN, "listener"), "Subscribe"], env=clean_env(), stdout=f,
                             stderr=subprocess.STDOUT, stdin=subprocess.PIPE)
        self.procs["subscribe-%s" % os.path.basename(outfile)] = p
        self.receipt(event="subscribe-open", pid=p.pid, out=outfile)
        return p

    # ---------- teardown ----------
    def teardown(self):
        self.release_all_gates()
        for name, p in list(self.procs.items()):
            if p.poll() is None:
                try:
                    p.kill()
                    p.wait(timeout=10)
                except Exception:
                    pass
        if self.pulse_pid:
            subprocess.run([PY, "-I", os.path.join(HERE, "audio_source.py"), "stop", PA], capture_output=True,
                           text=True, env=dict(os.environ, PULSE_BIN_DIR=PULSE_BIN), timeout=60)
        if self.dbus_pid:
            try:
                os.kill(self.dbus_pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        self.receipt(event="teardown")
        shutil.rmtree(SOCKS, ignore_errors=True)


def config_datom(audio_dir=AUDIO, openai_backend=True, extra_backends=(), order=None,
                 delivery="Pasted", sequence="AsRecorded", audio_kept="Forever", text_kept="Forever",
                 source="Named.fixture.monitor", pulse_server=None):
    backends = [
        "{ fixture OpenAiTranscription http://127.0.0.1:%d fixture-model fixture-key en "
        "«Transcribe all words exactly as heard.» 20 }" % OPENAI_PORT
    ] + list(extra_backends)
    order_list = order if order is not None else ["fixture"]
    server = pulse_server or ("Named.«unix:%s/s»" % PA)
    return (
        "{ [ %s ] [ %s ] %s %s %s %s [ Listener ] 32 %s 50 "
        "%s %s %s %s "
        "{ 65536 32 } "
        "{ %s Listener 48000 Mono 3840 15360 8 TwentyMilliseconds Voice Variable 5 Voice Automatic "
        "Disabled Disabled 0 2 SyncEachPage Interrupt } }"
    ) % (" ".join(backends), " ".join(order_list), delivery, sequence, audio_kept, text_kept,
         source, os.path.join(HERE, "bin", "paste"), os.path.join(HERE, "bin", "copy"),
         os.path.join(HERE, "bin", "read-key"), audio_dir, server)


def make_wav(name, ids):
    """Synthesize the tone utterances for ids (one utterance per id) under the run directory."""
    path = os.path.join(RUN, name)
    subprocess.run([PY, "-I", os.path.join(HERE, "fixture_audio.py"), "gen", path, "--ids",
                    ",".join(map(str, ids))], check=True, capture_output=True, text=True)
    return path
