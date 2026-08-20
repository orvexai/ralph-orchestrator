---
status: approved
created: 2026-08-20
related:
  - feature-parity.spec.md
  - interactive-mode
  - multi-loop-concurrency.code-task.md
  - fix-termination-cleanup.code-task.md
---

# Native Windows Parity Specification

## Goal

Make the Rust CLI a supported native Windows application while preserving all
existing Linux and macOS behavior and release artifacts.

## Scope

Windows parity includes:

- Building and testing the complete Cargo workspace for
  `x86_64-pc-windows-msvc`.
- Normal single-loop execution, resume/stop/restart, diagnostics, memories,
  tasks, hooks, web mode, TUI mode, and all supported adapters.
- Native PTY execution, terminal input, Ctrl+C handling, idle timeouts, and
  cleanup of adapter subprocess trees.
- Loop locks, registry and merge-queue file locking, PID liveness checks,
  parallel git worktrees, shared loop state, and merge operations.
- Windows-compatible hook and helper command execution, including documented
  PowerShell/cmd forms where a POSIX shell script cannot run natively.
- Windows CI validation and a Windows release artifact/installer.

Out of scope:

- Changing orchestration semantics or configuration formats.
- Removing or weakening Unix locking, signal, PTY, or process-group behavior.
- Replacing current Linux/macOS release targets or installers.
- Requiring WSL, MSYS2, Cygwin, tmux, or a Unix shell for the native target.

## Compatibility Design

1. Keep platform-specific behavior behind a small number of private platform
   modules. Callers must not contain ad-hoc `cfg` branches for locking,
   liveness, process trees, or shell resolution. Existing Unix behavior must
   remain behaviorally unchanged.
2. Use one cross-platform file-locking abstraction for the shared lock,
   registry, task/memory stores, and merge queue. Unix must continue to use
   advisory locks; Windows must use an equivalent OS-backed lock with the same
   blocking/non-blocking semantics.
3. Provide one process-lifecycle abstraction for Windows PID liveness and
   descendant cleanup without relying on
   Unix signals or negative process-group IDs. Cleanup must terminate direct
   children and descendants and must not leave adapter or MCP processes behind.
4. Preserve `portable-pty` as the PTY abstraction. Add Windows-specific
   behavior only where terminal control, process termination, or shell
   resolution differs.
5. Avoid unsafe code in the workspace. Any Windows API behavior must use a
   safe, maintained abstraction or an existing safe OS command boundary.
6. Make worktree shared-state setup work without requiring Windows Developer
   Mode or administrator privileges. If native symlinks are unavailable, use a
   safe equivalent or an explicit Windows fallback that preserves the sharing
   and isolation contract.
7. Treat configured hook commands as argv, not shell fragments. Support
   executable `.cmd`, `.bat`, and PowerShell forms on Windows while retaining
   existing Unix executable/script behavior.

## Architecture and Invariants

### Platform boundary

The implementation must introduce explicit boundaries rather than spreading
Windows conditionals through orchestration code:

| Boundary | Owner | Consumers | Contract |
|---|---|---|---|
| File locking | `ralph-core` platform module | loop lock, registry, stores, merge queue | A held guard owns the OS lock; try/blocking modes and drop-release semantics are stable. |
| Process liveness | `ralph-core` platform module | registry, merge decisions, loop commands | A PID check is best-effort and never claims an unrelated process is Ralph. |
| Descendant cleanup | `ralph-adapters` process module | PTY/CLI/ACP executors | Cleanup is scoped to the spawned process tree and is idempotent. |
| Command resolution | shared adapter/helper module | hooks and backend launchers | Commands are argv; shell selection is explicit and never inferred from user text. |
| Worktree shared state | `ralph-core` worktree/context modules | parallel loops | Isolation and shared-memory semantics are preserved without requiring symlink privilege. |

The public orchestration APIs and persisted formats remain unchanged. Platform
modules should expose behavior-oriented methods, not OS handles or Windows
types, so the rest of the workspace remains target-neutral.

### Locking invariants

- The lock handle remains alive for the entire guard lifetime.
- Metadata is written only after the lock is acquired and while it is held.
- A failed non-blocking acquisition never truncates or modifies another
  process's metadata.
- Registry, task/memory, and merge-queue read-modify-write operations retain
  their current serialization guarantees.
- Lock release is automatic on normal return, panic, cancellation, and
  process exit as provided by the selected OS-backed abstraction.
