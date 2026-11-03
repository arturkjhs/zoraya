# Plan for TASK-18 Part 2: building the native shell

The order in which Part 2 builds the accepted versions of ADR-0150 to ADR-0158, what each step depends on, and which
steps wait for stage 6. ADR-0150 to ADR-0158 were accepted on 2026-11-03, ADR-0153 and ADR-0155 with changes, and with
them the changes to ADR-0002 (INV-0002-4, -5), ADR-0004, ADR-0053, ADR-0055, ADR-0062, ADR-0096, ADR-0104, ADR-0105
and ADR-0107 they declare (notes in those ADRs); the steps follow the decisions. Each step ends with `make test` passing and a commit. Stage 6's sub-stages are
those of [`../stage-6/plan.md`](../stage-6/plan.md): 6a kernel objects (byte streams, `image_create`, `job_suspend`),
6b toolchain, 6c `libposix` core, 6d broker, `ttyd` and `tmpd`.

## Dependencies at a glance

| Step | Builds | ADRs | Needs from stage 6 |
|---|---|---|---|
| 1 | Description format and parser crate | 0156 | none |
| 2 | `zoraya-command`: argv from descriptions, `--help`, errors | 0155, 0156 | none |
| 3 | Language: tokenizer, parser, expander, evaluator (host only) | 0150, 0151 | none |
| 4 | Line editor, history, completion, suggestions, highlighting (host only) | 0154 | none |
| 5 | Settings crate and `settings` | 0157 | none |
| 6 | The new `zosh` on today's kernel: builtins, functions, scripts, reports, `help` and the viewer | 0150, 0151, 0154, 0155, 0156, 0157 | none |
| 7 | Native start-up record, resolver, grants, file commands as programs | 0153, 0155 | none for files; the resolver is shared with 6c |
| 8 | Streams for commands, relay, pipelines, redirection | 0152, 0153 | 6a streams |
| 9 | Job control: Ctrl-Z, `fg`, `bg`, notices | 0158 | 6a `job_suspend` |
| 10 | Running POSIX programs from `zosh` through the broker | 0153, 0158 | 6a `image_create`, 6b, 6c, 6d, 6e's `make posix-image` |
| 11 | The tour as a test, on serial and GUI; documentation | all | all of the above for the full tour |
| 12 | `reboot` through PSCI `SYSTEM_RESET` | 0159 (proposed), 0062, 0155 | none; kernel work, after the architect decides ADR-0159 |

Steps 1 to 7 need nothing from stage 6 and can start as soon as the ADRs are decided. Step 8 waits for 6a on `main`;
step 10 for 6d and for 6e's image with the POSIX programs installed. Until then `docs/design.md` lists each waiting part as pending (TASK-18, "Part 2").

## Steps

1. **Descriptions** (ADR-0156). A `no_std` crate (`zoraya-describe`) that parses `NAME.cmd` into a typed form:
   header keys, operand kinds, options, exits, settings, body sections. Total and bounded (INV-0156-1), with a
   deterministic random test and a `fuzz/` target. A build step that embeds a description into a program's ELF section
   and into the ramdisk as `NAME.cmd` (ADR-0156 item 2); `tools/mkramdisk` learns the second. Descriptions written for
   every existing native command and builtin.
2. **Command conventions** (ADR-0155). `zoraya-command` over step 1: argv parsing (long and short options, grouping,
   `--`, `./` rule), `--help` and usage rendering, the status-to-words table, the error line and hint, exit codes,
   `PeerClosed` handling. Host tests for INV-0155-3 and the rendering half of INV-0156-3.
3. **Language** (ADR-0150, ADR-0151). In `zoraya-shell`, replacing `split`: a tokenizer, a parser to a tree, an
   expander (lists, quotes, `$(...)`, globs over a directory trait, `~`, the `./-` rule) and an evaluator over a
   `Runner` trait (spawn, wait, builtins), with the stop rule and `$status`. The crate gains `alloc`: a shell with
   functions and lists cannot stay in fixed buffers, and `zosh` already has a heap (`user/src/heap.rs`). Host tests for
   INV-0150-1 to -4 and INV-0151-1 to -3 with a fake runner and a fake directory; the parser fuzz target.
