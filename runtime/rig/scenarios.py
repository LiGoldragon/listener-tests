#!/usr/bin/env python3 -I
"""Scenarios for the Listener runtime.

Each scenario runs in a process of its own (LISTENER_RIG_SCENARIO=<name>), starts the isolated
stack, drives the built clients, and judges the outcome from logs the rig does not write itself:
the kept audio file, the HTTP fixture log, the desktop sink receipts, the notification fixture
log, the Jobs view, and the subscriber's frames. Results are appended to results.jsonl under the
work directory, one JSON line per scenario.

  scenarios.py --list
  scenarios.py NAME [NAME...]
"""
import base64
import hashlib
import json
import os
import re
import signal
import subprocess
import sys
import time
import wave

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rig as R  # noqa: E402
import audio_source as A  # noqa: E402
import fixture_audio as F  # noqa: E402

RESULTS = os.path.join(R.WORK, "results.jsonl")
os.makedirs(R.WORK, exist_ok=True)
os.environ["PULSE_BIN_DIR"] = R.PULSE_BIN
GATE_WAIT = 20

JOB_RE = re.compile(r"\{ ([0-9a-f]{32}) (\w+) \}")


class Check:
    def __init__(self, scenario):
        self.scenario = scenario
        self.items = []
        self.observations = []

    def ok(self, name, cond, detail=None, grade="exercised-production-path"):
        self.items.append({"check": name, "pass": bool(cond), "detail": detail, "grade": grade})
        print(("PASS " if cond else "FAIL ") + name + ("" if cond else "  detail=%s" % (str(detail)[:400])), flush=True)
        return cond

    def note(self, name, value, grade="exercised-production-path"):
        self.observations.append({"observation": name, "value": value, "grade": grade})
        print("OBS  %s = %s" % (name, str(value)[:400]), flush=True)

    def finish(self):
        rec = {"scenario": self.scenario, "checks": self.items, "observations": self.observations,
               "passed": sum(1 for c in self.items if c["pass"]), "failed": sum(1 for c in self.items if not c["pass"])}
        with open(RESULTS, "a") as f:
            f.write(json.dumps(rec) + "\n")
        print("== %s: %d passed, %d failed" % (self.scenario, rec["passed"], rec["failed"]), flush=True)
        return rec


def wav_seconds(path):
    with wave.open(path) as w:
        return w.getnframes() / float(w.getframerate())


def sha(path_or_bytes):
    data = open(path_or_bytes, "rb").read() if isinstance(path_or_bytes, str) else path_or_bytes
    return hashlib.sha256(data).hexdigest()


def jobs_map(rig):
    rc, out, err = rig.listener("Jobs", label="jobs")
    return {jid: state for jid, state in JOB_RE.findall(out)}, out


def wait_job_gone_or_failed(rig, jid, timeout=GATE_WAIT):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        m, out = jobs_map(rig)
        last = m.get(jid, "gone")
        if last in ("gone", "Failed", "Cancelled"):
            return last
        time.sleep(0.5)
    return last


def wait_desk(rig, kind, count, timeout=GATE_WAIT):
    end = time.time() + timeout
    while time.time() < end:
        n = len([e for e in rig.desk_log() if e.get("event") == "received" and e.get("kind") == kind])
        if n >= count:
            return n
        time.sleep(0.2)
    return len([e for e in rig.desk_log() if e.get("event") == "received" and e.get("kind") == kind])


def desk_received(rig, kind=None):
    return [e for e in rig.desk_log() if e.get("event") == "received" and (kind is None or e.get("kind") == kind)]


def wait_gone(path, timeout):
    end = time.time() + timeout
    while time.time() < end:
        if not os.path.exists(path):
            return True
        time.sleep(0.2)
    return not os.path.exists(path)


def wait_notify(rig, needle, count=1, timeout=GATE_WAIT):
    end = time.time() + timeout
    while time.time() < end:
        hits = [n for n in rig.notify_log() if needle in json.dumps(n)]
        if len(hits) >= count:
            return hits
        time.sleep(0.3)
    return [n for n in rig.notify_log() if needle in json.dumps(n)]


def replies(rig):
    return [h for h in rig.http_log() if h.get("phase") != "arrival"]


def arrivals(rig, route=None):
    return [h for h in rig.http_log() if h.get("phase") == "arrival" and (route is None or h.get("route") == route)]


def make_wav(rig, name, ids):
    return R.make_wav(name, ids)


def record(rig, name, ids, hold=None, label=None):
    """Toggle, play the tone utterances into the private sink, Toggle again."""
    wav = make_wav(rig, "utt-%s.wav" % name, ids)
    dur = wav_seconds(wav)
    rc, out, err = rig.listener("Toggle", label=(label or name) + ":start")
    if rc != 0 or not out.startswith("Toggled.Began."):
        return {"error": (rc, out, err)}
    jid = out.rsplit(".", 1)[1]
    t_began = time.time()
    A.play_cmd(R.PA, wav, background=True, loops=1)
    time.sleep(hold if hold is not None else max(4.0, 2.2 * dur))
    t_end_call = time.time()
    rc2, out2, err2 = rig.listener("Toggle", label=(label or name) + ":end")
    t_ended = time.time()
    return {"job": jid, "start": out, "end": out2, "end_rc": rc2, "played": ids, "wav": wav,
            "wav_seconds": round(dur, 2), "held_s": round(t_end_call - t_began, 2),
            "end_call_s": round(t_ended - t_end_call, 3), "began_wall": t_began, "ended_wall": t_ended}


def kept_audio(jid):
    return R.AUDIO + "/" + jid + ".opus"


def opus_check(path):
    if not os.path.exists(path):
        return {"exists": False}
    p = subprocess.run([R.OPUSINFO, path], capture_output=True, text=True)
    res = F.decode_file(path)
    return {"exists": True, "bytes": os.path.getsize(path), "sha256": sha(path), "opusinfo_rc": p.returncode,
            "duration": res["duration"], "ids": res["ids"], "unmatched": len(res.get("unmatched_tones", []))}


def stack(rig, config=None, capacity=64, pulse=True):
    rig.prepare()
    rig.start_dbus()
    rig.start_notify()
    rig.start_http()
    rig.start_pulse()
    rig.start_translator()
    rig.start_nexus("Create", capacity, config or R.config_datom())
    rig.set_desk(paste="ok", copy="ok")


def restart_nexus(rig, capacity=64):
    rig.kill_nexus(signal.SIGKILL)
    rig.start_nexus("Open", capacity)


def expected_text(ids):
    return "utterance " + " ".join(str(i) for i in ids)


# ---------------------------------------------------------------- scenarios

