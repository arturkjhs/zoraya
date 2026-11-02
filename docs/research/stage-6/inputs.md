# Stage 6 design inputs: everything the accepted ADRs defer to stage 6

Part of [TASK-15](../../tasks/TASK-15-stage-6-design.md). This lists every "Deferred to stage 6" section, every open
question, follow-up and revisit trigger that names stage 6, `libposix`, the broker, `fork`, `snapshot_process` or the
temporary directory service, in the ADRs the task names, plus the items `docs/design.md` and `docs/backlog.md` leave
to this stage. Each row says which proposed ADR answers it, or why it stays open. Numbers refer to proposed ADRs
0100 to 0110 (`docs/adr/`). The list was made by reading each ADR in full and by `grep` over `docs/adr/` for
`stage 6`, `libposix`, `personality`, `Deferred`, `snapshot`, `fork` and `broker`; it is complete to that search.

**Status words.** *answered*: an ADR below recommends a design for it. *measured later*: an ADR defines how the
measurement is taken, and the number itself waits for the prototype. *open*: no ADR decides it, with the reason.

## ADR-0002: boundary

| # | Item | Source | Disposition |
|---|---|---|---|
| I-1 | INV-0002-2 (`#[personality_only]` set, kernel refusal without the `posix` bit) and INV-0002-6 (`WIFSIGNALED` from `reason`) cannot be checked before `snapshot_process` and `libposix` exist | open questions | answered: ADR-0102 (the one gated facility stays `snapshot_process`, now with a move list), ADR-0106 (`WIFSIGNALED`); both checks are stage 6 work in `plan.md` |
| I-2 | INV-0002-3, -4, -5 are `not yet applicable (stage 6)` | `invariants.md` | answered: ADR-0103 (no broker IPC on the data path), ADR-0107 (resolver, table), ADR-0104 and ADR-0107 (what a POSIX process starts with). ADR-0107 adds one transient handle, the bootstrap channel, and says how INV-0002-5's test treats it |
| I-3 | Path-resolution cost of a prefix table on W1's 5,294 header opens | open question, follow-up | measured later: ADR-0110 names the measurement; ADR-0107 bounds the table to a handful of entries so the prefix match is cheap |
| I-4 | Memory cost of fork emulation under strict commit (X-6): dash RSS at fork points in W4 | open question | measured later: ADR-0102 gives the mechanism and the estimate, ADR-0110 the measurement for the T2 decision |
| I-5 | Byte names that are not UTF-8 (X-8) | open question | open: no workload exercised them (ADR-0013 already resolves X-8 toward rejection with `EILSEQ`); ADR-0107 keeps that mapping and nothing here changes it |
| I-6 | Whether third-party jobserver clients accept both jobserver styles | open question | open: still unmeasured; ADR-0101 and ADR-0107 support both styles, so the question does not block stage 6 |
| I-7 | Broker per session, per user or per machine | open question | answered: ADR-0104, per session, one broker instance per terminal session, started by `init`'s launcher path |
| I-8 | A broker protocol specification | follow-up | answered: ADR-0104 lists the methods; the `protocol!` declaration is stage 6 code |
| I-9 | A namespace-table format in the `abi` crate | follow-up (moved to stage 6 by ADR-0053) | answered: ADR-0107 |
| I-10 | `/proc/self/exe` from the process's own image handle; cwd as a directory handle plus a canonical path | rule 5 | answered: ADR-0107 (cwd), ADR-0108 (`/proc/self/exe` and image handles) |
| I-11 | `environ` from the spawn vector; `MAKEFLAGS` fd numbers at requested slots; FIFO paths shared through `/tmp` | rule 7 | answered: ADR-0103 (fd slots at spawn), ADR-0107 (`/tmp`) |
| I-12 | Identity: fixed non-zero uid/gid, `/etc/passwd` and `/etc/group` as files | rule 6 | answered: ADR-0100 keeps musl's `passwd` parsers over files from the namespace; ADR-0107 lists the two files |