4. **Interactive layer** (ADR-0154). The editor rewritten in `zoraya-shell`: UTF-8 line buffer with a cursor, keys,
   multi-line continuation, history with prefix search and Ctrl-R, completion over a `Completer` trait fed by step 1's
   descriptions and a directory trait, suggestions, highlighting, error rendering with a caret. Host tests for
   INV-0154-1 to -3, with the subset checker of INV-0154-2. The terminal side: `zoraya-term` learns SGR 2 and 22
   (ADR-0154's tension with ADR-0096), with its own tests; the GUI and serial test runners drop SGR sequences before
   matching text.
5. **Settings** (ADR-0157). A `no_std` reader and writer of `/config/settings` that keeps comments, typed checks from
   step 1's `setting` declarations, host tests for INV-0157-1 to -3.
6. **The new `zosh`.** `user/src/bin/zosh.rs` rebuilt on steps 2 to 5: the evaluator's runner over today's `spawn`;
   every builtin of ADR-0155 item 8 with its description; `help`, the topics and the pager; history and settings in
   `/config` and `/home` (created by `zoraya-mkfs` from `rootfs/config/` and `rootfs/home/`); `/config/start.zosh`; the reports of ADR-0151; `count --lines` and the hints for Unix names (ADR-0150 and ADR-0154
   Decisions). `init` starts a new shell when `zosh` exits instead of powering off (ADR-0155 Decision, ADR-0053's
   note); `poweroff` and `reboot` are builtins (`reboot` says it is unavailable until the kernel has a reset call,
   `docs/backlog.md`); `tests/shell/exit.script` checks INV-0155-5, and INV-0063-2's
   check moves to a test that ends `init` itself (ADR-0053's note). Programs
   still come from the ramdisk and still print to the console (no streams yet). The existing serial and GUI scripts
   are updated to the new prompt, reports and commands (`write`, `ls-ramdisk`, `exit-code`, `run`, `exit N` go;
   ADR-0155 item 8), and the demonstration program `exit` is renamed `exit-with`.
7. **Grants and file programs** (ADR-0153, ADR-0155). The native start-up record `zoraya_abi::native::Startup`
   (ADR-0153 rule 8) and the `.zoraya.native` note, its reader and the resolver in `zoraya-rt` (one resolver crate, which 6c's `zoraya-posix` reuses, or
   which reuses 6c's if 6c lands first: the shared crate is the first thing written, whoever needs it first); the
   grant builder in the shell (designated operands, `+r`, `+w`, missing paths) and `grants`; `ls`, `cat`, `cp`, `mv`,
   `rm`, `mkdir`, `stat`, `sync`, `head`, `find` and `search` as ramdisk programs on `zoraya-command`, removed from
   the shell. Paths from values (rule 4), link objects (rule 5), absolute resolution (rule 6). Automatic write designation only for
   base-system commands (ADR-0153 Decision). The `REPORT` channel, its writer in `zoraya-rt` and the shell's hint after
   a job ends (rule 11). INV-0153-1 to -9 (INV-0153-7's `libposix` half in step 10) and INV-0155-1, -2. `narrow` in
   `zoraya-proto-file` and `zoraya-fs-service` (ADR-0055's note), accepted by the architect with this ADR; until it
   lands the shell uses the fallback of rule 2 and shows it. Without streams the programs write to the console, so this step's
   in-QEMU checks read the serial line.
8. **Streams and pipelines** (ADR-0152), after 6a is on `main`. Standard slots `IN`, `OUT`, `ERR` in `zoraya-abi`;
   `zoraya-rt`'s print goes to `OUT`; the shell's relay between the terminal and its jobs (on the console and on the
   GUI channel); pipelines, `&|`, redirection through the shell's own opens; the pipeline status rule. INV-0152-1 to
   -3, INV-0155-4. ADR-0096's limitation "programs started from `zosh` print to the serial console" ends here
   (backlog row closed).
9. **Job control** (ADR-0158), after 6a. Ctrl-Z through `job_suspend`, `fg`, `bg`, the stopped state in `jobs`,
   notices, `exit` with live jobs, background input closed. INV-0158-1 to -3.
10. **POSIX programs** (ADR-0153), after 6d. `init` starts the broker beside `zosh` without a shell (ADR-0153's
    tension with ADR-0104 item 12) and gives `zosh` a native client connection; the broker's `Group(job)` form of
    `spawn_prepare`; `zosh` detects a program with no `.zoraya.native` note, prepares it in the line's job, builds the
    bootstrap record with the toolchain set, a `/tmp` of the line's own and the designated entries, commits, waits on
    the process job and reaps the pid; `libposix` writes refused paths on `REPORT` (ADR-0107's note); `/zoraya/commands/` with descriptions for tcc, make and dash; Ctrl-Z for POSIX jobs through the
    broker. The broker and `init` changes are outside the shell's files; the architect accepted them with ADR-0153. The tour's examples 15 to 17 and 27 then run.
11. **The tour as a test, and documentation.** `tests/shell/tour.script` plays `docs/research/shell/tour.md` on the
    serial console, and `tests/gui/tour.script` in the GUI, checking every line (INV-0154-4); a host check that the
    scripts and the tour's code blocks are the same text, so neither can change alone. Hostile input to the parser
    (fuzz), grants that must be refused, completion and history in the scripted sessions. `docs/design.md` "Using the
    shell" rewritten from the tour and "Conventions for commands" pointing at ADR-0155; `index.md`, `invariants.md`
    (statuses), `backlog.md`.

12. **Reboot** (ADR-0159, added by the architect on 2026-11-03). A short proposed ADR, ADR-0159: a call
    `power_reset(power)` that needs the same `POWER` right as `power_off` and resets through PSCI `SYSTEM_RESET`,
    returning `NotSupported` where the platform has none. After the architect decides it: the call in `zoraya-abi`
    and the kernel, `has_system_reset` in `zoraya-dtb`, a `tools/run-qemu.sh` option that lets a reset restart QEMU
    instead of ending it (`-no-reboot` today), the `reboot` builtin calling it, INV-0159-1 and INV-0155-7 (= INV-0159-2)
    checked. Independent of stage 6; until it lands `reboot` says it is not available (step 6), as the architect
    accepted. This step touches kernel code, which TASK-18 otherwise leaves alone; the architect asked for it.

Review (TASK-18, Part 2): mutation testing of `zoraya-shell`, `zoraya-describe` and `zoraya-command` with
`make mutants-host`, then the review in `docs/tasks/TASK-18-review.md`.

## Risks

- **Size.** Steps 3 and 4 are the largest (an estimate of 5,000 to 7,000 lines with tests); they are host-only and can
  be reviewed before anything boots.
- **Two test runners.** The tour must give the same text on the serial line and in the GUI (INV-0154-4); colour and
  redraw sequences are the likely difference.
- **The resolver shared with 6c.** If 6c and step 7 run in parallel they must agree on one crate first.
- **Descriptions are authority.** A wrong operand kind is a security bug (ADR-0153); review reads each `.cmd` file.
- **POSIX programs without a tty** (ADR-0153): anything interactive in the toolchain set (an interactive dash) is not
  runnable from `zosh` until a later ADR.
