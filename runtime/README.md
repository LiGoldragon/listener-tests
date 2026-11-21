# Listener runtime scenarios

Scenarios that run the built Listener binaries end to end, each in a private stack: a private
session bus, a PulseAudio null-sink daemon, loopback OpenAI and Gemini fixtures, a notification
fixture, and a desktop sink that records what the paste and copy programs receive. The live
listener, the live Nexus and the real bus and Pulse daemon are never addressed.

## One command

```sh
LISTENER_BIN_DIR=<dir> PULSE_BIN_DIR=<dir> runtime/run.sh            # every scenario
LISTENER_BIN_DIR=<dir> PULSE_BIN_DIR=<dir> runtime/run.sh NAME...    # named scenarios
```

- `LISTENER_BIN_DIR` holds `listener`, `listener-meta`, `listener-nexus` and `listener-translator`,
  built from the candidate source (the packaged `bin/` of the candidate build works).
- `PULSE_BIN_DIR` holds `pulseaudio`, `paplay`, `parecord` and `pactl` (PulseAudio 17.0 was used).
- Also on `PATH`: `python3` (standard library only), `dbus-daemon`, `opusinfo` (opus-tools), and
  `ffmpeg` or `opusdec` for the independent oracle.
- `run.sh` exits 0 only when every judged check of every named scenario passes. It prints the
  work directory and `results.jsonl`, one JSON line per scenario with each check's name, result and
  detail.
- Optional: `LISTENER_RIG_ROOT` (the work directory: logs, receipts, fixture logs; default a fresh
  directory under `TMPDIR`), `LISTENER_RIG_SOCK_ROOT` (where the short socket directory is made;
  default `XDG_RUNTIME_DIR`), `PYTHON`.

Each scenario runs in its own process and is torn down before the next starts: fixed loopback ports
are used, so scenarios never overlap.

## What is real, and what is a stand-in

| Part | Grade | Note |
| --- | --- | --- |
| Nexus, Translator, `listener`, `listener-meta`, the Memory state machine, delivery claims, recovery | exercised | Built binaries, real processes, real sockets |
| Capture and Opus encoding, the kept file | exercised for encoding | The source is a private null-sink monitor, not a microphone |
| OpenAI and Gemini HTTP | client exercised | The service is a protocol-shaped loopback fixture |
| The transcription result | stand-in | The fixture decodes the uploaded Opus with an independent oracle and answers with the utterance ids |
| The credential | stand-in | A program prints a fixed fixture key. It is not a credential |
| Paste and copy | spawning and standard input exercised | The desktop is a sink that writes receipts; a gate holds it blocked on demand |
| Notification | the D-Bus call exercised | The service is a fixture that accepts, refuses or answers with a wrong type |
| Process death | exercised | `SIGKILL`, not power loss |

The judges are not the Listener's own output: the kept file (`opusinfo` and the oracle), the
uploaded bytes (sha256), the fixture request and notify logs, the desk receipts, the Jobs view,
the subscriber's frames, and the store file.

## Scenarios

| Scenario | Proves |
| --- | --- |
| `happy_openai` | Toggle, kept decodable Ogg Opus, one transcription request with the expected shape, persisted text, focused delivery once |
| `happy_gemini` | Gemini request shape, key header, inline audio byte-identical to the kept file |
| `ordering_asrecorded` | A second recording begins during the first transcription; AsRecorded delivers in recording order |
| `ordering_asready` | Both recordings are delivered; readiness order is observed, not asserted |
| `blocker` | Jobs waiting behind an occupied claim drain in recording order once the blocker is released |
| `failure` | Fallback after HTTP 500, exhaustion keeps audio and Retranscribe delivers, zero candidates, non-loopback HTTP refused, malformed reply, missing credential |
| `uncertain_paste_translator` | The Translator is killed mid-paste: the paste is settled Kept, no copy fallback, the orphaned paste completes once |
| `uncertain_paste_nexus` | The Nexus is killed mid-paste: the claim is settled on Open, Jobs is empty, no replay |
| `restart_settles_paste_nexus` | An unanswered paste is never replayed: three Nexus opens, each with the paste still blocked or released; the receipts stay at one invocation |
| `restart_settles_paste_translator` | The same, with the Translator killed mid-paste and then three Nexus opens |
| `recall_after_unanswered_paste` | An explicit RecallJob and an explicit Recall each hand the retained text to the desk exactly once; a restart between them adds none |
| `restart_drains_later_jobs` | The Waiting job behind an unanswered claim is delivered once after the open; the settled job is not replayed |
| `restart_hearing` | The Nexus is killed mid-transcription: Open re-hears the job and delivers once; the stale reply adds nothing |
| `partial_capture` | The Nexus is killed mid-recording: the file fails `opusinfo` before Open, is repaired on Open, and its text is delivered once |
| `cancel_capture_failure` | A missing source is refused; Cancel keeps job and audio; Retranscribe and its refusals |
| `retention_audio`, `retention_text` | Audio and text expire independently; expired audio is removed at the next start; expired text is not recallable |
| `subscribe_fold`, `subscribe_race` | A subscriber's fold equals the Jobs view, including after a subscription opened mid-flood |
| `slow_subscriber_meta`, `slow_subscriber_ordinary` | A stopped subscriber past capacity converges by a replacement snapshot and misses no removal |
| `notify_typed` | Accepted, refused and wrong-type notifications are judged by the service's own log; a refused notification does not stop the job settling |
| `repeated_restart` | Three Open cycles on an idle store: no replay, history intact |
| `unsubmitted` | The Translator is down while a recording runs: the end press is refused Unsubmitted, and after it returns the same end press delivers once |
| `failure_limits` | A backend slower than its timeout fails; a redirect is not followed |
| `delivery_modes` | Paste failure falls back to copy; Copied uses only copy; Kept makes no desktop effect and the text stays recallable |
| `refusals` | Typed refusals (unknown backend, no backend, nothing kept, no such job, invalid value) with no effect |

## Isolation

Sockets live in a fresh directory of mode 0700 under `LISTENER_RIG_SOCK_ROOT`, named `lr-*`, and
it is removed when `run.sh` exits. Every client call carries its own `XDG_RUNTIME_DIR`,
`XDG_CONFIG_HOME` and `HOME`. The rig's dummy key is `rig-dummy-key-not-a-credential`; it protects
nothing and is only the fixture services' expectation. The Pulse daemon strips inherited `PULSE_*`,
`XDG_*` and `DBUS_*` variables and listens only on its own socket. The loopback fixtures listen on
127.0.0.1 ports 18123 and 18124.

## Limits

The rig does not exercise a real microphone, the real desktop, real OpenAI or Gemini services,
real unplug or sleep, power loss, or long runs. Those need a consented trial and are not part of
this command.