## ADR-0003: process creation

| # | Item | Source | Disposition |
|---|---|---|---|
| I-13 | Memory and time cost of `snapshot_process` for dash under strict commit on W4's 764 configure forks | open question, follow-up, revisit trigger | measured later: ADR-0102 (mechanism, estimate), ADR-0110 (measurement) |
| I-14 | Semantics of a duplicated channel endpoint in the child | open question | answered: ADR-0102: none is duplicated (ADR-0050 has no `DUPLICATE` on an endpoint); the move list carries fresh endpoints, ADR-0103 says which |
| I-15 | Whether toybox's `xfork` applets (`cpio -p`, `tar`) are in scope | open question | open: they are T2 applets (`applets-by-tier.tsv`), outside T1; ADR-0102's `fork` serves them if T2 comes |
| I-16 | Does any program depend on `exec` keeping the pid, apart from the shell's `exec` builtin | open question | answered in part: ADR-0104 keeps the pid for every `exec` (make's `$(shell)` and `sh -c` children are waited on by pid), so the question need not be settled |
| I-17 | Per-type duplicability for `snapshot_process` in the `abi` crate (also ADR-0040 follow-up) | follow-up | answered: ADR-0102 lists the types |
| I-18 | Patch dash's interactive `vforkexec` to `spawn` plus `start_suspended` | follow-up | answered: ADR-0109 lists the dash patches; ADR-0104 gives the group handle `spawn` needs |
| I-19 | INV-0003-2 and INV-0003-7 are stage 6 | `invariants.md` | answered: ADR-0102 keeps both and adds the move-list invariants |
| I-20 | Rule 3's "new process in the same job" | rule 3 | answered with a change: ADR-0104 makes every POSIX process a job of its own inside its group's job, so the child goes into a fresh sibling job; ADR-0102 lists this as a tension |

## ADR-0004: asynchronous notification

| # | Item | Source | Disposition |
|---|---|---|---|
| I-21 | INV-0004-6 (faulting thread suspended until an exception handler resumes it) waits for exception channels, "marked stage 6" | open question | open: ADR-0106 recommends no exception channels at stage 6 (ADR-0005 excludes fault capture); a faulting POSIX process ends with reason `Fault` and the invariant stays `not yet applicable`, with the stage number to be revised by the architect |
| I-22 | INV-0004-7 (`PeerClosed` on a byte stream) waits for byte streams | open question | answered: ADR-0101 |
| I-23 | INV-0004-2's `waitpid` half, INV-0004-8 (`SIGCHLD` subscription only with a handler) | open question | answered: ADR-0106 |
| I-24 | Reaction latency of a compute-bound POSIX program with a handler | open question | answered in part: ADR-0106 adds a bounded poll at library entry for processes with a handler installed, and names the measurement; the compute-loop case stays a documented limitation |
| I-25 | Whether make's cleanup runs correctly from the main wait (M7 estimate, about 30 lines) | open question, follow-up | answered: ADR-0106 runs the handler on the calling thread at the wait point, so make's `fatal_error_signal` runs unpatched; ADR-0110's W9 test is the check |
| I-26 | Interrupts arriving during `spawn` or `snapshot_process` | open question | answered: ADR-0102 and ADR-0104: a request is queued on a new process only if its disposition is `Deliver` at creation, and `snapshot_process` copies dispositions but not the queue |
| I-27 | Ctrl-Z, `SIGTTIN`/`SIGTTOU`, `SIGWINCH` not exercised; a W5 extension needed before the job-control mapping is trusted | open question | open in part: ADR-0105 specifies Ctrl-Z through `job_suspend`/`job_resume` and `SIGWINCH` as a `Custom` request from the terminal, and leaves `SIGTTIN`/`SIGTTOU` unemulated at stage 6 with the reason; the W5 extension is listed in ADR-0110 as a measurement, not an acceptance test |
| I-28 | Who may bind an exception channel to a `posix` job; whether a crash reporter is wanted | open question | open: ADR-0106 says no exception channel is bound at stage 6; a crash reporter is a later ADR |
| I-29 | The terminal service's foreground-job protocol (CP-25) | follow-up | answered: ADR-0105 |
| I-30 | A patch of GNU make's `fatal_error_signal` as the M7 calibration | follow-up | answered: not needed under ADR-0106's delivery rule; ADR-0110 runs W9 unpatched |
| I-31 | A test reproducing W9a and W9b with exit statuses 130/143 and zero partial targets | follow-up | answered: ADR-0110 |
| I-32 | `SIGTTIN`: "a background job's read on the terminal returns `NotForeground`" | personality mapping | answered with a deviation: ADR-0105 makes the terminal a byte stream, which cannot refuse a read by caller, so this sentence is deferred; the tension is listed there |
| I-33 | Process groups are sub-jobs; `setpgid` at spawn through `start_suspended`; `setpgid(0, 0)` by a lone process declares its job a group | personality mapping | answered: ADR-0104, with the group handle passed at `spawn` and `snapshot_process` |