def s_happy_openai(rig, c):
    stack(rig)
    r = record(rig, "happy", [3, 7, 12], label="happy")
    c.ok("toggle began and ended", "error" not in r and r["end"] == "Toggled.Ended." + r["job"], r)
    jid = r["job"]
    state = wait_job_gone_or_failed(rig, jid)
    c.ok("job delivered and retired from Jobs", state == "gone", state)
    kept = opus_check(kept_audio(jid))
    c.note("kept audio", {k: v for k, v in kept.items() if k != "ids"})
    c.ok("kept audio is a decodable Ogg Opus file", kept.get("exists") and kept.get("opusinfo_rc") == 0 and kept.get("duration", 0) > 0, kept)
    c.ok("independent oracle hears the played utterances in the kept file", kept.get("ids") == r["played"], kept.get("ids"))
    http = [h for h in replies(rig) if h.get("route") == "openai"]
    c.ok("exactly one transcription request for the job", len(http) == 1, len(http))
    h = http[0] if http else {}
    c.ok("request is multipart to /v1/audio/transcriptions with bearer auth", h.get("path") == "/v1/audio/transcriptions" and h.get("bearer_ok") is True, h)
    c.ok("request carries model, language, prompt (with terms), response_format=json",
         h.get("fields", {}).get("model") == "fixture-model" and h.get("fields", {}).get("language") == "en"
         and h.get("fields", {}).get("response_format") == "json"
         and h.get("fields", {}).get("prompt") == "Transcribe all words exactly as heard.\nTerms: Listener", h.get("fields"))
    c.ok("uploaded file is audio/ogg, byte-identical to the kept file", h.get("file_type") == "audio/ogg" and h.get("file_sha256") == kept.get("sha256"), (h.get("file_sha256"), kept.get("sha256")))
    c.ok("service heard the uploaded tones", h.get("heard_ids") == r["played"], h.get("heard_ids"))
    d = desk_received(rig, "paste")
    c.ok("focused delivery: paste sink received the exact text once", len(d) == 1 and d[0]["text"] == expected_text(r["played"]), [x.get("text") for x in d])
    c.ok("no copy fallback on successful paste", len(desk_received(rig, "copy")) == 0)
    rc, out, err = rig.listener("History", label="history")
    c.ok("text is persisted and listed in History", ("utterance" in out) and jid in out, out[:300])
    rc, out, err = rig.listener("RecallJob." + jid, label="recall-job")
    c.ok("retained text is recallable by job id", out.startswith("JobRecalled.") and "utterance 3 7 12" in out, out)
    c.note("delivered text (desk receipt)", d[0]["text"] if d else None)
    c.note("latency toggle end (s)", r["end_call_s"])
    rc, out, err = rig.listener("Recall", label="recall-last")
    c.ok("Recall returns the last text", out.startswith("Recalled.") and "utterance 3 7 12" in out, out)
    d2 = desk_received(rig, "paste")
    c.note("Recall is an explicit repeat of the paste effect (receipts now)", len(d2), grade="observation")


def s_happy_gemini(rig, c):
    gem = ("{ gem Gemini http://127.0.0.1:%d gemini-model fixture-key en «Transcribe all words exactly as heard.» 20 }"
           % R.GEMINI_PORT)
    cfg = R.config_datom(order=["gem"], extra_backends=[gem])
    stack(rig, config=cfg)
    r = record(rig, "gem", [5, 9], label="gemini")
    jid = r.get("job", "")
    state = wait_job_gone_or_failed(rig, jid)
    c.ok("job delivered via Gemini", state == "gone", state)
    kept = opus_check(kept_audio(jid))
    c.ok("kept audio decodable and oracle hears [5, 9]", kept.get("ids") == [5, 9], kept.get("ids"))
    g = [h for h in replies(rig) if h.get("route") == "gemini"]
    c.ok("one Gemini request to generateContent with the model in the path", len(g) == 1 and g[0].get("path") == "/v1beta/models/gemini-model:generateContent", g)
    gh = g[0] if g else {}
    c.ok("Gemini request carries x-goog-api-key and inline audio/ogg byte-identical to the kept file",
         gh.get("api_key_ok") is True and gh.get("mime") == "audio/ogg" and gh.get("file_sha256") == kept.get("sha256"), gh)
    c.ok("Gemini prompt carries instruction and terms", gh.get("prompt_text", "") == "Transcribe all words exactly as heard.\nTerms: Listener"
         or gh.get("prompt_text", "").startswith("Transcribe all words exactly as heard."), gh.get("prompt_text"))
    d = desk_received(rig, "paste")
    c.ok("paste sink received the Gemini text", len(d) == 1 and d[0]["text"] == expected_text([5, 9]), [x.get("text") for x in d])


def s_ordering(rig, c, sequence):
    cfg = R.config_datom(sequence=sequence)
    stack(rig, config=cfg)
    # first request slow, so the second recording happens during the first transcription
    rig.set_http_plan(openai=[{"do": "delay", "ms": 7000, "then": {"do": "ok"}}])
    a = record(rig, "first", [3], label="first")
    t0 = time.time()
    rc, out, err = rig.listener("Jobs", label="during-first-transcription")
    jobs_latency = round(time.time() - t0, 3)
    c.ok("Jobs answers while the first transcription is in flight", rc == 0 and "Transcribing" in out, out[:200])
    c.note("Jobs latency while transcribing (s)", jobs_latency)
    b_start = time.time()
    rc, out, err = rig.listener("Toggle", label="second:start")
    b_started_after = round(time.time() - b_start, 3)
    c.ok("second Toggle begins recording while first is transcribing", out.startswith("Toggled.Began.") and b_started_after < 2.0,
         {"out": out, "seconds": b_started_after})
    b_id = out.rsplit(".", 1)[1] if out.startswith("Toggled.Began.") else None
    wav = make_wav(rig, "utt-second.wav", [7])
    A.play_cmd(R.PA, wav, background=True)
    time.sleep(max(4.0, 2.2 * wav_seconds(wav)))
    rc, out, err = rig.listener("Toggle", label="second:end")
    c.ok("second Toggle ends the second recording", out == "Toggled.Ended." + str(b_id), out)
    # wait for both to drain
    end = time.time() + 45
    order = []
    while time.time() < end:
        got = desk_received(rig, "paste")
        if len(got) >= 2:
            break
        time.sleep(0.5)
    got = desk_received(rig, "paste")
    texts = [g["text"] for g in got]
    c.note("paste order (receipts)", texts, grade="observation")
    if sequence == "AsRecorded":
        c.ok("AsRecorded delivers the first recording before the second", texts == [expected_text([3]), expected_text([7])], texts)
    else:
        c.ok("AsReady delivers both recordings", sorted(texts) == sorted([expected_text([3]), expected_text([7])]), texts)
        c.note("AsReady order is not asserted (no readiness-FIFO guarantee)", texts, grade="observation")
    c.ok("both jobs delivered and retired", wait_job_gone_or_failed(rig, a["job"]) == "gone" and wait_job_gone_or_failed(rig, b_id) == "gone")
    http = [h for h in replies(rig) if h.get("route") == "openai"]
    c.note("request arrival order (heard ids)", [h.get("heard_ids") for h in http], grade="observation")


