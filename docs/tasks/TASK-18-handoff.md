# TASK-18 Part 2: handoff (2026-11-04)

The work moves to another host. Branch `shell/impl`, pushed to `origin`. Never merge into `main`; push only this
branch, never tags, never force.

## Finished last: step 8, byte streams and pipelines (ADR-0152)

- `zoraya_abi::native::slot::{IN, OUT, ERR}` (1001 to 1003). `zoraya-rt`: `print!`/`println!` write to `OUT` and
  `eprint!`/`eprintln!` to `ERR` when the process holds a stream there, else to the console; a closed reader ends
  the program with 0 (INV-0155-4); `input_read` reads `IN`.
- `zoraya-shell`: the host gets a whole pipeline (`Pipe` of `Exec`s, an optional input text from a builtin or
  function that starts it, out and err sinks) and gives every element's record and the captured text;
  `pipeline_status` is ADR-0152 rule 6 (INV-0152-3, tested over every combination of three elements).
- `zoraya-commands`: `Files::read_input` and `Files::print`; `cat`, `head` and `search` read their input when
  given no file; `head` stops reading once it has its lines.
- `zosh`: one job per pipeline, one stream per `|`, relays (terminal, file, capture) and feeds (keys in canonical
  mode, `< FILE`, builtin output), all pumped from the port loop without waiting on a stream; background jobs get
  an `in` at its end and their output above the edited line, control characters as `?`.
- Tests: `tests/shell/files-write.script` (pipelines, `$(...)`, redirection, `echo-args --streams` for
  INV-0152-1, 256 KiB into `head --lines 1` for INV-0155-4, Ctrl-C through a stalled 256 KiB feed for
  INV-0152-2); `tests/gui/terminal.script` reads program output in the window. `docs/design.md`,
  `docs/adr/invariants.md`, ADR-0155's open questions and `docs/backlog.md` updated.

Time for step 8: writing about 60 min (15:41 to 16:40), builds and host tests a few minutes within that, QEMU runs
about 20 min (`make test-files` 4 times, `make test-gui` once; all passed in their last run). **`make test` was not
run to completion** for commit `01af1c2`: the architect asked to go on without it. Run it first on the new host.
No independent review and no mutation run were done for step 8 yet: do them next (`make mutants-host` for
`zoraya-shell` and `zoraya-commands`, then the `mutant-triage` agent).

## Next, in the architect's order

1. `make test` from a clean checkout; then the review of step 8 (independent, by the main model) and its mutants.
2. Step 9, job control (ADR-0158): Ctrl-Z via `job_suspend`, `fg`/`bg`, the stopped state in `jobs`, notices,
   the second Ctrl-C's kill message, `exit` with live jobs; INV-0158-1 to -4. A job moved to the background must
   switch its terminal relays to tamed output (`Z::foreground` decides it per chunk already) and get its keys feed
   closed.
3. The rest of step 7: link objects (ADR-0153 rule 5), the in-QEMU halves of INV-0153-1, -2 and -7, and cursor-up's
   in-place redraw: apply `docs/tasks/TASK-18-drafts/editor-cursor-up.patch` (`git apply`), add `ESC [ n A` to
   ADR-0154's subset checker and the editor's host screen simulator, a GUI screendump test of a long line, and the
   INV-0154 notes. Then delete the draft directory.
4. Step 10: POSIX programs from `zosh` through the broker (`--no-shell`, handoff slot 9) and `ttyd`; `init` starts
   them beside `zosh`. Read ADR-0190, -0191 and -0192 (the changed `snapshot_process` rule) first. POSIX programs
   should see the same three streams as fds 0, 1 and 2 (ADR-0152 rule 7).
5. Step 12: `reboot` (ADR-0159): `power_reset`, `zoraya-dtb`'s `has_system_reset`, the `run-qemu.sh` option for
   `-no-reboot`, the builtin; INV-0155-7, INV-0159-1 and -2.

ADR numbers 0200 to 0209 are reserved for TASK-18; none is used yet.

## Open problems

- **W9a under TCG** (`docs/backlog.md`): `make test-posix`'s W9a scenario failed 4 of 7 runs on this branch against
  0 of 3 on `main`. The architect's decision: do not change 6d's test. When `stage-6e` (ADR-0213, an interrupt on
  progress instead of the fixed 2 s) is tagged on `main`, merge `origin/main` into `shell/impl` and run W9 under TCG
  7 times; only if it still fails more often than on `main`, look at the memory difference.
- INV-0155-4 is run for `cat` and `search` only; `ls` and `find` would need a listing over 64 KiB (ADR-0155's note).
- A builtin that starts a pipeline runs to its end before the programs start (its output is held, up to 1 MiB):
  recorded in `docs/design.md`'s limitations.

## How to test

```
export CARGO_BUILD_JOBS=4                      # as CLAUDE.md asks on a small host
make test                                      # the whole gate; summary, logs in build/logs/
cargo test -p zoraya-shell -p zoraya-commands -p zoraya-describe -p zoraya-resolve
make test-files                                # the shell's file and pipeline scripts (tests/shell/)
make test-gui                                  # the terminal window (tests/gui/terminal.script)
make test-boot                                 # console.script, leak and exit on every configuration
make test-posix                                # 6d's POSIX session, W9 among it
make mutants-host                              # then the mutant-triage agent on its survivors
```

QEMU runs only through `make` or `tools/test-runner`; each run's whole output is in `target/test-logs/`. Long
commands go to a file and are read with `tail` or `grep`; on macOS a time limit is
`perl -e 'alarm N; exec @ARGV' COMMAND`. HVF skips the EL2 configurations; that is expected.
