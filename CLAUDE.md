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
- A command chain that commits must stop at the first failure: join its commands with `&&` or run it under `set -e`,
  never with `;`. A push is a command of its own, never part of a chain: run `make check` (and the part's tests) first;
  if it exits 0, check that `git rev-parse HEAD` is the commit you tested, then run `git push ...` in the next call, in
  the worktree's directory. This holds for `main`, for branches and for tags (`refs/tags/...`). Never push `main`
  unless `make check` exited 0 on the commit being pushed (two pushes went out on 2026-11-14 while `make check`
  failed, because a chain printed the failure and went on).
- Merge form, each command a call of its own, the `cd` first: `git fetch origin`; `git checkout --detach origin/main`;
  `git merge --no-ff origin/BRANCH`; `make check`; `git rev-parse HEAD` (the commit checked); `git push origin
  HEAD:main`. If the push is refused because `main` moved, start again from `git fetch`. Then `git tag NAME`,
  `git push origin refs/tags/NAME`, remove the part's worktree. Do not wait for `main`'s CI run after the merge.
- A stage's tag goes on `main` only after a scheduled, nightly or `candidate-full/` full run that includes the tagged commit
  (its head is that commit or a descendant of it) is green (ADR-0348); `tools/ci-last-full.rb --green-for SHA` (with
  `GITHUB_REPOSITORY` set and `gh` logged in) names that run, or exits 1: a run that skipped or a red one never counts. A published tag is moved only with the
  architect's permission.

## Environment

- Never create, delete or reconfigure virtual machines, accounts or system-wide settings on the host. If a required
  environment is missing, stop and report.
- Do not modify `docs/research/` (historical record) or a task file in `docs/tasks/`.
- `tools/posix-surface/` is the research pipeline that produced `docs/research/task-02*`. Leave it unchanged unless a
  task is about it.
- `tools/run-qemu.sh` is the only thing that invokes QEMU. To change the QEMU command line, change that script.
- `make test` must not need network access once the toolchain in `rust-toolchain.toml` is installed.
- Disk: before starting a part, check free disk (`df -h`). Below 25 GB, first delete `target/` and `build/` of the
  worktrees whose parts are merged, and remove those worktrees. After a part is merged, remove its worktree and its
  mutants' copies (`build/mutants/w*`) at once. At most two worktrees with builds at a time.

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
- Locally run only `make check` and `make test-changed`; CI does the rest. CI policy (ADR-0348, amended by ADR-0463).
  Accepted 2026-11-15 and 2026-11-16. Decided by the architect; recorded by an agent at the architect's request. CI is
  the brake on the work, so it runs less.
  - A part merges after `make check`, `make test-changed` under HVF and its independent Opus review pass locally. No
    candidate run in CI, and no waiting for `main`'s run after the merge.
  - A push to a `work/*` branch (or any branch but `main`) runs `make check` only, no QEMU. Do not dispatch a full run
    (`workflow_dispatch` with `full=true`) except to chase a flake.
  - A candidate run (the tag `candidate/BRANCH` on the branch's head, a pull request, or
    `gh workflow run test.yml --ref BRANCH`; it runs the steps `tools/ci-plan.sh` selects and a smoke set) is made only
    when a part changes `.github/workflows`, the ci-plan scripts (`tools/ci-plan.sh`, `tools/plan-lib.sh`,
    `tools/ci-plan-step.sh`, `tools/ci-makefile-steps.rb`, `tools/ci-last-full.rb`) or `tools/test-runner/`; when a
    failure shows only under TCG; and as `candidate-full/NAME` before a stage tag where no green scheduled or nightly
    run already includes the commit. Adding or changing a test's own lines in `tools/ci-tests.txt` or
    `tools/test-map.txt` (a new step, a map entry) needs no candidate run. The agent tags a candidate itself (a
    `git push` of `refs/tags/candidate/...`, a command of its own); the architect does not. Architect's decision of
    2026-11-19.
  - `main`'s full set runs on a schedule: every 2 hours if `main` changed since the last full run, and nightly (a
    2-hourly run that skips fails if the last full run was red). A push to `main` runs the document checks only,
    whatever changed, and does not cancel a scheduled or manual full run in progress. GitHub's cron is not reliable
    (on 2026-11-16 the runs of 10:23 to 14:23 UTC never started): when `main` changed and no full run has started for
    2.5 hours, an agent starts one on `main`'s head (`gh workflow run test.yml --ref main -f full=true`; check
    `gh run list --workflow test.yml` first, a run already queued or running counts). Architect's rule of 2026-11-17.
- A red scheduled or nightly run is fixed first, before any merge; its summary lists the merges since the last green
  full run. If it is not fixed within an hour, revert the merge that broke it and fix it on its branch.
- A stage is not done until a scheduled, nightly or `candidate-full/` full run that includes its last commit is green.
  A stage tag needs that green run: `tools/ci-last-full.rb --green-for SHA` finds it (its head is the tagged commit or
  a descendant of it on `main`).
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
- A dispatcher never waits without a wake-up (architect's rule of 2026-11-19; a dispatcher idled for hours on an agent
  that had stopped). Before it waits on an agent or a run, it starts a background command `sleep 1800` (or a Monitor
  with a 30-minute limit). When that ends, it checks every agent's branch on origin, its log (the `RC=` line, or the
  last lines and their time) and its processes; acts on what ended (push, merge, start the next part); kills only the
  processes of a run that hung; runs the test itself if the agent is gone; and starts the next wake-up.
- A mutation found by hand goes into a list in `tools/mutants/`, so that the next run repeats it.

## Dependencies

- Every external crate used by the kernel or a `no_std` crate is listed in `docs/dependencies.md` with its exact
  version, license, purpose, and the reason it is not written in-house. Adding one is a decision (see above).
- Only permissively licensed (MIT, Apache-2.0, BSD, ISC or Zlib) `no_std` crates. Prefer writing small things yourself.
  A data file that is not code (the TLS root store, ADR-0228) may also be `CDLA-Permissive-2.0`, and an unmodified font file may be `OFL-1.1` with its licence text (ADR-0327; a subset or a changed font is not allowed); code never. `wget` is GPL-3.0-or-later as a separate program, like GNU make (architect, 2026-11-14); SQLite is public domain, as a test input and inside `python3`.
- An LGPL library (LGPL-2.0, -2.1 or -3.0, any "or later") may be linked statically into a program `ports/Makefile`
  builds, if the library and the program are built from sources pinned in `ports/sources.lock`, the library's patches
  are in `ports/patches/` under its licence, and `docs/dependencies.md` lists it with its licence and the programs that
  link it; never in the kernel, a native program or a `no_std` crate (ADR-0421).
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
