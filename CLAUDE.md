# Rules for agents working on Zoraya

Read `docs/design.md` first. It says what exists and where it lives.

## Decisions

- Accepted ADRs in `docs/adr/` are binding. Before changing an area, read the ADRs that govern it
  (`docs/adr/index.md`).
- Any deviation from an accepted ADR, or any new architectural decision, requires a new ADR with status `proposed`,
  written from `docs/adr/0000-template.md` under the rules of ADR-0001. Never change design silently.
- Never set or change an ADR status, and never edit an ADR's "Architect's decision" section (ADR-0001 rule 2). Only
  the architect does this. If an architect's task explicitly tells you to transcribe a decision the architect already
  recorded (as TASK-04 Step 1 did), that is the architect's instruction for that task, not a standing permission.
- Change an accepted ADR only in the two ways ADR-0001 allows agents: the `### Decision` sub-heading of rule 3, after
  the architect changes a recommendation, and the open-question note of rule 6 below. For anything else, propose a
  superseding ADR.
- Use the ADR number the task reserves for you. If the task reserves none, ask the architect. Number allocation for
  parallel work is an open question of ADR-0001.
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
- A stage's tag goes on `main` only after CI on `main` is green for that commit. A published tag is moved only with
  the architect's permission.

## Environment

- Never create, delete or reconfigure virtual machines, accounts or system-wide settings on the host. If a required
  environment is missing, stop and report.
- Do not modify `docs/research/` (historical record) or a task file in `docs/tasks/`.
- `tools/posix-surface/` is the research pipeline that produced `docs/research/task-02*`. Leave it unchanged unless a
  task is about it.
- `tools/run-qemu.sh` is the only thing that invokes QEMU. To change the QEMU command line, change that script.
- `make test` must not need network access once the toolchain in `rust-toolchain.toml` is installed.

## Memory limits

The host has crashed from memory exhaustion (`docs/backlog.md`, the memory entries of 6a and 6c).

- QEMU and guest programs run only through `tools/test-runner` or `make`, never directly. The runner caps a run's
  output at 64 MiB and keeps the whole of it in `target/test-logs/` (`zoraya-crash-test` keeps none, only its
  bounded buffer).
- Any ad-hoc command whose output can grow goes to a file, and is read with `tail`, `grep` or `head -c`; never stream it
  into the session.
- No background command without a timeout: the runner's `--timeout`, a test's own limit, or (macOS has no `timeout`)
  `perl -e 'alarm N; exec @ARGV' COMMAND...`.
- Every host-tested crate under `crates/` installs `zoraya_test_alloc::cap_memory!()` in its library and in every
  file of its `tests/` (`make check-test-caps`); the `tools/` crates do not yet (`docs/backlog.md`). Mutation runs
  go through `make mutants` or `make mutants-host`, under
  `tools/mutants/watchdog.sh`. `make mutants-host` runs two mutants at once and gives each one's tests 5 times the
  unmutated baseline's test time, at least 20 s (`MUTANTS_TIMEOUT_MULTIPLIER`, `MUTANTS_TIMEOUT_MIN`);
  `MUTANTS_TIMEOUT=N` sets N seconds instead, for a crate whose tests are slow. Do not go back to a fixed long timeout:
  each mutant that loops costs the whole of it.

## Work discipline

- Testing has tiers, fastest first:
  - `make check` (fmt, clippy, adr-check and every `check-*`; seconds): before every commit.
  - `make test-changed`: during work. It runs the host tests of the crates changed since `origin/main` and of the
    crates that depend on them, and the QEMU steps that cover the changed paths (`tools/test-map.txt`; a path not
    in the map runs every step), under HVF, in parallel. Add a line to the map when you add a directory.
  - `make test-par`: every step, in parallel lanes (at most 3 QEMU guests and one host-test job, under the
    watchdog). `make test`: every step, one after another. These are for the architect and for CI's own checks;
    an agent session does not run them, nor a full GUI run (`FULL=1`, every `GUI_RUNS`).
  - Locally the TCG runs, the tour's second run and crash rounds past 2 are skipped; CI runs them all.
