# TASK-07: Filesystem core implementation (host-tested)

## Precondition

Start only if the filesystem ADRs from TASK-06 (`docs/adr/0010-*` to `0019-*`) are **accepted** on `main`. If any ADR this task needs is still `proposed` or was rejected, stop and report which ones. Implement the accepted version, including any changes the architect recorded in "Architect's decision"; the accepted text overrides `docs/research/fs/implementation-outline.md` wherever they differ.

## Where this runs

A **cloud session** (Linux container), in parallel with kernel work. Everything in this task runs on the host with `cargo test`; no QEMU, no Mac, no VM.

## Context

Read first: `CLAUDE.md` (if present), `docs/adr/index.md`, the accepted filesystem ADRs in full, ADR-0001 and ADR-0002, `docs/research/fs/requirements.md` and `implementation-outline.md`, `docs/dependencies.md` (if present).

The filesystem will run as a userspace service on Zoraya. This task builds its storage engine as a library that knows nothing about Zoraya's IPC, so it can be tested completely on the host now and wrapped in a service later.

## What to build

1. **`crates/zoraya-fs`** — `no_std` + `alloc`, no `std`:
   - on-disk format types with explicit, versioned layout (no reliance on Rust struct layout for persistence);
   - a `BlockDevice` trait matching the accepted block-device ADR: block reads and writes, flush, and the reordering and error semantics the ADR specifies;
   - the engine: mount, create/open/read/write/truncate, directories, rename, unlink, and every operation the accepted ADRs require, with exactly the atomicity, ordering, durability and error-reporting guarantees they specify;
   - all parsing of on-disk data treats the image as untrusted: corrupt input returns an error, never panics, never reads out of bounds, never loops forever.
2. **`tools/zoraya-mkfs`** and **`tools/zoraya-fsck`** — host binaries (`std` allowed) using the same crate. `mkfs` creates an image from a directory tree on the host (this will later build the system disk). `fsck` checks every invariant the ADRs define and reports violations without modifying the image unless asked to repair.
3. **Test devices** in a test-support crate: an in-memory device; a device that records every write and flush; a device that injects I/O errors on chosen operations.

## Testing (the core of this task)

Implement every test the accepted ADRs name for their invariants, plus:

- **Model-based tests.** A simple in-memory reference model of the guaranteed semantics. Random operation sequences (property testing) run against both the engine and the model; observable results must match.
- **Crash-consistency tests.** Using the recording device: after a sequence of operations, construct every crash state the block-device contract allows (every prefix at flush boundaries, and permitted reorderings of writes between flushes; bound the enumeration and state the bound), mount each state, run `fsck`, and check that what is visible satisfies the guarantees: everything made durable is present; anything atomic is all-or-nothing; no state violates an invariant.
- **Error-propagation tests.** Injected write-back and flush errors must be reported to the caller as the client-guarantees ADR specifies and must never be silently dropped (the fsync failure mode from the research catalog).
- **Image fuzzing.** A fuzz target that mounts and walks arbitrary images. Run it for a stated time and commit any crashing inputs it found, fixed, as regression tests. If the fuzzer needs a nightly toolchain, keep it in a separate directory outside the workspace and document how to run it; the workspace itself stays on the pinned toolchain.
- **Toolchain workload replay (if feasible).** From the Part 1 data in `requirements.md`, write a test that replays the filesystem operation pattern of a build (create temporaries, write, rename over targets, stat timestamps) and checks make-relevant behaviour: modification times are monotonic and have the resolution the ADR promises.

Report test counts, the crash-state enumeration bound, fuzzing time and coverage if available.

## Rules

- Follow the accepted ADRs exactly. If something cannot be implemented as specified, or the ADRs are silent on a point that matters, write a short **proposed** ADR (use a free number in 0010–0019; if none is free, number 0020–0029) and implement the least committing option behind a clearly marked boundary. Never change the design silently.
- Every external crate: permissive license, recorded in `docs/dependencies.md` with version and reason. Prefer dev-dependencies (e.g. property testing) over runtime ones. The `no_std` crate should need few or none.
- Every `unsafe` block has a `// SAFETY:` comment. Prefer none in `zoraya-fs`.
- `cargo fmt --check`, `cargo clippy` (warnings as errors) and `cargo test` must pass for the new crates. If a top-level `make test` exists on `main`, wire the new tests into it without breaking the targets it already runs.

## Independent review

A subagent that did not write the code: re-runs all tests from a clean clone; checks each ADR invariant against the test that claims to check it; reviews on-disk parsing for out-of-bounds reads, unchecked arithmetic and unbounded loops; checks that the crash enumeration matches the block-device contract. Findings and dispositions in `docs/tasks/TASK-07-review.md`.

## Git

- Branch: `fs/impl`, from the latest `main`.
- Use the configured git identity. If none is configured, stop and ask. Never pass `-c user.name` / `-c user.email` or invent an identity.
- Commit after each component. You may **push this branch only** (`fs/impl`). Never merge, never push other branches, never force-push.
- Touch only `crates/zoraya-fs*`, `tools/zoraya-mkfs`, `tools/zoraya-fsck`, the fuzz directory, `docs/dependencies.md`, `docs/tasks/TASK-07-review.md`, and the workspace manifest entries needed to add the new crates. Do not modify kernel code or other crates.

## Definition of done

- `zoraya-fs`, `zoraya-mkfs`, `zoraya-fsck` and test support exist and build on the pinned toolchain.
- Every invariant in the accepted filesystem ADRs has a test that checks it.
- Model-based, crash-consistency, error-propagation and fuzz testing done, with numbers reported.
- `mkfs` can build an image from a host directory and `fsck` accepts it.
- Independent review done and addressed; branch `fs/impl` pushed.

## Out of scope

- The filesystem service process, its IPC protocol, and integration with the Zoraya kernel or virtio-blk driver.
- Performance tuning beyond avoiding obviously quadratic behaviour.
