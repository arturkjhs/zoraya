# TASK-17: Stage 6b — cross toolchain, sysroot and ported programs

## Where and how this runs

Locally on the architect's Mac, **in parallel with TASK-16 (6a) in a separate git worktree**. Work only in the worktree this session was started in. Do not touch the other worktree, its branch, or `main`.

## Precondition

`main` is at or after tag `stage-6-design`, with ADR-0100 to ADR-0110 accepted. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/research/stage-6/plan.md` (row 6b), `docs/research/stage-6/inputs.md`, `tools/posix-surface/patches/README.md`.

Read **in full**: ADR-0005, ADR-0033, ADR-0042, ADR-0100, ADR-0108, ADR-0109, ADR-0110. Read the measured data the tier list comes from: `docs/research/task-02/libc-surface.csv`, `posix-surface.md`, `method.md` (pinned versions).

This task builds row 6b of `plan.md`: everything on the host that turns C sources into Zoraya executables, and the ported programs themselves. **No guest code and no QEMU run of ported programs** — `libposix`'s machine half is 6c.

ADR numbers **0140 to 0149** are reserved for this task. Where an accepted ADR is silent, write a proposed ADR; never fill a gap silently.

## Host tools and network

- `make fetch-ports` is the only step that uses the network (ADR-0109). It downloads every pinned archive, checks its SHA-256 against `ports/sources.lock`, and fills a local cache outside the repository's tracked files. Nothing GPL or LGPL is committed. `make test` and `make ports` work offline once the cache is filled, and fail with a clear message naming `make fetch-ports` when it is not.
- If a required host tool is missing (for example a clang that can emit AArch64 ELF objects, or LLVM's `ar`), **do not install it yourself**. Stop and give the architect the exact command (for example a `brew install …` line) and why it is needed, as `CLAUDE.md` requires for host changes. Then continue with what does not need it.
- Record every host tool and its version in `docs/dependencies.md`.

## What to build

As ADR-0100 and ADR-0109 specify:

- **Driver scripts:** `tools/zcc` (clang plus `rust-lld` for `aarch64-unknown-zoraya`, the sysroot, static ELF per ADR-0033) and `tools/zoraya-ar`.
- **`ports/`:** `sources.lock` with pinned versions and hashes (reuse TASK-02's pins where ADR-0109 says so), per-port patch directories with a README naming each original's license, and the `fetch-ports` target.
- **musl:** the retained machine-independent files and headers, the shim that routes their few internal system calls to the Rust crate (ADR-0100), `crt1.S`, `crti`, `crtn`, empty `libm.a` and the other empty archives the toolchain expects.
- **The link before 6c exists:** if the accepted ADRs do not already say how programs link before `zoraya-posix`'s machine half is written, generate a stub `zoraya-posix` whose every OS-facing export from the tier list returns failure with `ENOSYS`, so that every port links now; 6c replaces the stubs. Record this in a proposed ADR if it is your decision.
- **Cross builds** of dash, GNU make, toybox (the T1 applets plus `sort`, `wc`, `yes`), busybox, tcc and `arm64-libtcc1.a`, with their patches, through `make ports`, producing `build/sysroot` and `build/rootfs-posix`.
- **Host checks:** `zoraya-elf` validation of every produced executable (static, W^X, entry point, program headers as ADR-0033 and ADR-0109 require); `tools/posix-surface-check` with its `ar` and `ET_REL` symbol-table reader, checking `libc.a`'s exports against the tier list (INV-0005-1, INV-0005-2) and scanning for stray `svc` outside the permitted code.
- **CI:** the Linux workflow runs `make fetch-ports` and `make ports` and the host checks. Keep the total CI time within its limit; cache the downloaded archives.

## Tests

- The `plan.md` criterion: `make ports` produces `build/sysroot` and `build/rootfs-posix` on macOS and on Linux CI; every file passes the host `zoraya-elf` check; and **the cross-built `tcc`, run on the host only as far as its linker step can be checked there, links a hello world against the sysroot**, with the result checked by `zoraya-elf` (say exactly how this is done, since the tcc binary itself targets Zoraya).
- `posix-surface-check` passes on the real archive, and fails on deliberately broken ones (an extra export, a missing tier symbol, a stray `svc`): each failure case is a test.
- The three risks `plan.md` names, each with a recorded outcome: clang targeting bare AArch64 ELF with musl headers on Apple's clang; toybox and make building without `__linux__`; autoconf cross checks where a port uses them (and the `config.sub` cost ADR-0109 records for T2).
- Reproducibility: two clean `make ports` runs give the same archive symbol tables and the same executables, or the difference is explained.

`make test` passes from a clean clone (with the port cache filled) in every configuration. It gains only fast host checks; long builds stay behind `make ports`.

## Process

Follow `CLAUDE.md`: scripts and code on the main model, test runs through the scripts, mutant triage through the subagent for `posix-surface-check` and other new Rust code. Independent review by a subagent that did not write the code: hashes and pins, license handling of every port and patch, the shim's routing against ADR-0100, the ELF checks against ADR-0033/0109, `posix-surface-check` with mutations. Findings and dispositions in `docs/tasks/TASK-17-review.md`; then one check of the changed lines; if it finds errors, fix and repeat that check at most once more, then report.

## Shared files

`TASK-16` runs at the same time. Keep edits to shared files small and local: `Cargo.toml` workspace members (add lines, do not reorder), `Makefile` (add targets, do not restructure), `docs/design.md` (your own sections only), `docs/adr/index.md` and `invariants.md` (your rows only), `docs/dependencies.md`, `docs/backlog.md`, the CI workflow. Do not edit `kernel/`, `crates/zoraya-abi`, `crates/zoraya-rt` or anything TASK-16 builds.

## Git

Branch `stage-6b` from the latest `main`, in its own worktree. Configured git identity only. Commit after each step. **Never merge, never push.**

## Definition of done

- `make fetch-ports` and `make ports` work as specified; every ported program and the sysroot are built and pass the host checks; nothing GPL/LGPL is committed.
- `posix-surface-check` enforces INV-0005-1 and INV-0005-2 on the real archive.
- The three named risks have recorded outcomes; missing host tools, if any, are listed with install commands.
- `make test` passes from a clean clone; CI runs the new steps.
- Review and the check of its fixes done and addressed. Final report with build times, sizes, invariant delta, proposed ADRs, open questions.

## Out of scope

Running anything in QEMU beyond what `make test` already runs; `zoraya-posix`'s machine half (6c); the broker, terminal, `/tmp` (6d); acceptance workloads (6e); T2.