- Locally run only `make check` and `make test-changed`; CI does the rest. A merge candidate (the tag
  `candidate/BRANCH` on the branch's head, a pull request, or `gh workflow run test.yml --ref BRANCH`) runs the
  steps its change touches (`tools/ci-plan.sh`, the map of `make test-changed`) and a smoke set (boot, the shell's
  files script, one GUI run, the POSIX hello script). The full set (`tools/ci-tests.txt`, what `make test` runs)
  runs nightly on `main`, on demand (`gh workflow run test.yml --ref BRANCH -f full=true`) and on a tag
  `candidate-full/NAME` that the orchestrator pushes before every stage tag.
- Before a merge, the candidate run on the branch must be green; merge only when it is. A stage is not done until a
  full run of its head (`candidate-full/`), and then `main`'s, pass. Run them; do not infer them. A red nightly run
  is the first item of the next session.
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

## Models and loops

- Code and reviews, including every independent review of a task, are done by the main model. An implementation
  session may run on Sonnet; it then hands every independent review, kernel code, `unsafe` code, security boundaries
  (grants and rights, the trusted UI, driver boundaries) and hard debugging to the `opus-review` subagent
  (`.claude/agents/`).
- Waiting for CI goes to the `ci-watch` subagent (it reports failing jobs and their first lines in at most 30 lines),
  and finding the first real failure in a log to `log-triage`; both are cheap and read only.
- Test runs and the sorting of mutants are not: run tests with `make test` (it prints a summary, with each step's log in
  `build/logs/`) or the `test-runner` subagent, and mutation tests with `make mutants` (survivors in
  `build/mutants/report.md`) and the `mutant-triage` subagent (`.claude/agents/`). Open a log only when its step
  failed.
- A mutation found by hand goes into a list in `tools/mutants/`, so that the next run repeats it.

## Dependencies

- Every external crate used by the kernel or a `no_std` crate is listed in `docs/dependencies.md` with its exact
  version, license, purpose, and the reason it is not written in-house. Adding one is a decision (see above).
- Only permissively licensed (MIT, Apache-2.0, BSD, ISC or Zlib) `no_std` crates. Prefer writing small things yourself.
  A data file that is not code (the TLS root store, ADR-0228) may also be `CDLA-Permissive-2.0`; code never.
- Host-side tools use `std` only unless a dependency is recorded the same way.

## Kernel code

- Target `aarch64-unknown-none`. The kernel runs on QEMU `virt`. Real hardware is out of scope.
- Hardware facts come from the device tree, not from constants. The one exception is
  `console::FALLBACK_UART_BASE`, which is used only to report that the device tree is unusable.
- Rust code only runs with the MMU on, on Normal memory, so atomics including read-modify-write are allowed. The boot
  stub in `boot.rs` runs before that on Device-nGnRnE memory: there, use only aligned accesses and no exclusives. MMIO
  uses aligned volatile accesses through device mappings.
- There is no allocator before `memory::frames::init` runs, so code before it uses fixed-capacity data. After it,
  follow ADR-0022 on where infallible allocation is allowed.
- Tests that need the machine go in `kernel/src/testing.rs`, using the `ZORAYA-TEST` protocol. A test fails by
  panicking. A test that must fail is listed with `--expect-panic` in the `Makefile`.

## Conventions

- Repository language: English (code, comments, docs, commit messages).
- Committed files contain no absolute paths of the host, no user names and no email addresses, apart from the
  configured git identity. Write paths relative to the repository root. `make check-host-paths` (part of `make test`)
  looks for home-directory paths and email addresses; `docs/research/` and `tools/posix-surface/results/` are cleaned
  separately.
- Crate names use the prefix `zoraya-`.
- Task specifications live in `docs/tasks/`. Reviews of a task go next to it (`docs/tasks/TASK-NN-review.md`).
- Keep `docs/design.md` true when what exists changes. Its Vision section and the stage plan belong to the architect.