- No implementation may silently fall back to unlocked writes.

The preferred implementation is a maintained cross-platform file-locking
dependency behind the existing internal APIs. Reimplementing `LockFileEx`,
`flock`, or equivalent OS calls in application code is not acceptable unless a
dependency audit proves it necessary.

### Process-lifecycle invariants

- Cleanup identifies the owned process tree at spawn time; it must not kill by
  executable name or broad PID guesses.
- Normal completion, timeout, cancellation, and Drop cleanup all converge on
  the same idempotent termination path.
- Graceful termination has a bounded wait, followed by forced termination.
- A descendant that exits before cleanup is treated as success, not as an
  error.
- Process cleanup must not use shell interpolation with user-controlled
  command or path data.

On Windows, the implementation should prefer an OS process-tree primitive
(such as a Job Object) exposed through a safe maintained dependency. A
`taskkill` fallback is acceptable only if it is scoped to the owned process
tree, has tests proving descendant cleanup, and is isolated behind the same
process module.

### Worktree and shared-state invariants

- Git worktree creation/removal continues to use Git's argv interface and
  remains independent of the host shell.
- Relative paths are resolved with `Path`/`PathBuf`; no manual slash or
  drive-letter parsing is introduced.
- Shared memories/specs/tasks remain shared, while events and diagnostics
  remain isolated per worktree.
- The Windows fallback must not silently turn shared state into stale copies.
- Copy/link operations must not follow an untrusted symlink outside the
  intended workspace.

### Compatibility and dependency policy

- New dependencies must be target-capable, actively maintained, compatible
  with the workspace license, and justified in the PR description.
- Unix dependencies remain target-gated where possible; Windows dependencies
  must not be added to Unix builds unnecessarily.
- No changes to event schemas, YAML/TOML configuration, JSONL formats, lock
  metadata, or CLI output are permitted solely for Windows support.
- Existing Unix code paths should be preserved unless a shared abstraction
  demonstrably removes duplicated behavior and has equivalent Unix tests.

## Required Changes

### Core portability

- Introduce the shared platform locking/liveness boundary first.
- Replace non-Unix `UnsupportedPlatform` paths in loop locking, loop registry,
  and merge queue with implementations through that boundary.
- Port all shared file-locking users, not only `.ralph/loop.lock`, without
  weakening their existing atomicity.
- Port PID liveness and stale-entry cleanup through the boundary.
- Port worktree shared-state links/fallbacks and path handling.

### Adapters and process lifecycle

- Gate every Unix-only `nix`, `ExitStatusExt`, signal, process-group, and raw
  permission use correctly.
- Implement Windows equivalents for graceful and forced adapter termination
  through one descendant-process module.
- Ensure ACP terminal commands and grandchildren are cleaned up on completion,
  cancellation, timeout, and panic/drop paths.
- Verify command lookup and environment propagation for native Windows CLI
  backends.

### CLI, hooks, TUI, and web

- Gate Unix signal handlers while retaining Ctrl+C and normal shutdown on
  Windows.
- Keep TUI raw-mode cleanup correct on Windows abort and panic paths.
- Add native Windows hook execution/documentation without changing Unix hook
  behavior.
- Verify web server startup/shutdown and child-process cleanup on Windows.

### CI and distribution

- Add a Windows workspace build/test job to CI without replacing existing jobs.
- Add Windows-specific smoke/integration coverage for locks, worktrees,
  subprocess cleanup, TUI startup/shutdown, and the CLI binary.
- Append `x86_64-pc-windows-msvc` to cargo-dist targets only after the Windows
  build and smoke gates pass.
- Add the Windows archive/executable and appropriate installer documentation;
  existing Unix release artifacts must remain unchanged.

## Threat Model and Review Gates

The PR must explicitly address these failure modes:

| Risk | Required defense |
|---|---|
| Concurrent writers corrupt state | OS-backed lock held across the complete read-modify-write operation; contention test with multiple processes. |
| Stale metadata blocks future runs | Handle-based lock ownership plus stale-PID/worktree recovery tests. |
| PID reuse kills an unrelated process | Ownership-scoped process cleanup; no image-name or arbitrary-PID termination. |
| Adapter grandchildren survive | Real descendant process integration test on Windows. |
| User command injection | argv-based execution; no implicit `cmd /c` around arbitrary strings. |
| Symlink/junction escapes workspace | Link/copy boundary tests for in-workspace and out-of-workspace targets. |
| Unix regression hidden by Windows work | Existing Unix CI remains required and unchanged; platform-specific tests run on both relevant targets. |
| Cross-compile-only confidence | Runtime tests execute on `windows-latest`; cross-compilation is supplementary. |

