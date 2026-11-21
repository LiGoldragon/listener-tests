#!/usr/bin/env -S python3 -I
"""fixture-audio: synthetic utterance generator and an independent oracle.

Generator: each utterance id is a sequence of 3 tones (0.4 s each, 0.1 s apart),
the three frequencies distinct and drawn from a 16-frequency grid; utterances are
separated by 1.0 s of silence.  Oracle: decodes any audio file with ffmpeg (or
opusdec), runs a Hann-windowed Goertzel analysis on the grid frequencies and
reports the utterance ids heard.  Python 3 stdlib only.

  fixture_audio.py gen OUT.wav (--ids 3,7,12 | --count N) [--rate 16000] [--each DIR]
  fixture_audio.py decode FILE [--json]
  fixture_audio.py selftest
"""
import argparse
import json
import math
import operator
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import wave

# ---- the pattern definition (shared by generator and oracle) ----
GRID = [500 + 150 * k for k in range(16)]          # Hz: 500 .. 2750
TONE_S = 0.4
TONE_GAP_S = 0.1
UTT_GAP_S = 1.0
LEAD_S = 0.5
AMPLITUDE = 0.5
N_TRIPLES = 16 * 15 * 14                            # 3360 distinct ids: 0..3359
_SCRAMBLE = 1237                                    # coprime with 3360; spreads neighbouring ids apart


def triple_of(uid):
    """Grid indices (a, b, c), pairwise distinct, for utterance id uid."""
    if not 0 <= uid < N_TRIPLES:
        raise ValueError("utterance id out of range 0..%d: %r" % (N_TRIPLES - 1, uid))
    idx = (uid * _SCRAMBLE) % N_TRIPLES
    a, r = divmod(idx, 15 * 14)
    b, c = divmod(r, 14)
    rest = [k for k in range(16) if k != a]
    b_k = rest[b]
    rest = [k for k in rest if k != b_k]
    return (a, b_k, rest[c])


_ID_OF = {triple_of(i): i for i in range(N_TRIPLES)}


def freqs_of(uid):
    return [GRID[k] for k in triple_of(uid)]


# ---- generator ----
def _tone(freq, rate, dur, fade=0.01):
    n = int(round(rate * dur))
    nf = int(rate * fade)
    out = []
    for i in range(n):
        env = 1.0
        if i < nf:
            env = i / nf
        elif i >= n - nf:
            env = (n - 1 - i) / nf
        out.append(AMPLITUDE * env * math.sin(2 * math.pi * freq * i / rate))
    return out


def synth(ids, rate=16000):
    """Samples (floats) for the utterances in order."""
    sil = lambda s: [0.0] * int(round(rate * s))
    x = sil(LEAD_S)
    for uid in ids:
        for j, f in enumerate(freqs_of(uid)):
            x += _tone(f, rate, TONE_S)
            if j < 2:
                x += sil(TONE_GAP_S)
        x += sil(UTT_GAP_S)
    return x


def write_wav(path, samples, rate=16000):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32767)) for s in samples))


# ---- oracle ----
WIN = 1600      # samples at 16 kHz = 0.1 s
HOP = 800       # 0.05 s
RATE = 16000
MIN_RMS = 0.02
MIN_PURITY = 0.5
MIN_RUN_WINDOWS = 5
MAX_TONE_GAP_S = 0.4


def _pcm16k(path):
    """Decode any audio file to 16 kHz mono float samples via ffmpeg, else opusdec."""
    if shutil.which("ffmpeg"):
        p = subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-i", path, "-f", "s16le",
                            "-ac", "1", "-ar", str(RATE), "-"], capture_output=True)
        if p.returncode != 0:
            raise RuntimeError("ffmpeg failed on %s: %s" % (path, p.stderr.decode(errors="replace")))
        raw = p.stdout
    elif shutil.which("opusdec"):
        p = subprocess.run(["opusdec", "--quiet", "--rate", str(RATE), "--force-wav", path, "-"],
                           capture_output=True)
        if p.returncode != 0:
            raise RuntimeError("opusdec failed on %s" % path)
        raw = p.stdout[44:]
    else:
        raise RuntimeError("neither ffmpeg nor opusdec is on PATH")
    n = len(raw) // 2
    return [s / 32768.0 for s in struct.unpack("<%dh" % n, raw[: n * 2])]


def _tables():
    w = [0.5 - 0.5 * math.cos(2 * math.pi * i / (WIN - 1)) for i in range(WIN)]
    sw = sum(w)
    sw2 = sum(v * v for v in w)
    k = sw * sw / (2 * sw2)
    cos_t, sin_t = [], []
    for f in GRID:
        cos_t.append([w[i] * math.cos(2 * math.pi * f * i / RATE) for i in range(WIN)])
        sin_t.append([w[i] * math.sin(2 * math.pi * f * i / RATE) for i in range(WIN)])
    return w, k, cos_t, sin_t


