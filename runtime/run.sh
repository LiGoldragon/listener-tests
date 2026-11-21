#!/bin/sh
# Runs the Listener runtime scenarios against built binaries, each in a private stack.
#
#   LISTENER_BIN_DIR=<dir> PULSE_BIN_DIR=<dir> runtime/run.sh [SCENARIO...]
#
# With no scenario named, every scenario runs. Exit status 0 means every judged check passed.
# The run's logs, receipts and results are kept under LISTENER_RIG_ROOT (a fresh directory,
# printed at the end, unless set); the sockets live in a short directory that is removed at exit.
# See runtime/README.md for what each scenario proves and the tools it needs.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
PYTHON=${PYTHON:-python3}

: "${LISTENER_BIN_DIR:?set LISTENER_BIN_DIR to the directory holding listener, listener-meta, listener-nexus and listener-translator}"
: "${PULSE_BIN_DIR:?set PULSE_BIN_DIR to the directory holding pulseaudio, paplay, parecord and pactl (PulseAudio 17.0 was used)}"
export LISTENER_BIN_DIR PULSE_BIN_DIR

if [ -z "${LISTENER_RIG_ROOT:-}" ]; then
  LISTENER_RIG_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/listener-rig-XXXXXX")
fi
mkdir -p "$LISTENER_RIG_ROOT"

# A Unix socket path must stay under 108 bytes, so the sockets are not under the work directory;
# they go under the per-user runtime directory unless LISTENER_RIG_SOCK_ROOT names another short one.
SOCK=$(mktemp -d "${LISTENER_RIG_SOCK_ROOT:-${XDG_RUNTIME_DIR:-/tmp}}/lr-XXXXXX")
chmod 700 "$SOCK"
trap 'rm -rf "$SOCK"' EXIT INT TERM

export LISTENER_RIG_WORK="$LISTENER_RIG_ROOT" LISTENER_RIG_SOCK="$SOCK"

if [ $# -eq 0 ]; then
  set -- $("$PYTHON" -I "$HERE/rig/scenarios.py" --list)
fi

status=0
"$PYTHON" -I "$HERE/rig/scenarios.py" "$@" || status=$?
echo "work directory: $LISTENER_RIG_ROOT"
echo "results: $LISTENER_RIG_ROOT/results.jsonl"
exit "$status"