## ADR-0005: scope

| # | Item | Source | Disposition |
|---|---|---|---|
| I-34 | INV-0005-1 to -4 are stage 6 | `invariants.md` | answered: ADR-0100 (export check), ADR-0110 (call log, `W*-check`) |
| I-35 | A generated `docs/adr/posix-surface-T1.txt` and `-T2.txt` from the CSVs | follow-up | answered: ADR-0100 and ADR-0110 |
| I-36 | A larger autoconf project before T2 is accepted | open question | open: T2 beyond the measurement plan is out of this task's scope |
| I-37 | Whether `tcc -ar` and the `ld` wrapper suffice for autoconf projects | open question | open: T2 |
| I-38 | toybox `mktemp` prints a `/tmp` path: confirm the per-session `/tmp` satisfies configure | open question | answered in part: ADR-0107 gives `/tmp` the semantics `mktemp` needs (a path that other processes of the session can open); confirmation on W4 is T2 |
| I-39 | Which toybox applets are the accepted set | open question | open: `applets-by-tier.tsv` stays normative; ADR-0109 builds the T1 row and the architect may add applets |
| I-40 | `snapshot_process` measured on W4 before T2 is accepted | in-scope text, revisit trigger | measured later: ADR-0110 |
| I-41 | Job control beyond Ctrl-C and background jobs is specified to POSIX but not an acceptance test | exclusion table | answered: ADR-0105 keeps that split |

## ADR-0006: events

| # | Item | Source | Disposition |
|---|---|---|---|
| I-42 | INV-0006-7 (`pselect` returns `EINTR` only if a handler ran) and INV-0006-9 (three FIFO open behaviours) are stage 6 | `invariants.md`, open question | answered: ADR-0106, ADR-0107 |
| I-43 | FIFO nodes in the temp-directory service with POSIX open semantics | follow-up, personality mapping | answered: ADR-0107, on ADR-0101's stream object |
| I-44 | `libposix` `pselect`/`sigsuspend`/`wait4`/`nanosleep` over the port; `O_NONBLOCK` as a library flag | personality implementation | answered: ADR-0106 (waits), ADR-0103 (flags) |
| I-45 | Jobserver pipe style: fds 3 and 4 at slots 3 and 4 for recursive makes only; `-2,-2` | personality implementation | answered: ADR-0103 |
| I-46 | Bad stdin: a stream with `PEER_CLOSED` at slot 0 | personality implementation | answered: ADR-0101, ADR-0103 |
| I-47 | A prototype rerun of W2-j, W8, W9 and W10 as the acceptance test | follow-up | answered: ADR-0110 |
| I-48 | Wake-up rate at high `-j` (W8 at `-j16`) | open question, follow-up | measured later: ADR-0110 lists it |
| I-49 | Other jobserver clients (ninja, cargo) and `poll` | open question | open: unmeasured, not in scope |
| I-50 | Interactive line editors (`ppoll`) | open question | open: line editing is excluded by ADR-0005; ADR-0105's terminal is a byte stream with `READABLE`, which is the form a later line editor needs |
| I-51 | Edge versus level for the personality | open question | answered: ADR-0106 uses `Level` throughout, with one `Edge` use for the request-pending poll |
| I-52 | `stream_read`/`stream_write` "synchronous with a `nonblocking` flag" (rule 6), `object_wait_one`, `object_get_signals`, timers, event objects not built | rule 6, open question | answered with a change: ADR-0101 makes stream transfers never block and drops the flag (tension listed); ADR-0106 needs timers for `nanosleep` and lists `object_wait_one` as optional |