def analyse(samples):
    w, kconst, cos_t, sin_t = _tables()
    labels = []
    nwin = 0 if len(samples) < WIN else (len(samples) - WIN) // HOP + 1
    for n in range(nwin):
        seg = samples[n * HOP: n * HOP + WIN]
        rms = math.sqrt(sum(v * v for v in seg) / WIN)
        if rms < MIN_RMS:
            labels.append(None)
            continue
        e = sum((a * b) ** 2 for a, b in zip(seg, w))
        best, best_p = None, 0.0
        for fi in range(len(GRID)):
            re = sum(map(operator.mul, seg, cos_t[fi]))
            im = sum(map(operator.mul, seg, sin_t[fi]))
            p = (re * re + im * im) / (e * kconst) if e > 0 else 0.0
            if p > best_p:
                best, best_p = fi, p
        labels.append(best if best_p >= MIN_PURITY else None)
    # collapse into runs of the same tone
    runs = []
    i = 0
    while i < len(labels):
        if labels[i] is None:
            i += 1
            continue
        j = i
        while j + 1 < len(labels) and labels[j + 1] == labels[i]:
            j += 1
        if j - i + 1 >= MIN_RUN_WINDOWS:
            runs.append((labels[i], i * HOP / RATE, (j * HOP + WIN) / RATE))
        i = j + 1
    # match three consecutive runs close in time to a known id
    utts, partial = [], []
    k = 0
    while k < len(runs):
        if k + 2 < len(runs):
            a, b, c = runs[k], runs[k + 1], runs[k + 2]
            if (b[1] - a[2] <= MAX_TONE_GAP_S and c[1] - b[2] <= MAX_TONE_GAP_S
                    and (a[0], b[0], c[0]) in _ID_OF):
                utts.append({"id": _ID_OF[(a[0], b[0], c[0])], "start": round(a[1], 2),
                             "end": round(c[2], 2)})
                k += 3
                continue
        partial.append({"freq": GRID[runs[k][0]], "start": round(runs[k][1], 2),
                        "end": round(runs[k][2], 2)})
        k += 1
    return {"ids": [u["id"] for u in utts], "utterances": utts, "unmatched_tones": partial,
            "duration": round(len(samples) / RATE, 2)}


def decode_file(path):
    return analyse(_pcm16k(path))


def decode_ids(path):
    return decode_file(path)["ids"]


# ---- selftest ----
def _selftest():
    ok = True
    def check(name, cond, extra=""):
        nonlocal ok
        print(("PASS " if cond else "FAIL ") + name + (" " + str(extra) if extra and not cond else ""))
        ok = ok and cond
    # distinctness of the pattern
    check("triples distinct for all ids", len(_ID_OF) == N_TRIPLES)
    check("tones within an utterance distinct", all(len(set(triple_of(i))) == 3 for i in range(N_TRIPLES)))
    with tempfile.TemporaryDirectory() as d:
        ids = [3, 7, 12, 0, 41, 1]
        wav = os.path.join(d, "u.wav")
        write_wav(wav, synth(ids))
        check("wav oracle exact", decode_ids(wav) == ids, decode_file(wav))
        ogg = os.path.join(d, "ffmpeg.ogg")
        subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-i", wav, "-c:a", "libopus", "-b:a", "16k",
                        "-application", "voip", ogg], check=True)
        check("ffmpeg libopus 16k voip", decode_ids(ogg) == ids, decode_file(ogg))
        if shutil.which("opusenc"):
            ogg2 = os.path.join(d, "opusenc.opus")
            subprocess.run(["opusenc", "--quiet", "--bitrate", "12", wav, ogg2], check=True)
            check("opusenc 12 kbps", decode_ids(ogg2) == ids, decode_file(ogg2))
        # 48 kHz input, as a real capture would be
        wav48 = os.path.join(d, "u48.wav")
        write_wav(wav48, synth([9, 8], 48000), 48000)
        ogg3 = os.path.join(d, "u48.ogg")
        subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-i", wav48, "-c:a", "libopus", ogg3], check=True)
        check("48 kHz source", decode_ids(ogg3) == [9, 8], decode_file(ogg3))
        # negative: silence, and a wrong id is not reported
        wsil = os.path.join(d, "s.wav")
        write_wav(wsil, [0.0] * 16000 * 2)
        check("silence -> no ids", decode_ids(wsil) == [])
        # a cut utterance is not reported as complete
        full = synth([5])
        cut = os.path.join(d, "cut.wav")
        write_wav(cut, full[: int(16000 * (LEAD_S + 2 * TONE_S + TONE_GAP_S + 0.1))])
        r = decode_file(cut)
        check("truncated utterance not reported", r["ids"] == [] and len(r["unmatched_tones"]) >= 1, r)
        # the oracle must distinguish ids: ids 5 and 6 differ
        w56 = os.path.join(d, "w56.wav")
        write_wav(w56, synth([6]))
        check("id 6 is not id 5", decode_ids(w56) == [6])
        # order matters
        r1 = os.path.join(d, "r.wav")
        write_wav(r1, synth([2, 1]))
        check("order preserved", decode_ids(r1) == [2, 1])
    print("fixture_audio selftest:", "OK" if ok else "FAILED")
    return 0 if ok else 1


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("out")
    g.add_argument("--ids")
    g.add_argument("--count", type=int)
    g.add_argument("--rate", type=int, default=16000)
    g.add_argument("--each", help="also write one WAV per id into this directory (utt-<id>.wav)")
    d = sub.add_parser("decode")
    d.add_argument("file")
    d.add_argument("--json", action="store_true", help="full report (default prints ids)")
    sub.add_parser("selftest")
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        if (a.ids is None) == (a.count is None):
            ap.error("give exactly one of --ids and --count")
        ids = [int(s) for s in a.ids.split(",")] if a.ids else list(range(a.count))
        write_wav(a.out, synth(ids, a.rate), a.rate)
        if a.each:
            os.makedirs(a.each, exist_ok=True)
            for i in ids:
                write_wav(os.path.join(a.each, "utt-%d.wav" % i), synth([i], a.rate), a.rate)
        print(json.dumps({"wav": a.out, "ids": ids, "rate": a.rate}))
    elif a.cmd == "decode":
        r = decode_file(a.file)
        print(json.dumps(r) if a.json else ",".join(map(str, r["ids"])))
    else:
        return _selftest()
    return 0


if __name__ == "__main__":
    sys.exit(main())