def s_blocker(rig, c):
    """Removing a blocker advances eligible delivery: jobs wait behind an occupied claim, drain in recording order."""
    stack(rig)
    rig.set_desk(paste="gate")
    a = record(rig, "blk-a", [3], label="blocker-a")
    wait_desk(rig, "paste", 1)
    c.ok("paste of the first job is blocked on the desktop gate", len(desk_received(rig, "paste")) == 1)
    b = record(rig, "blk-b", [7], label="blocker-b")
    cc = record(rig, "blk-c", [12], label="blocker-c")
    time.sleep(3)
    m, out = jobs_map(rig)
    c.ok("waiting jobs stay waiting while the claim is occupied", m.get(b["job"]) in ("Waiting", "Transcribing") and m.get(cc["job"]) in ("Waiting", "Transcribing"), out[:300])
    c.note("states behind the blocker", {a["job"][:6]: m.get(a["job"]), b["job"][:6]: m.get(b["job"]), cc["job"][:6]: m.get(cc["job"])}, grade="observation")
    rig.set_desk(paste="ok")
    rig.release_gate("paste")
    end = time.time() + 40
    while time.time() < end and len(desk_received(rig, "paste")) < 3:
        time.sleep(0.3)
    texts = [d["text"] for d in desk_received(rig, "paste")]
    c.ok("releasing the blocker drains the waiting jobs in recording order", texts == [expected_text([3]), expected_text([7]), expected_text([12])], texts)
    c.ok("all three jobs retired", all(wait_job_gone_or_failed(rig, j) == "gone" for j in (a["job"], b["job"], cc["job"])))


def s_failure(rig, c):
    gem = ("{ gem Gemini http://127.0.0.1:%d gemini-model fixture-key en «Transcribe all words exactly as heard.» 20 }"
           % R.GEMINI_PORT)
    stack(rig, config=R.config_datom(order=["fixture", "gem"], extra_backends=[gem]))
    # F1: first backend fails, the second answers
    rig.set_http_plan(openai=[{"do": "status", "code": 500}])
    f1 = record(rig, "f1", [3], label="f1")
    st = wait_job_gone_or_failed(rig, f1["job"])
    c.ok("F1 fallback: job delivered from the second backend", st == "gone", st)
    fails = wait_notify(rig, "Transcription attempt failed")
    c.ok("F1 the failed attempt is notified through the typed Notify, and the service accepted it",
         any(n.get("decision") == "accepted" and "HTTP 500" in json.dumps(n) for n in fails), fails)
    c.ok("F1 the second backend received the job audio", len([h for h in replies(rig) if h.get("route") == "gemini"]) == 1)
    c.ok("F1 desk receives the second backend's text", [d["text"] for d in desk_received(rig, "paste")] == [expected_text([3])])
    # F2: every configured backend fails -> exhausted, audio kept, retranscribe later
    rig.set_http_plan(openai=[{"do": "status", "code": 503}], gemini=[{"do": "status", "code": 503}])
    rig.meta("Configure.Order.[ fixture gem ]")
    f2 = record(rig, "f2", [7], label="f2")
    st = wait_job_gone_or_failed(rig, f2["job"])
    c.ok("F2 exhausted job is Failed and kept", st == "Failed", st)
    ex = wait_notify(rig, "No transcription was returned")
    c.ok("F2 exhaustion is notified", len(ex) >= 1, ex)
    kept = opus_check(kept_audio(f2["job"]))
    c.ok("F2 audio remains kept and decodable after exhaustion", kept.get("exists") and kept.get("ids") == [7], kept.get("ids"))
    rig.set_http_plan()
    rc, out, err = rig.listener("Retranscribe." + f2["job"], label="retranscribe-f2")
    c.ok("F2 retranscription is accepted", out == "Retranscribing." + f2["job"], out)
    st = wait_job_gone_or_failed(rig, f2["job"])
    c.ok("F2 retranscription delivers the text", st == "gone", st)
    c.ok("F2 desk receives retranscribed text", [d["text"] for d in desk_received(rig, "paste")][-1:] == [expected_text([7])])
    # F3: zero candidates. The job names fixture and gem; remove both while the job is recording,
    # so at hearing time nothing resolves and the order is empty: exhausted with no Try.
    attempts_before = len(wait_notify(rig, "Transcription attempt failed"))
    exhausted_before = len(wait_notify(rig, "No transcription was returned"))
    rc, out, err = rig.listener("Toggle", label="f3-zero:start")
    z_id = out.rsplit(".", 1)[1]
    wav = make_wav(rig, "utt-f3.wav", [12])
    A.play_cmd(R.PA, wav, background=True)
    time.sleep(max(4.0, 2.2 * wav_seconds(wav)))
    rig.meta("Configure.RemoveBackend.fixture", label="f3-remove")
    rig.meta("Configure.RemoveBackend.gem", label="f3-remove")
    rc, out, err = rig.listener("Toggle", label="f3-zero:end")
    st = wait_job_gone_or_failed(rig, z_id)
    c.ok("F3 zero candidates ends Failed", st == "Failed", st)
    c.ok("F3 no transcription request was made for the zero-candidate job",
         not any(h.get("heard_ids") == [12] for h in rig.http_log()), [h.get("heard_ids") for h in rig.http_log()])
    attempts_after = len(wait_notify(rig, "Transcription attempt failed"))
    exhausted_after = len(wait_notify(rig, "No transcription was returned"))
    c.ok("F3 zero candidates notifies exhaustion and no attempt", exhausted_after == exhausted_before + 1 and attempts_after == attempts_before,
         {"attempts": [attempts_before, attempts_after], "exhausted": [exhausted_before, exhausted_after]})
    rig.meta("Configure.Backend.{ fixture OpenAiTranscription http://127.0.0.1:%d fixture-model fixture-key en «Transcribe all words exactly as heard.» 20 }" % R.OPENAI_PORT)
    rig.meta("Configure.Backend.{ gem Gemini http://127.0.0.1:%d gemini-model fixture-key en «Transcribe all words exactly as heard.» 20 }" % R.GEMINI_PORT)
    rig.meta("Configure.Order.[ fixture gem ]")
    # F4 negatives: plain HTTP to a non-loopback host is refused without a request
    rig.meta("Configure.Backend.{ fixture OpenAiTranscription http://example.com:80 fixture-model fixture-key en «x» 20 }")
    rig.meta("Configure.Order.[ fixture ]")
    n4 = record(rig, "f4", [3], label="f4-address")
    st = wait_job_gone_or_failed(rig, n4["job"])
    c.ok("F4 a non-loopback http address is refused and the job fails", st == "Failed", st)
    address = [n for n in rig.notify_log() if "HTTPS or a loopback" in json.dumps(n)]
    c.ok("F4 the refusal is notified", len(address) >= 1, address)
    # F5: malformed response
    rig.meta("Configure.Backend.{ fixture OpenAiTranscription http://127.0.0.1:%d fixture-model fixture-key en «x» 20 }" % R.OPENAI_PORT)
    rig.set_http_plan(openai=[{"do": "malformed"}], gemini=[])
    m5 = record(rig, "f5", [5], label="f5-malformed")
    st = wait_job_gone_or_failed(rig, m5["job"])
    c.ok("F5 a malformed response fails the attempt and exhausts", st == "Failed", st)
    c.ok("F5 the malformed response is notified", len(wait_notify(rig, "did not return a whole text")) >= 1)
    # F6: a missing credential: the secret program is asked for an unknown entry
    rig.meta("Configure.Backend.{ fixture OpenAiTranscription http://127.0.0.1:%d fixture-model no-such-entry en «x» 20 }" % R.OPENAI_PORT)
    rig.set_http_plan()
    before = len(rig.http_log())
    m6 = record(rig, "f6", [9], label="f6-key")
    st = wait_job_gone_or_failed(rig, m6["job"])
    c.ok("F6 a credential the program cannot supply fails the job before any request", st == "Failed" and len(rig.http_log()) == before, (st, len(rig.http_log()) - before))
    c.ok("F6 the credential failure is notified", len(wait_notify(rig, "did not supply a key")) >= 1)


