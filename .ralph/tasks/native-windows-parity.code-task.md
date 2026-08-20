---
status: done
spec: ../specs/native-windows-parity.spec.md
---

# Implement native Windows parity

Implement the approved native Windows parity specification without changing
existing Unix targets or orchestration semantics.

## Required gates

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc`
- `cargo test`
- `cargo test -p ralph-core --features recording --test smoke_runner`
- Native Windows CI runs core and adapter tests plus a release build.

## Verification

- `cargo test --quiet` — passed on native Windows.
- `cargo test -p ralph-core --features recording --test smoke_runner --quiet` — 52 passed.
- `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc` — passed.
- `cargo fmt --all -- --check` — passed.
