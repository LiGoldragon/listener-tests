# listener-tests

Independent Listener tests: Memory oracles (`tests/scenarios.rs`) and runtime scenarios that run
the built Listener binaries end to end (`runtime/`).

## Memory oracles

```sh
cargo test --features datom --no-fail-fast        # against the pinned listener-memory (Cargo.toml)
runtime/memory-candidate.sh <candidate listener source tree>   # against a candidate listener-memory
```

The candidate form copies the repository and the candidate tree, patches the copy to take
`listener-memory` from the candidate, and runs the same suite. The oracles for `Recover` assert
that an outstanding delivery claim is settled as Kept on every open and is never offered again;
an explicit `Recall` or `RecallJob` answers with the settled text and starts no handoff of its
own. The updated suite needs the candidate kernel (`Input::Advance`, `Input::AbortStart`).

## Runtime scenarios

```sh
LISTENER_BIN_DIR=<dir> PULSE_BIN_DIR=<dir> runtime/run.sh [NAME...]
```

See `runtime/README.md` for the tools, the isolation, and what each scenario proves.