def s_restart_paste(rig, c, what):
    """Uncertainty: a paste whose outcome is unknowable when the process dies is never replayed."""
    stack(rig)
    rig.set_desk(paste="gate", copy="ok")
    a = record(rig, "unc-a", [3], label="unc-a")
    n = wait_desk(rig, "paste", 1)
    c.ok("the paste program has received the text and is blocked", n == 1, n)
    if what == "translator":
        rig.kill_translator(signal.SIGKILL)
        time.sleep(2)
        st = wait_job_gone_or_failed(rig, a["job"])
        c.ok("translator killed mid-paste: the Nexus settles the unanswered paste as Kept (no fallback copy)", st == "gone", st)
        c.ok("no copy program was started for the unanswered paste", len(desk_received(rig, "copy")) == 0)
        rig.start_translator()
    else:  # nexus killed mid-paste: the delivery claim is persisted and settled on Open
        rig.kill_nexus(signal.SIGKILL)
        time.sleep(1)
        rig.start_nexus("Open", 64)
        m, out = jobs_map(rig)
        c.ok("after Open the claimed job is settled and not reported as pending", a["job"] not in m, out[:300])
    rig.set_desk(paste="ok")
    rig.release_gate("paste")
    time.sleep(3)
    c.ok("the orphaned paste completes exactly once after the gate opens", len([d for d in desk_received(rig, "paste")]) == 1
         and any(e.get("event") == "exit" and e.get("status") == 0 for e in rig.desk_log()), rig.desk_log()[-3:])
    c.ok("no replayed paste after restart", len(desk_received(rig, "paste")) == 1, [d["text"] for d in desk_received(rig, "paste")])
    c.ok("no copy after restart", len(desk_received(rig, "copy")) == 0)
    rc, out, err = rig.listener("Jobs", label="after-restart")
    c.ok("Jobs empty after the settled claim", out == "JobsListed.[]", out)
    # An explicit recall is a fresh instruction: it delivers the retained text once, even though the
    # earlier paste was unanswered. Nothing else delivers it.
    rc, out, err = rig.listener("RecallJob." + a["job"], label="recall-settled")
    texts = [d["text"] for d in desk_received(rig, "paste")]
    c.ok("an explicit RecallJob of the settled job hands its text to the desk once more",
         out.startswith("JobRecalled.") and "utterance 3" in out and texts == [expected_text([3]), expected_text([3])], (out, texts))


def s_settled_paste_never_replayed(rig, c, what):
    """A paste unanswered when the Nexus or the Translator dies is settled Kept on each open, and no
    open replays it: the desk receipts stay at the one invocation across three Nexus restarts, and
    the orphaned paste completes once when its gate opens."""
    stack(rig)
    rig.set_desk(paste="gate", copy="ok")
    a = record(rig, "zr-a", [3], label="zero-repeats-a")
    c.ok("the paste program has received the text and is blocked", wait_desk(rig, "paste", 1) == 1)
    if what == "translator":
        rig.kill_translator(signal.SIGKILL)
        st = wait_job_gone_or_failed(rig, a["job"])
        c.ok("translator killed mid-paste: the unanswered paste is settled Kept", st == "gone", st)
        c.ok("no copy program was started for the unanswered paste", len(desk_received(rig, "copy")) == 0)
        rig.start_translator()
    for i in range(3):
        # Three kills and opens of the Nexus. Under the nexus kind the first is the mid-paste kill.
        restart_nexus(rig)
        time.sleep(0.5)
        m, out = jobs_map(rig)
        pastes = len(desk_received(rig, "paste"))
        c.ok("Open %d: the settled job is not pending and no paste is replayed" % (i + 1),
             a["job"] not in m and pastes == 1, {"pending": a["job"] in m, "receipts": pastes})
    rig.set_desk(paste="ok")
    rig.release_gate("paste")
    time.sleep(3)
    texts = [d["text"] for d in desk_received(rig, "paste")]
    c.ok("the orphaned paste completes exactly once when its gate opens, and nothing else is pasted",
         texts == [expected_text([3])] and any(e.get("event") == "exit" and e.get("status") == 0 for e in rig.desk_log()),
         texts)
    rc, out, err = rig.listener("Jobs", label="after-restarts")
    c.ok("Jobs is empty after the settled claim", out == "JobsListed.[]", out)


def s_recall_after_unanswered_paste(rig, c):
    """An explicit RecallJob and an explicit Recall each hand the retained text to the desk exactly
    once: one new handoff per request. A restart in between adds none."""
    stack(rig)
    rig.set_desk(paste="gate", copy="ok")
    a = record(rig, "rc-a", [3], label="recall-a")
    wait_desk(rig, "paste", 1)
    restart_nexus(rig)  # mid-paste: the claim is settled Kept on this open
    time.sleep(0.5)
    c.ok("the restart settles the unanswered paste with no replay", len(desk_received(rig, "paste")) == 1,
         len(desk_received(rig, "paste")))
    rig.set_desk(paste="ok")
    rc, out, err = rig.listener("RecallJob." + a["job"], label="recall-job")
    c.ok("RecallJob answers with the retained text", out.startswith("JobRecalled.") and "utterance 3" in out, out)
    c.ok("RecallJob adds exactly one paste", wait_desk(rig, "paste", 2) == 2 and len(desk_received(rig, "paste")) == 2,
         len(desk_received(rig, "paste")))
    rc, out, err = rig.listener("Recall", label="recall-last")
    c.ok("Recall answers with the last retained text", out.startswith("Recalled.") and "utterance 3" in out, out)
    c.ok("Recall adds exactly one paste", wait_desk(rig, "paste", 3) == 3 and len(desk_received(rig, "paste")) == 3,
         len(desk_received(rig, "paste")))
    restart_nexus(rig)
    time.sleep(0.5)
    c.ok("a restart after the explicit recalls adds no paste", len(desk_received(rig, "paste")) == 3,
         len(desk_received(rig, "paste")))
    rig.release_gate("paste")
    time.sleep(2)
    texts = [d["text"] for d in desk_received(rig, "paste")]
    c.ok("three pastes in all: the orphan once, then one per explicit request, all the same text",
         texts == [expected_text([3])] * 3, texts)


def s_restart_drains_later_jobs(rig, c):
    """The claim settled on open does not hold the order: the job waiting behind it is offered once,
    after the open, and the settled job's paste is not replayed."""
    stack(rig)
    rig.set_desk(paste="gate", copy="ok")
    a = record(rig, "dl-a", [3], label="drain-a")
    wait_desk(rig, "paste", 1)
    b = record(rig, "dl-b", [7], label="drain-b")
    time.sleep(2)
    m, out = jobs_map(rig)
    c.ok("the later job waits behind the outstanding claim", m.get(b["job"]) == "Waiting", out[:300])
    rig.kill_nexus(signal.SIGKILL)  # mid-paste of a; b waits
    rig.set_desk(paste="ok")
    rig.start_nexus("Open", 64)
    st = wait_job_gone_or_failed(rig, b["job"])
    c.ok("the later job is delivered once after the open", st == "gone", st)
    texts = [d["text"] for d in desk_received(rig, "paste")]
    c.ok("the settled job is not replayed: only a's orphan and b's text", texts == [expected_text([3]), expected_text([7])], texts)
    rig.release_gate("paste")
    time.sleep(2)
    m, out = jobs_map(rig)
    c.ok("after the orphan completes, Jobs is empty", out == "JobsListed.[]", out)


