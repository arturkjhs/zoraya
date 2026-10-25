# Rules for agents working on Zoraya

Read `docs/design.md` first. It says what exists and where it lives.

## Decisions

- Accepted ADRs in `docs/adr/` are binding. Before changing an area, read the ADRs that govern it
  (`docs/adr/index.md`).
- Any deviation from an accepted ADR, or any new architectural decision, requires a new ADR with status `proposed`,
  written from `docs/adr/0000-template.md` under the rules of ADR-0001. Never change design silently.
- Never set or change an ADR status, and never edit an ADR's "Architect's decision" section. Only the architect does
  this. The one exception is transcribing a decision the architect has already recorded elsewhere, when a task tells
  you to.
- Change an accepted ADR only in the two ways ADR-0001 allows agents: the `### Decision` sub-heading of rule 3, after
  the architect changes a recommendation, and the open-question note of rule 6 below. For anything else, propose a
  superseding ADR.
- Use the ADR number the task reserves for you. If the task reserves none, use the next free number and say so in
  your report.
- Architectural choices are decisions. These include language features or toolchain channel, external dependencies in
  the kernel, memory layout conventions and ABI shapes. Small implementation choices are not decisions. When unsure,
  write a short proposed ADR and continue with the least committing option.
- `docs/research/candidate-principles.md` (CP-xx) holds candidates, not decisions. Cite them, but do not treat them
  as binding unless an accepted ADR adopts them.
- The first task that works in the area of an accepted ADR implements the checks for its invariants (ADR-0001 rule
  6). If a check cannot be implemented yet, record why in that ADR's open questions. Keep the Status column of
  `docs/adr/invariants.md` true: `checked: <command>` or `not yet applicable (stage N)`. `make adr-check` verifies
  the table against the ADRs.

## Git

- Use the repository's configured git identity. If none is configured, stop and ask. Never pass `-c user.name` or
  `-c user.email`, and never invent an identity.
- Work on the branch named in the task. Never merge, never push and never rewrite published history, unless the task
  explicitly says so.
- Commit after each step the task lists, with a message that says what changed and why.

## Environment

- Never create, delete or reconfigure virtual machines, accounts or system-wide settings on the host. If a required
  environment is missing, stop and report.
- Do not modify `docs/research/` (historical record) or a task file in `docs/tasks/`.
- `tools/posix-surface/` is the research pipeline that produced `docs/research/task-02*`. Leave it unchanged unless a
  task is about it.
- `tools/run-qemu.sh` is the only thing that invokes QEMU. To change the QEMU command line, change that script.
- `make test` must not need network access once the toolchain in `rust-toolchain.toml` is installed.

## Work discipline

- A stage is not done until `make test` passes from a clean checkout. Run it; do not infer it.
- Never claim that a test passes, or report a number or a behaviour, unless you ran the command that shows it in this
  session.
- Logic that does not need hardware lives in a separate `no_std` crate under `crates/`, with host tests
  (`cargo test`). The kernel crate contains only what needs the machine.
- Every `unsafe` block has a `// SAFETY:` comment stating the invariant it relies on. Every `unsafe fn` has a
  `# Safety` section. Clippy enforces the comment (`undocumented_unsafe_blocks` is denied workspace-wide).
- `make test` runs `cargo fmt --check` and `cargo clippy -D warnings`. Do not silence a lint with `allow` without a
  comment saying why.
- Do not start the next stage without the architect's go-ahead.
- Record known limitations where they apply (`docs/design.md` or the task report). Do not leave them implicit.

## Dependencies

- Every external crate used by the kernel or a `no_std` crate is listed in `docs/dependencies.md` with its exact
  version, license, purpose, and the reason it is not written in-house. Adding one is a decision (see above).
- Only permissively licensed (MIT, Apache-2.0, BSD, ISC or Zlib) `no_std` crates. Prefer writing small things yourself.
- Host-side tools use `std` only unless a dependency is recorded the same way.

## Kernel code

- Target `aarch64-unknown-none`. The kernel runs on QEMU `virt`. Real hardware is out of scope.
- Hardware facts come from the device tree, not from constants. The one exception is
  `console::FALLBACK_UART_BASE`, which is used only to report that the device tree is unusable.
- Until the MMU is on (stage 1), memory is Device-nGnRnE. Use only aligned accesses (the target enforces
  `strict-align`), and use no atomic read-modify-write (exclusives need not work on Device memory). Plain atomic loads
  and stores are fine.
- There is no allocator before stage 1. Use fixed-capacity data.
- Tests that need the machine go in `kernel/src/testing.rs`, using the `ZORAYA-TEST` protocol. A test fails by
  panicking. A test that must fail is listed with `--expect-panic` in the `Makefile`.

## Conventions

- Repository language: English (code, comments, docs, commit messages).
- Crate names use the prefix `zoraya-`.
- Task specifications live in `docs/tasks/`. Reviews of a task go next to it (`docs/tasks/TASK-NN-review.md`).
- Keep `docs/design.md` true when what exists changes. Its Vision section and the stage plan belong to the architect.
