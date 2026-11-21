#!/bin/sh
# Runs the Memory oracles in tests/ against a candidate listener-memory crate, by the patched-copy
# method: a copy of this repository is made, its Cargo.toml is patched to take listener-memory from
# the candidate's source tree, and cargo test runs in that copy. This repository's own Cargo.toml,
# its pins and its lock are not changed.
#
#   runtime/memory-candidate.sh <candidate listener source tree>
#
# LISTENER_BUILDER=<ssh host> runs the copy on that host instead (heavy builds belong on the
# builder); the repository and the candidate tree are copied there first.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
CAND=$(cd "${1:?usage: runtime/memory-candidate.sh <candidate listener source tree>}" && pwd)

patch_cargo() {
  # $1: the directory of the copied tests crate; $2: the directory of the copied candidate tree
  cat >> "$1/Cargo.toml" <<EOF

[patch."https://github.com/LiGoldragon/listener"]
listener-memory = { path = "$2/crates/listener-memory" }
EOF
}

if [ -n "${LISTENER_BUILDER:-}" ]; then
  DEST=/tmp/offload/listener-tests-memory-candidate
  ssh -o BatchMode=yes "$LISTENER_BUILDER" "rm -rf $DEST && mkdir -p $DEST/tests $DEST/listener"
  rsync -a --exclude target --exclude .git -e "ssh -o BatchMode=yes" "$REPO/" "$LISTENER_BUILDER:$DEST/tests/"
  rsync -a --exclude target --exclude .git -e "ssh -o BatchMode=yes" "$CAND/" "$LISTENER_BUILDER:$DEST/listener/"
  ssh -o BatchMode=yes "$LISTENER_BUILDER" "cat >> $DEST/tests/Cargo.toml" <<EOF

[patch."https://github.com/LiGoldragon/listener"]
listener-memory = { path = "$DEST/listener/crates/listener-memory" }
EOF
  ssh -o BatchMode=yes "$LISTENER_BUILDER" "cd $DEST/tests && cargo test --features datom --no-fail-fast"
else
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/memory-candidate-XXXXXX")
  trap 'rm -rf "$WORK"' EXIT
  mkdir -p "$WORK/tests" "$WORK/listener"
  (cd "$REPO" && tar --exclude=./.git --exclude=./target -cf - .) | (cd "$WORK/tests" && tar -xf -)
  (cd "$CAND" && tar --exclude=./.git --exclude=./target -cf - .) | (cd "$WORK/listener" && tar -xf -)
  patch_cargo "$WORK/tests" "$WORK/listener"
  cd "$WORK/tests" && cargo test --features datom --no-fail-fast
fi