def s_restart_hearing(rig, c):
    """A transcription in flight when the Nexus dies: Open re-hears the job; a repeated attempt is allowed."""
    stack(rig)
    rig.set_http_plan(openai=[{"do": "delay", "ms": 8000, "then": {"do": "ok"}}])
    a = record(rig, "rh", [3], label="rh")
    time.sleep(1.0)
    m, out = jobs_map(rig)
    c.ok("job is in flight (Transcribing) before the kill", m.get(a["job"]) == "Transcribing", out[:200])
    rig.kill_nexus(signal.SIGKILL)
    rig.start_nexus("Open", 64)
    st = wait_job_gone_or_failed(rig, a["job"], timeout=40)
    c.ok("Open re-hears the job and delivers its text", st == "gone", st)
    d = desk_received(rig, "paste")
    c.ok("desk receives the text once at delivery", len(d) == 1 and d[0]["text"] == expected_text([3]), [x["text"] for x in d])
    arr = arrivals(rig, "openai")
    c.ok("two requests reached the service: the attempt in flight at the kill, and the re-hear", len(arr) == 2, arr)
    c.ok("the re-hear sent the same kept audio", len({h.get("file_sha256") for h in replies(rig)}) == 1, [h.get("file_sha256") for h in replies(rig)])
    time.sleep(9.0)  # the stale attempt's reply returns to the dead Nexus's exchange; it must add no delivery
    d = desk_received(rig, "paste")
    c.ok("the stale reply of the killed hearing adds no second delivery", len(d) == 1, [x["text"] for x in d])
    c.ok("the stale attempt's reply was received by the service path (its own reply is logged)", len(replies(rig)) >= 2, len(replies(rig)))


def s_partial_capture(rig, c):
    """Kill the Nexus during a recording: the tail is repaired and the recording is heard on Open."""
    stack(rig)
    rc, out, err = rig.listener("Toggle", label="pc:start")
    jid = out.rsplit(".", 1)[1]
    wav = make_wav(rig, "utt-pc.wav", [3])
    A.play_cmd(R.PA, wav, background=True)
    time.sleep(3.5)
    rig.kill_nexus(signal.SIGKILL)
    before = opus_check(kept_audio(jid))
    c.note("file before Open (partial recording)", {k: v for k, v in before.items() if k != "ids"}, grade="observation")
    rig.start_nexus("Open", 64)
    st = wait_job_gone_or_failed(rig, jid, timeout=40)
    after = opus_check(kept_audio(jid))
    c.note("file after Open (repaired)", {k: v for k, v in after.items() if k != "ids"})
    c.ok("the interrupted recording is repaired to a decodable Ogg Opus file", after.get("opusinfo_rc") == 0 and after.get("duration", 0) > 0, after)
    texts = [d["text"] for d in desk_received(rig, "paste")]
    c.ok("the recovered job is heard and delivered once, with the text the repaired audio holds",
         st == "gone" and texts == [expected_text([3])], {"state": st, "texts": texts})


def s_cancel_and_capture_failure(rig, c):
    stack(rig)
    cfg_ok = True
    # failed capture: an absent source is refused and the fresh job removed, leaving prior jobs as they were
    prior, _ = jobs_map(rig)
    rig.meta("Configure.Source.Named.no-such-source")
    rc, out, err = rig.listener("Toggle", label="absent-source")
    c.ok("a refused start is refused as NoMicrophone", out == "Refused.NoMicrophone", out)
    after, _ = jobs_map(rig)
    c.ok("the refused start leaves no job behind", after == prior, after)
    rig.meta("Configure.Source.Named.fixture.monitor")
    # cancel keeps job and audio, and skips hearing
    rc, out, err = rig.listener("Toggle", label="cancel:start")
    jid = out.rsplit(".", 1)[1]
    wav = make_wav(rig, "utt-cx.wav", [7])
    A.play_cmd(R.PA, wav, background=True)
    time.sleep(6.0)
    rc, out, err = rig.listener("Cancel", label="cancel")
    c.ok("Cancel ends the capture", out == "Cancelled." + jid, out)
    time.sleep(1.5)
    m, _ = jobs_map(rig)
    c.ok("cancelled job stays in Jobs as Cancelled", m.get(jid) == "Cancelled", m.get(jid))
    k = opus_check(kept_audio(jid))
    c.ok("cancelled job keeps its audio, decodable", k.get("opusinfo_rc") == 0 and k.get("ids") == [7], k.get("ids"))
    c.ok("cancel skips hearing (no request for the job)", not any(h.get("heard_ids") == [7] for h in rig.http_log()))
    rc, out, err = rig.listener("Retranscribe." + jid, label="retranscribe-cancelled")
    c.ok("a cancelled job can be retranscribed from its kept audio", out == "Retranscribing." + jid, out)
    st = wait_job_gone_or_failed(rig, jid)
    c.ok("retranscription delivers the cancelled job", st == "gone", st)
    rc, out, err = rig.listener("Cancel", label="cancel-nothing")
    c.ok("Cancel with nothing recording is refused", out == "Refused.NothingRecording", out)
    rc, out, err = rig.listener("Retranscribe.zzzz", label="retranscribe-unknown")
    c.ok("Retranscribe of an unknown job is refused NoSuchJob", out == "Refused.NoSuchJob.zzzz", out)


def s_retention(rig, c, which):
    if which == "audio":
        cfg = R.config_datom(audio_kept="Days.0", text_kept="Forever")
    else:
        cfg = R.config_datom(audio_kept="Forever", text_kept="Days.0")
    stack(rig, config=cfg)
    a = record(rig, "ret-a", [3], label="ret-a")
    st = wait_job_gone_or_failed(rig, a["job"])
    c.ok("job delivered", st == "gone", st)
    c.ok("before the next start the kept audio exists", os.path.exists(kept_audio(a["job"])))
    b = record(rig, "ret-b", [7], label="ret-b")   # the next start prunes after its Began reply
    gone = wait_gone(kept_audio(a["job"]), 5)
    rc, out, err = rig.listener("RecallJob." + a["job"], label="recall-a")
    rc2, out2, err2 = rig.listener("Retranscribe." + a["job"], label="retranscribe-a")
    if which == "audio":
        c.ok("audio retention expired: the file of the old job is removed", gone)
        c.ok("text retention forever: the old job's text is still recallable", out.startswith("JobRecalled.") and "utterance 3" in out, out)
        c.ok("retranscription of erased audio is refused NoAudio", out2 == "Refused.NoAudio." + a["job"], out2)
    else:
        c.ok("text retention expired: the old job's text is no longer recallable", out == "Refused.NothingKept", out)
        c.ok("audio retention forever: the old job's audio is kept", os.path.exists(kept_audio(a["job"])))
        c.ok("retranscription uses the kept audio", out2 == "Retranscribing." + a["job"], out2)
        st = wait_job_gone_or_failed(rig, a["job"])
        c.ok("the retranscribed text is delivered", st == "gone", st)
    rig.listener("Cancel", label="close-open-b")
    time.sleep(0.5)