## ADR-0031, ADR-0033, ADR-0040 to ADR-0046

| # | Item | Source | Disposition |
|---|---|---|---|
| I-53 | Demand paging and copy-on-write for `snapshot_process` | ADR-0031 follow-up | answered with a change: ADR-0102 recommends an eager copy at stage 6, copy-on-write as the revisit; the tension with ADR-0003 rule 3's wording is listed |
| I-54 | 255 live processes (8-bit ASIDs) | ADR-0031 | open: W1 to W3 and W10 run a handful of processes at once; W4's parallel configure is T2; ADR-0102 notes that a fork-heavy T2 run may approach it and names the measurement |
| I-55 | Replace the ramdisk with the filesystem; dynamic linking and `ET_DYN` outside ADR-0033 | ADR-0033 follow-up, open question | answered: ADR-0108 (images from files; static `ET_EXEC` only stays) |
| I-56 | Per-type duplicability for `snapshot_process` | ADR-0040 follow-up | answered: ADR-0102 |
| I-57 | Whether 64 KiB is enough for `spawn`'s buffer: measure argv at stage 6 | ADR-0042 open question | measured later: ADR-0110 logs the largest `execve` argument block in each workload |
| I-58 | Whether an image handle should stay valid as a read-only memory object | ADR-0043 open question | answered: ADR-0108: an image is created from a memory object and holds it; the handle types stay apart |
| I-59 | `Fault` exception class to signal table; the signal for `reason = Limit` | ADR-0046 open questions | answered: ADR-0106 |
| I-60 | A shell's need for job-level `SUSPENDED`, which brings `job_suspend` in | ADR-0046 revisit trigger | answered: ADR-0105 brings `job_suspend` and `job_resume` in for Ctrl-Z, `fg` and `bg`, outside the T1 acceptance tests |
| I-61 | 64 request-queue entries against a real signal storm | ADR-0046 open question | open: nothing measured; ADR-0106 maps `LimitReached` from `process_signal` to `EAGAIN` |

## ADR-0050 to ADR-0057, ADR-0060, ADR-0080 (the "Deferred to stage 6" sections)