The implementation should be submitted in reviewable commits, preferably:

1. platform abstractions and compile gate;
2. locks/registry/merge queue/worktree state;
3. adapter process lifecycle and PTY behavior;
4. hooks/TUI/web behavior and integration tests;
5. CI/release/docs.

Each commit must compile, and the final PR must include a short compatibility
matrix showing which features were executed natively on Windows.

## Acceptance Criteria

### Build and existing-target safety

- Given the current repository, when the existing Linux/macOS CI gates run,
  then they pass without platform-specific regressions.
- Given the workspace, when `cargo check --workspace --target
  x86_64-pc-windows-msvc` runs, then it succeeds.
- Given the workspace, when the Windows CI test command runs, then all
  non-Unix tests pass and Unix-only tests are explicitly and correctly gated.
- Given a clean Windows checkout without WSL, MSYS2, Cygwin, or tmux, when the
  native smoke suite runs, then the supported Ralph features execute without
  a POSIX shell dependency.

### Single-loop operation

- Given a native Windows checkout with Git installed, when `ralph run` starts,
  then it acquires the primary lock and runs in place.
- Given a second native Windows `ralph run` in the same workspace, when the
  first loop holds the lock, then the second loop is isolated in a git
  worktree rather than corrupting shared state.
- Given a loop process exits unexpectedly, when a later run starts, then stale
  lock and registry state is safely detected and recoverable.

### File coordination and worktrees

- Given concurrent Windows processes, when they update registry, task,
  memory, and merge-queue files, then updates are serialized with no partial
  JSONL/JSON writes.
- Given parallel loops, when a worktree is created and removed, then Windows
  paths, branches, shared state, and cleanup behave like Unix worktrees.
- Given symlink privileges are unavailable, when a worktree is created, then
  Ralph uses the documented fallback and remains functional.

### Adapters and terminals

- Given a supported Windows backend CLI, when Ralph runs a prompt through PTY
  mode, then output, ANSI handling, input, timeout, and completion parsing work.
- Given Ctrl+C, timeout, cancellation, and ACP terminal completion, when the
  operation ends, then all owned child and descendant processes are gone.
- Given a child process that spawns an MCP/helper descendant, when Ralph is
  cancelled, then the descendant is also terminated without affecting unrelated
  processes.

### Hooks and services

- Given a native Windows hook configured as `.cmd`, `.bat`, or PowerShell,
  when its phase runs, then arguments, environment, exit status, and output
  are handled correctly.
- Given web mode or the Telegram service, when Ctrl+C requests shutdown, then
  services stop cleanly without Unix-only signal failures.

### Release

- Given a release tag, when cargo-dist runs, then it produces a Windows
  executable/archive in addition to the existing artifacts.
- Given an existing Linux or macOS release, when the release workflow changes,
  then its target names, installers, and build commands remain unchanged.

### Review quality

- No production `cfg(windows)` branch is duplicated across more than one
  caller when it can live in the platform boundary.
- No `UnsupportedPlatform` path remains for a feature in the parity scope.
- No unsafe code is added to workspace crates.
- Every new Windows behavior has at least one runtime test on
  `windows-latest`; source-pattern tests are not sufficient.
- The PR description documents dependency choices, known Windows behavior
  differences, and evidence that existing targets are unaffected.

## Verification Commands

On Unix:

```bash
cargo fmt --all -- --check
cargo test
cargo test -p ralph-core smoke_runner
cargo run -p ralph-e2e -- --mock
```

On Windows:

```powershell
cargo check --workspace --target x86_64-pc-windows-msvc
cargo test --workspace
cargo run -p ralph-e2e -- --mock
```

The final PR must include evidence from a real `windows-latest` runner for
the Windows commands and the relevant native Windows smoke scenarios.

## Delivery Stages

1. Add platform abstractions and make the workspace compile on Windows.
2. Port locks, registry, merge queue, PID checks, and worktree shared state.
3. Port adapter process lifecycle and PTY behavior.
4. Port hooks, TUI/web shutdown, and Windows integration tests.
5. Add CI and Windows release packaging.
6. Run the full Unix and Windows gates and document the supported feature
   matrix.