def s_subscribe(rig, c):
    stack(rig, config=R.config_datom(audio_kept="Days.0", text_kept="Days.0"))
    outfile = R.RUN + "/subscription.txt"
    sub = rig.subscribe(outfile)
    time.sleep(1.0)
    a = record(rig, "sub-a", [3], label="sub-a")
    st = wait_job_gone_or_failed(rig, a["job"])
    c.ok("job delivered while subscribed", st == "gone", st)
    b = record(rig, "sub-b", [7], label="sub-b")  # prunes a: audio and text both expire, job is forgotten
    time.sleep(1.5)
    rig.listener("Cancel", label="close-b")
    time.sleep(1.0)
    sub.kill()
    lines = [x for x in open(outfile).read().splitlines() if x.strip()]
    c.note("subscriber frames", lines[:40], grade="observation")
    c.ok("subscription opened on the retained snapshot", lines and lines[0].startswith("Subscribed.Opened."), lines[:1])
    c.ok("removal of a Delivered job reaches the subscriber", any(("Delivered" in x) and a["job"] in x for x in lines), [x for x in lines if a["job"] in x])
    c.ok("forgetting a job reaches the subscriber as Forgotten.JobId", any(x.startswith("Subscribed.Changed.Forgotten.") and a["job"] in x for x in lines), [x for x in lines if a["job"] in x])
    # fold the frames as a subscriber would, and compare with the Jobs view
    view = fold(lines)
    m, _ = jobs_map(rig)
    c.ok("the subscriber fold equals the Jobs view", view == m, {"fold": view, "jobs": m})


def fold(lines):
    view = {}
    for x in lines:
        if x.startswith("Subscribed.Opened."):
            view = {jid: st for jid, st in JOB_RE.findall(x)}
        elif x.startswith("Subscribed.Lagged."):
            view = {jid: st for jid, st in JOB_RE.findall(x)}
        elif x.startswith("Subscribed.Changed.JobState."):
            for jid, st in JOB_RE.findall(x):
                if st == "Delivered":
                    view.pop(jid, None)
                else:
                    view[jid] = st
        elif x.startswith("Subscribed.Changed.Forgotten."):
            jid = x.rsplit(".", 1)[1]
            view.pop(jid, None)
    return view


def s_slow_subscriber_meta(rig, c):
    """A stopped subscriber past publisher capacity converges by replacement snapshot; no removal is missed."""
    stack(rig, capacity=2)
    outfile = R.RUN + "/meta-subscription.txt"
    sub = subprocess.Popen([R.BIN + "/listener-meta", "Subscribe"], env=R.clean_env(),
                           stdout=open(outfile, "w"), stderr=subprocess.STDOUT, stdin=subprocess.PIPE)
    time.sleep(1.0)
    os.kill(sub.pid, signal.SIGSTOP)
    big = "x" * 4000
    for i in range(120):
        rig.meta("Configure.AddTerm.«Term-%03d-%s»" % (i, big), label="flood-add")
    for i in range(0, 120, 3):
        rig.meta("Configure.RemoveTerm.«Term-%03d-%s»" % (i, big), label="flood-remove")
    os.kill(sub.pid, signal.SIGCONT)
    time.sleep(4.0)
    sub.kill()
    text = open(outfile).read()
    frames = [x for x in text.splitlines() if x.startswith("Subscribed.")]
    lagged = [x for x in frames if x.startswith("Subscribed.Lagged.")]
    rc, out, err = rig.meta("Configuration", label="final-config")
    terms_final = re.findall(r"Term-(\d{3})-x", out)
    snap_terms = []
    if lagged:
        snap_terms = re.findall(r"Term-(\d{3})-x", lagged[-1])
    c.note("subscriber frame counts", {"total": len(frames), "lagged": len(lagged), "changed": len([x for x in frames if x.startswith("Subscribed.Changed.")])}, grade="observation")
    c.ok("the stopped subscriber observed the capacity overflow as a replacement snapshot", len(lagged) >= 1, len(lagged))
    c.ok("final lagged snapshot equals the committed terms (no removal missed)", sorted(snap_terms) == sorted(terms_final) if lagged else False,
         {"snapshot": len(snap_terms), "final": len(terms_final)})
    c.ok("the final configuration retains exactly the terms not removed", sorted(terms_final) == sorted("%03d" % i for i in range(120) if i % 3 != 0), (len(terms_final)))


def s_slow_subscriber_ordinary(rig, c):
    """Overflow past publisher capacity with a removal in the middle: the subscriber converges and misses no removal."""
    stack(rig, config=R.config_datom(audio_kept="Days.0", text_kept="Days.0"), capacity=2)
    rc, out, err = rig.listener("Toggle", label="x-start")
    x_id = out.rsplit(".", 1)[1]
    time.sleep(0.5)
    rig.listener("Cancel", label="x-cancel")          # X: Cancelled, audio kept, live until the next start prunes it
    outfile = R.RUN + "/ordinary-subscription.txt"
    sub = rig.subscribe(outfile)
    time.sleep(1.0)
    os.kill(sub.pid, signal.SIGSTOP)
    for i in range(1100):                              # small changes until the socket buffer is full
        rig.meta("Remember.{ %d «flood %d» }" % (1700000000 + i, i), label="flood-remember")
    rc, out, err = rig.listener("Toggle", label="y-start")   # this start prunes X (Forgotten) and the flood
    y_id = out.rsplit(".", 1)[1]
    rig.listener("Cancel", label="y-cancel")
    time.sleep(0.5)
    os.kill(sub.pid, signal.SIGCONT)
    time.sleep(5)
    sub.kill()
    lines = [x for x in open(outfile).read().splitlines() if x.strip()]
    kinds = {k: sum(1 for x in lines if x.startswith(k)) for k in ("Subscribed.Opened", "Subscribed.Changed", "Subscribed.Lagged")}
    c.note("ordinary subscriber frame kinds", kinds, grade="observation")
    m, _ = jobs_map(rig)
    view = fold(lines)
    c.ok("the removal of X (Forgotten) during overflow is not missed", x_id not in view and x_id not in m, {"fold": list(view), "jobs": list(m)})
    c.ok("the subscriber converges on the Jobs view after overflow", view == m, {"fold": view, "jobs": m})
    c.ok("overflow reached the subscriber as a replacement snapshot or a full frame sequence", kinds["Subscribed.Lagged"] >= 1 or kinds["Subscribed.Changed"] > 1000, kinds)


def s_notify_typed(rig, c):
    """Typed Notify judged by the fixture service's own acceptance or refusal, not by stderr."""
    stack(rig, config=R.config_datom())
    rig.set_notify("accept")
    rig.set_http_plan(openai=[{"do": "status", "code": 500}])
    a = record(rig, "nt-accept", [3], label="nt-accept")
    wait_job_gone_or_failed(rig, a["job"])
    acc = [n for n in rig.notify_log() if n.get("event") == "notify" and n.get("decision") == "accepted"]
    c.ok("accepted: the service answered a u32 id and logged it as accepted", len(acc) >= 1 and isinstance(acc[-1].get("id"), int), acc[-1:])
    rig.set_notify("refuse")
    rig.set_http_plan(openai=[{"do": "status", "code": 500}])
    b = record(rig, "nt-refuse", [7], label="nt-refuse")
    wait_job_gone_or_failed(rig, b["job"])
    refused = [n for n in rig.notify_log() if n.get("event") == "notify" and n.get("decision") == "refused"]
    c.ok("refused: the service returned a D-Bus error and logged it as refused", len(refused) >= 1, refused[-1:])
    st = wait_job_gone_or_failed(rig, b["job"])
    c.ok("a refused notification does not stop the job settling (exhausted, Failed)", st == "Failed", st)
    rig.set_notify("badtype")
    rig.set_http_plan(openai=[{"do": "status", "code": 500}])
    d = record(rig, "nt-badtype", [9], label="nt-badtype")
    wait_job_gone_or_failed(rig, d["job"])
    bad = [n for n in rig.notify_log() if n.get("event") == "notify" and n.get("decision") == "badtype"]
    c.note("badtype (a string where a u32 is required) reached the service", len(bad), grade="observation")
    rig.set_notify("accept")
    c.note("notify log decisions", [(n.get("decision"), n.get("summary")) for n in rig.notify_log() if n.get("event") == "notify"], grade="observation")