| # | Item | Source | Disposition |
|---|---|---|---|
| I-62 | What a `snapshot_process` child does without channels (INV-0003-7); how a `fork` child reconnects; the handoff object sketched in TASK-10 and not introduced | ADR-0050, ADR-0055 | answered: ADR-0103 chooses an explicit move list on `snapshot_process` over the handoff; ADR-0102 defines the call |
| I-63 | Byte streams for `pipe()`: a separate object or a mode of the channel | ADR-0050 | answered: ADR-0101, a separate object |
| I-64 | ADR-0055's one requirement: an open file description with a shared position, behind `dup`, `fork` and fds passed at `spawn` | ADR-0050 to ADR-0057, ADR-0070 to ADR-0072, ADR-0080 | answered: ADR-0103 |
| I-65 | What `snapshot_process` does to a mapping of a memory object | ADR-0051 | answered: ADR-0102: every mapping is copied into a fresh object; the child shares no memory with its parent |
| I-66 | `mmap` for `libposix` | ADR-0051 | answered: ADR-0100 and ADR-0103 (anonymous private mappings on memory objects; `MAP_SHARED` stays out) |
| I-67 | How C programs reach services through `libposix` | ADR-0052 | answered: ADR-0100, ADR-0103 |
| I-68 | The broker is started as services are; who builds a user session | ADR-0053 | answered: ADR-0104 |
| I-69 | `pselect` over file handles (regular files always ready) | ADR-0055 | answered: ADR-0106 |
| I-70 | `close` returning `EIO` through `status` | ADR-0055, ADR-0010 personality mapping | answered: ADR-0103 |
| I-71 | How far 896 connections go for a POSIX toolchain | ADR-0055 | answered in part: ADR-0103 counts connections per POSIX process and names the measurement; ADR-0107's table of a few entries keeps the count low |
| I-72 | `snapshot_process` must charge a full copy to the same job; `QuotaExceeded` to `errno` | ADR-0056 (proposed) | answered in part: ADR-0102's eager copy is that full charge by construction; the `errno` mapping (`ENOMEM`) is in ADR-0106's error table; ADR-0056 itself stays proposed and outside this task |
| I-73 | A file descriptor stays a `libposix` number that maps to a handle | ADR-0057 | answered: ADR-0103 |
| I-74 | A terminal service "must take the rule's form" (ADR-0006 rule 1: a byte stream with `READABLE`, `WRITABLE`, `PEER_CLOSED`, and `WouldBlock` when empty) | ADR-0060 tensions, follow-up, revisit trigger | answered: ADR-0105 |
| I-75 | `fsync` family not exported; `O_TMPFILE` to unnamed create; tcc's unlink-then-create pattern | ADR-0010 personality mapping and open question 1 | answered: ADR-0103 keeps the mapping and leaves tcc's pattern as written (open question stays with ADR-0010) |
| I-76 | Symbolic links followed by `libposix`; `..` lexical; `st_nlink`, `st_ino`, `st_atime` synthesis; `umask` carried through the spawn protocol | ADR-0013 rule 8, ADR-0014 rules 4 to 10 | answered: ADR-0107 (resolver, `umask` in the bootstrap record), ADR-0103 (`stat` synthesis) |

## `docs/design.md` and `docs/backlog.md`

| # | Item | Source | Disposition |
|---|---|---|---|
| I-77 | "Not built at stage 3: `job_suspend`, `job_resume`, `object_wait_one`, `object_get_signals`, timers, event objects, byte streams, channels, exception channels and `snapshot_process`" (channels and event objects came with stage 4) | design.md | answered: ADR-0101 (streams), ADR-0102 (snapshot), ADR-0105 (`job_suspend`/`job_resume`), ADR-0106 (timers, `object_get_signals` for `WNOHANG`); exception channels stay unbuilt (I-21) |
| I-78 | "The `posix` bit is stored and unused until stage 6" | design.md | answered: ADR-0104 says who creates `posix` jobs |
| I-79 | Stage 6 roadmap row: `libposix` T1, port of tcc, make, dash; done when `tcc hello.c && ./a.out` runs inside Zoraya | design.md | answered: ADR-0109, ADR-0110 |
| I-80 | User copies are byte at a time (3.2 µs a KiB under HVF); try word-sized copies with a measurement | backlog | open: not a stage 6 decision; ADR-0102 and ADR-0108 note that their copy costs scale with it |
| I-81 | `zosh` `slots` cannot list a table (R-39) | backlog | open: INV-0002-5's test needs a way to enumerate a child's table; ADR-0107 says the test kernel does it, as INV-0003-1's test does |

## Items this task adds

| # | Item | Disposition |
|---|---|---|
| I-82 | The stage 5 terminal (TASK-14, ADR-0090 to ADR-0099) is designed in parallel and is not on `main`; TASK-14 asks that it "must not preclude job control and a controlling terminal" | ADR-0105 states the interface the POSIX side needs from any terminal, so that both the kernel console and the GUI terminal can provide it through one line-discipline service |
| I-83 | `make test` must not need the network, while the ported programs' sources are third-party | ADR-0109 and ADR-0110: a fetch step with pinned hashes, outside `make test`, like the toolchain install |