def s_repeated_restart(rig, c):
    stack(rig)
    a = record(rig, "rr-a", [3], label="rr-a")
    st = wait_job_gone_or_failed(rig, a["job"])
    c.ok("baseline job delivered", st == "gone", st)
    before = desk_received(rig, "paste")
    for i in range(3):
        restart_nexus(rig)
        time.sleep(0.5)
        m, out = jobs_map(rig)
        c.ok("restart %d leaves Jobs unchanged and empty" % (i + 1), out == "JobsListed.[]", out)
    c.ok("restarts do not replay any delivered text", len(desk_received(rig, "paste")) == len(before) == 1)
    rc, out, err = rig.listener("History", label="history-after-restarts")
    c.ok("the retained text survives repeated restarts", "utterance 3" in out, out[:200])


def s_unsubmitted(rig, c):
    """Translator down while a recording runs: the recording continues in the Nexus; the end press that could not be submitted is refused Unsubmitted; once the translator is back, the same end press ends the same job and it is delivered once."""
    stack(rig)
    rc, out, err = rig.listener("Toggle", label="uns:start")
    jid = out.rsplit(".", 1)[1]
    wav = make_wav(rig, "utt-uns.wav", [3])
    A.play_cmd(R.PA, wav, background=True)
    time.sleep(max(4.0, 2.2 * wav_seconds(wav)))
    rig.kill_translator(signal.SIGKILL)
    rc, out, err = rig.listener("Toggle", label="uns:end-while-translator-down")
    c.ok("the end press cannot reach the dead translator and is reported Unobserved.Unsubmitted",
         rc != 0 and "Unobserved.Unsubmitted" in err, {"rc": rc, "out": out, "err": err})
    rig.start_translator()
    m, out = jobs_map(rig)
    c.ok("the Nexus still holds the job Recording after the failed submit", m.get(jid) == "Recording", out)
    rc, out, err = rig.listener("Toggle", label="uns:end-after-translator-back")
    c.ok("the end press after the translator returns ends the same job", out == "Toggled.Ended." + jid, out)
    st = wait_job_gone_or_failed(rig, jid)
    c.ok("the job is heard and delivered once", st == "gone" and len(desk_received(rig, "paste")) == 1, (st, len(desk_received(rig, "paste"))))
    k = opus_check(kept_audio(jid))
    c.ok("the audio kept across the translator outage is decodable and hears the tone", k.get("opusinfo_rc") == 0 and k.get("ids") == [3], k.get("ids"))


def s_subscribe_race(rig, c):
    """A subscriber opened while changes are being committed: snapshot plus committed changes leave no gap."""
    import threading
    stack(rig, config=R.config_datom(audio_kept="Days.0", text_kept="Days.0"))
    outfile = R.RUN + "/race-subscription.txt"

    def flood():
        for i in range(40):
            rig.listener("Toggle", label="race-toggle")
            rig.listener("Cancel", label="race-cancel")

    t = threading.Thread(target=flood)
    t.start()
    time.sleep(0.3)
    sub = rig.subscribe(outfile)
    t.join()
    time.sleep(2.0)
    sub.kill()
    lines = [x for x in open(outfile).read().splitlines() if x.strip()]
    c.note("race subscriber frame kinds", {k: sum(1 for x in lines if x.startswith(k)) for k in ("Subscribed.Opened", "Subscribed.Changed", "Subscribed.Lagged")}, grade="observation")
    m, _ = jobs_map(rig)
    view = fold(lines)
    c.ok("a subscriber opened mid-flood folds to exactly the committed Jobs view", view == m, {"fold": len(view), "jobs": len(m)})
    c.ok("the subscription opened with a snapshot", lines and lines[0].startswith("Subscribed.Opened."), lines[:1])


def s_failure_limits(rig, c):
    """Backend timeout and redirect refusal, both judged by the fixture's own record."""
    stack(rig)
    rig.meta("Configure.Backend.{ fixture OpenAiTranscription http://127.0.0.1:%d fixture-model fixture-key en «x» 1 }" % R.OPENAI_PORT)
    rig.set_http_plan(openai=[{"do": "delay", "ms": 4000, "then": {"do": "ok"}}])
    t = record(rig, "lim-timeout", [3], label="timeout")
    st = wait_job_gone_or_failed(rig, t["job"], timeout=40)
    c.ok("a backend slower than its timeout fails the job", st == "Failed", st)
    c.ok("the timeout is notified as the deadline reached", len(wait_notify(rig, "deadline was reached")) >= 1)
    rig.meta("Configure.Backend.{ fixture OpenAiTranscription http://127.0.0.1:%d fixture-model fixture-key en «x» 20 }" % R.OPENAI_PORT)
    rig.set_http_plan(openai=[{"do": "redirect", "location": "/elsewhere"}])
    rd = record(rig, "lim-redirect", [5], label="redirect")
    st = wait_job_gone_or_failed(rig, rd["job"])
    c.ok("a redirect is not followed: the job fails with the 302 status", st == "Failed", st)
    c.ok("the redirect is notified as HTTP 302", len(wait_notify(rig, "HTTP 302")) >= 1)
    targets = [a for a in arrivals(rig) if a.get("path") not in (None, "/v1/audio/transcriptions")]
    c.ok("no request reached the redirect target", len(targets) == 0, targets)


def s_delivery_modes(rig, c):
    """Paste failure falls back to copy; Copied and Kept delivery; ToggleCustom overrides the delivery."""
    stack(rig)
    rig.set_desk(paste="fail", copy="ok")
    a = record(rig, "dm-fallback", [3], label="fallback")
    st = wait_job_gone_or_failed(rig, a["job"])
    c.ok("a paste program that fails falls back to the copy program", st == "gone" and [d["text"] for d in desk_received(rig, "copy")] == [expected_text([3])],
         {"copy": [d.get("text") for d in desk_received(rig, "copy")], "state": st})
    c.ok("the paste failure is notified with the program's status", len(wait_notify(rig, "ended with exit status: 3")) >= 1 or len(wait_notify(rig, "Paste outcome")) >= 1,
         [n for n in rig.notify_log() if "Paste" in json.dumps(n)])
    rig.set_desk(paste="ok", copy="ok")
    rig.meta("Configure.Delivery.Copied")
    b = record(rig, "dm-copied", [7], label="copied")
    st = wait_job_gone_or_failed(rig, b["job"])
    c.ok("Copied delivery uses only the copy program", st == "gone" and [d["text"] for d in desk_received(rig, "copy")][-1:] == [expected_text([7])]
         and len(desk_received(rig, "paste")) == 1, {"paste": len(desk_received(rig, "paste")), "copy": len(desk_received(rig, "copy"))})
    rig.meta("Configure.Delivery.Kept")
    k = record(rig, "dm-kept", [9], label="kept")
    st = wait_job_gone_or_failed(rig, k["job"])
    c.ok("Kept delivery makes no desktop effect and the job settles", st == "gone" and len(desk_received(rig)) == 3, {"receipts": len(desk_received(rig)), "state": st})
    rc, out, err = rig.listener("RecallJob." + k["job"], label="recall-kept")
    c.ok("Kept text is retained and recallable", out.startswith("JobRecalled.") and "utterance 9" in out, out)
    rig.meta("Configure.Delivery.Pasted")
    rc, out, err = rig.listener("ToggleCustom.{ Some.Copied Some.[ fixture ] }", label="custom-probe")
    c.note("ToggleCustom options probe", (rc, out, err), grade="observation")
    if out.startswith("CustomToggled.Began."):
        cid = out.rsplit(".", 1)[1]
        wav = make_wav(rig, "utt-dm-custom.wav", [12])
        A.play_cmd(R.PA, wav, background=True)
        time.sleep(max(4.0, 2.2 * wav_seconds(wav)))
        rig.listener("Toggle", label="custom-end")
        st = wait_job_gone_or_failed(rig, cid)
        c.ok("ToggleCustom with a Copied override copies instead of pasting", st == "gone" and [d["text"] for d in desk_received(rig, "copy")][-1:] == [expected_text([12])], st)


def s_refusals(rig, c):
    """Typed refusals of the ordinary and meta surfaces, each refused without effect."""
    stack(rig)
    rc, out, err = rig.listener("ToggleCustom.{ None Some.[ nosuch ] }", label="unknown-backend")
    c.ok("a Toggle naming an unknown backend is refused UnknownBackend and starts nothing", out == "Refused.UnknownBackend.nosuch", (out, err))
    rc, out, err = rig.meta("Configure.Order.[ nosuch ]", label="order-unknown")
    c.ok("an Order naming an unknown backend is refused UnknownBackend by meta",
         rc == 0 and out == "Refused.UnknownBackend.nosuch", (rc, out, err))
    rig.meta("Configure.Order.[ ]", label="order-empty")
    rc, out, err = rig.listener("Toggle", label="no-backend")
    c.ok("a Toggle with no backend order is refused NoBackend", out == "Refused.NoBackend", (out, err))
    rig.meta("Configure.Order.[ fixture ]", label="order-restore")
    rc, out, err = rig.listener("Recall", label="recall-empty")
    c.ok("Recall with nothing kept is refused NothingKept", out == "Refused.NothingKept", out)
    rc, out, err = rig.listener("RecallJob.zzzz", label="recall-unknown")
    c.ok("RecallJob of an unknown job is refused NoSuchJob", out == "Refused.NoSuchJob.zzzz", out)
    rc, out, err = rig.listener("Jobs", label="jobs-after-refusals")
    c.ok("refusals leave no job behind", out == "JobsListed.[]", out)
    rc, out, err = rig.meta("Configure.AudioKept.Days.-1", label="invalid-retention")
    c.ok("an invalid retention is refused InvalidValue and the setting is unchanged",
         rc == 0 and out == "Refused.InvalidValue", (rc, out, err))
    rc, out, err = rig.meta("Configuration", label="configuration-after")
    c.ok("the configuration still holds the valid retention", "Forever Forever" in out, out[:200])


SCENARIOS = {
    "happy_openai": lambda r, c: s_happy_openai(r, c),
    "happy_gemini": lambda r, c: s_happy_gemini(r, c),
    "ordering_asrecorded": lambda r, c: s_ordering(r, c, "AsRecorded"),
    "ordering_asready": lambda r, c: s_ordering(r, c, "AsReady"),
    "blocker": lambda r, c: s_blocker(r, c),
    "failure": lambda r, c: s_failure(r, c),
    "uncertain_paste_translator": lambda r, c: s_restart_paste(r, c, "translator"),
    "uncertain_paste_nexus": lambda r, c: s_restart_paste(r, c, "nexus"),
    "restart_hearing": lambda r, c: s_restart_hearing(r, c),
    "partial_capture": lambda r, c: s_partial_capture(r, c),
    "cancel_capture_failure": lambda r, c: s_cancel_and_capture_failure(r, c),
    "retention_audio": lambda r, c: s_retention(r, c, "audio"),
    "retention_text": lambda r, c: s_retention(r, c, "text"),
    "subscribe_fold": lambda r, c: s_subscribe(r, c),
    "slow_subscriber_meta": lambda r, c: s_slow_subscriber_meta(r, c),
    "slow_subscriber_ordinary": lambda r, c: s_slow_subscriber_ordinary(r, c),
    "notify_typed": lambda r, c: s_notify_typed(r, c),
    "repeated_restart": lambda r, c: s_repeated_restart(r, c),
    "unsubmitted": lambda r, c: s_unsubmitted(r, c),
    "subscribe_race": lambda r, c: s_subscribe_race(r, c),
    "failure_limits": lambda r, c: s_failure_limits(r, c),
    "delivery_modes": lambda r, c: s_delivery_modes(r, c),
    "refusals": lambda r, c: s_refusals(r, c),
    "restart_settles_paste_nexus": lambda r, c: s_settled_paste_never_replayed(r, c, "nexus"),
    "restart_settles_paste_translator": lambda r, c: s_settled_paste_never_replayed(r, c, "translator"),
    "recall_after_unanswered_paste": lambda r, c: s_recall_after_unanswered_paste(r, c),
    "restart_drains_later_jobs": lambda r, c: s_restart_drains_later_jobs(r, c),
}


def run(name):
    """Run one scenario in its own process, so its run directory is its own."""
    env = dict(os.environ)
    env["LISTENER_RIG_SCENARIO"] = name
    t0 = time.time()
    p = subprocess.run([R.PY, "-I", __file__, "--one", name], env=env, timeout=900)
    return {"scenario": name, "rc": p.returncode, "seconds": round(time.time() - t0, 1)}


def run_one(name):
    rig = R.Rig(name)
    c = Check(name)
    print("######## scenario %s" % name, flush=True)
    try:
        SCENARIOS[name](rig, c)
    except Exception:
        import traceback
        c.ok("scenario ran to completion", False, traceback.format_exc()[-1500:])
    finally:
        rig.teardown()
    return c.finish()


if __name__ == "__main__":
    if sys.argv[1:2] == ["--list"]:
        print("\n".join(SCENARIOS))
    elif sys.argv[1:2] == ["--one"]:
        R.check_tools()
        record = run_one(sys.argv[2])
        sys.exit(0 if record["failed"] == 0 else 1)
    else:
        R.check_tools()
        unknown = [name for name in sys.argv[1:] if name not in SCENARIOS]
        if unknown:
            sys.exit("unknown scenarios: " + ", ".join(unknown))
        failed = False
        for name in sys.argv[1:]:
            result = run(name)
            print(json.dumps(result), flush=True)
            failed = failed or result["rc"] != 0
        sys.exit(1 if failed else 0)
