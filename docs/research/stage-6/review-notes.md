# Independent review of the TASK-15 ADRs (ADR-0100 to ADR-0110)

Reviewer: a subagent that did not write ADR-0100 to ADR-0110, `inputs.md`, `sources.md` or `plan.md`. Reviewed on
branch `claude/task-15-stage-6-design-euw99h` at 78072b8, on 2026-11-02. Read in full: CLAUDE.md, ADR-0000, ADR-0001,
TASK-15, the eleven ADRs and their three companions, ADR-0002 to ADR-0006, ADR-0010 (Recommendation and open
questions), ADR-0013, ADR-0014 (Recommendation), ADR-0031, ADR-0033, ADR-0034, ADR-0040 to ADR-0043, ADR-0046,
ADR-0050 to ADR-0053, ADR-0055, ADR-0057, ADR-0060, ADR-0080, `docs/adr/index.md`, `docs/adr/invariants.md`, the
stage-4 review, and the cited sections of `task-02/`, `task-02b/`, `fs/requirements.md`, `alternatives-postmortem.md`
and the DM/GR/CP/X entries listed under "What was checked". Sixteen `sources.md` entries were fetched (list at the
end). `cargo run -q -p zoraya-adr-check -- docs` reports exactly the eleven expected `INV-0001-5 ... is not listed`
errors and the 17 pre-existing `not yet applicable (stage 6)` warnings; nothing else.

Severity: **error** means the design as written contradicts itself or an accepted ADR, cannot be built, or states
something false. **imprecision** means it is underspecified, overclaims, or leaves a tension unlisted. **note** is
minor. The Disposition column is for the author.

## The five walkthroughs

1. **`fork()` of dash (ADR-0102/0103/0104/0107): does not work as written.** The sequence prepare → `share` → move
   list → `snapshot_process` → commit → child start-up is coherent, and the move list does close the reconnection
   question. Four steps rely on something no ADR provides: (a) the child's copied memory holds the parent's handle
   *values*, which under ADR-0040/0057 are `slot | generation << 16`; nothing says the child's table keeps the
   parent's generations for duplicated slots or what generation a moved handle gets, so every handle value in the
   child's tables may be stale (finding 3); (b) the child's process job handle is "moved" once (ADR-0104 item 4) but
   three parties need it, and its listed rights have no `DUPLICATE` or `TRANSFER` (finding 4); (c) the child needs
   a handle to *itself* to subscribe `REQUEST_PENDING` and no ADR gives one; a duplicate of the parent's self handle
   names the parent (finding 5); (d) which call makes the child's terminal connection, `share` (ADR-0103 item 8) or
   the tty `clone(group pid, ...)` (ADR-0105 item 4), and with which group tag, is unstated (finding 19). The names
   `fork_prepare`/`fork_commit` do not exist in ADR-0104's protocol (finding 18).
2. **`exec` of `/bin/sh` by make with the parent's `waitpid` (ADR-0104): works as written**, given findings 4 and 5
   are fixed. `spawn_prepare(Inherit)` → `spawn` into the process job → `spawn_commit`; make waits on the job's
   `TERMINATED`, which the kernel asserts only when the job's live count reaches 0 (`kernel/src/job.rs`,
   `add_live`), so the handover old-process → new-process never empties the job; `reap(pid)` returns the current
   process's record. The attribute block must be built from POSIX dispositions, not kernel ones (finding 25), and
   the child's `JOB` handle needs `CREATE_PROCESS` for the `exec` (finding 4).
3. **Ctrl-C during `make -j4` (W9a, ADR-0105/0106): fails before it starts.** Interactive dash's `forkchild`
   calls `xxtcsetpgrp(pgrp)` from the child, which is in the new, not-yet-foreground group, and `waitforjob` calls
   `xxtcsetpgrp(rootpid)` from the shell while the job's group is foreground (both quoted from [D1] in this review).
   ADR-0105 item 7 returns `EPERM` to both, and dash's `xtcsetpgrp` then calls `sh_error`. So in W5 and W9a the job
   never gets the terminal, and after it the shell cannot take it back (finding 1). With item 7 fixed, the rest
   holds: `job_signal(fg_job, Interrupt, 2)` reaches the 7 makes and 4 recipes in the group job; recipes end by
   kernel disposition; each make's `port_wait` returns `REQUEST_PENDING` (which needs the self handle of finding
   5), `fatal_error_signal` runs with its nested `wait4` as nested `port_wait` plus `reap`, re-raise is
   `process_exit_with_reason(Terminated, 2)`, dash sees `WTERMSIG == SIGINT`, raises on itself, `$?` = 130.
4. **The FIFO jobserver (ADR-0107/0101): works as written except for the mode.** The three open behaviours fall
   out of items 14 and 15 and ADR-0101's end rules; the lost-race `EAGAIN`, `unlink` while open and end of file are
   right. `mkfifo` cannot convey `S_IFIFO`: the as-built `Open` message has `flags`, `rights`, `path` and no mode
   (`crates/zoraya-proto-file/src/lib.rs`, lines 118 to 122), and no ADR adds one (finding 6). Holding an `open`
   request unanswered is a new behaviour of ADR-0055's loop that the ADR does not list (tensions table).
5. **`tcc hello.c && ./a.out` (ADR-0108/0109): fails at `./a.out` as written.** `image_create`, the two copies,
   `zoraya-elf`'s acceptance of tcc's output (text at `ELF_START_ADDR` 0x400000 = `USER_LOW`, 64 KiB alignment
   satisfies `vaddr % 4096 == offset % 4096`, non-`PT_LOAD` headers are skipped, confirmed in `arm64-link.c` and
   `crates/zoraya-elf/src/lib.rs`), the startup block with program headers and musl's `_start_c` all hold. But tcc
   creates `a.out` with `open(O_WRONLY|O_CREAT|O_TRUNC, 0777)`; the create request carries no mode (finding 6), so
   the file is `0o644` and ADR-0108 item 2's execute-bit check refuses `execve` with `EACCES`. The auxiliary vector
   also lacks `AT_PHENT`, which musl's static TLS set-up reads (finding 13; harmless for programs without TLS).

## Findings

| # | Severity | Location | Finding and evidence | Suggested fix | Disposition |
|---|---|---|---|---|---|
| 1 | error | ADR-0105 item 7, item 8, Tensions ("a deviation from [P10] that no measured program exercises") | The broker refuses `tcsetpgrp` unless "the caller is in the foreground group or has no foreground set". dash's `forkchild` (fetched [D1]): `if (mode == FORK_FG) xxtcsetpgrp(pgrp);` runs in the child after `setpgid(0, pgrp)`, so the caller is in the new group, not the foreground one; `waitforjob` runs `xxtcsetpgrp(rootpid)` in the shell while the job's group is foreground; `xtcsetpgrp` wraps the call in `sigblockall`, which is why Linux lets a background caller through (SIGTTOU blocked, [P10]), and calls `sh_error("Cannot set tty process group")` on failure. Both calls get `EPERM` here, so every foreground job in W5 and the `make -j4` of W9a abort, and the shell cannot retake the terminal. The claim that no measured program exercises the background case is false: every interactive foreground job does, twice. | Allow `tcsetpgrp` from any process of the session whose group is a group of the session (Linux's rule with SIGTTOU ignored or blocked), or at least from the session's shell and from a process whose group is the one it names. Restate the deviation. Add the two dash calls to INV-0105-2's test. | Fixed. ADR-0104 item 10 and ADR-0105 item 7: any process of the session may make a group of the session the foreground group (Linux's rule with SIGTTOU blocked or ignored); the SIGTTOU deviation restated; dash's two calls added to INV-0105-2's test. |
| 2 | error | ADR-0106 item 9; ADR-0105 item 8; ADR-0104 item 13 | `kill(pid, 19 or 20)` "(default disposition) as `job_suspend`" and "`SIGTSTP` with a handler is `Custom(20)` delivered" need the *target's* POSIX disposition. The sender does not know it; the broker "never knows ... signal handlers" (ADR-0104 item 13); the kernel disposition of slot 20 is `Ignore` by ADR-0106 item 1, so `job_signal(Custom(20))` on a default target does nothing. No party can choose between the two paths. `SIGCONT` with a handler is never delivered at all (item 9 maps 18 to `job_resume` only). | Either the broker always `job_suspend`s and also sends `Custom(20)`, and the stopped process's handler runs at resume (state this and its deviation), or `sigaction` for 18 to 22 registers the disposition with the broker. Say the same for `SIGCONT`. | Fixed. ADR-0104 item 8a registers the POSIX disposition of SIGTSTP, SIGTTIN, SIGTTOU and SIGCONT with the broker from sigaction; item 9 acts on it (suspend, Custom(n), or nothing; SIGCONT always resumes and also delivers to a handler); ADR-0105 items 3 and 8 and ADR-0106 items 1 and 9 follow. |
| 3 | error | ADR-0102 item 3, item 4, "The `fork()` wrapper"; against ADR-0040 Representation, ADR-0057 | A handle value is `slot` plus a generation (ADR-0040; 48 bits under ADR-0057), and `libposix`'s copied tables hold the parent's *values*. The ADR says duplicates land "at the same slot" and moves "at `slot`" but never says the child's slots carry the parent's generations, nor what generation a moved handle receives in a table that is not "otherwise empty" (ADR-0040: `spawn` installs "with generation 0, in a table that is otherwise empty"). If the child's table starts at generation 0, every copied value with a nonzero generation is stale under INV-0040-1. The stage-4 review raised exactly this (S12) and it is not addressed. | State that the child's table copies each slot's generation, and that a moved handle is installed at the generation the child's slot then has, with the value the child computes; or install moved handles at generation 0 in slots the parent proves fresh. Add it to INV-0102-2's test. | Fixed. ADR-0102 items 3 and 4: duplicated slots keep the parent's generation, a moved handle takes the parent's generation of its slot; INV-0102-7 added, INV-0102-2's check extended; tension with ADR-0040/0057 listed. |
| 4 | error | ADR-0104 item 2, item 4, item 6, item 11; ADR-0107 item 5; against ADR-0040 Transfer/Duplication | The process job handle is created by the broker, kept in the pid table ("process job handle (`CREATE_PROCESS \| SIGNAL \| KILL \| WAIT`)"), "moved" to the parent in `spawn_prepare`, needed by the parent for `waitpid` ("a `port_wait` over the parent's children's job handles", item 11) and placed in the child at slot `JOB` (ADR-0107 item 5) with `CREATE_PROCESS` for `exec` (item 6). One moved handle cannot be in three tables, and the listed rights have neither `DUPLICATE` nor `TRANSFER`; under ADR-0040 a move needs `TRANSFER` and a duplicate `DUPLICATE`. The rights of the child's `JOB` handle are not stated anywhere. | Give the process job handle `DUPLICATE \| TRANSFER` and say who duplicates for whom: broker keeps one, parent gets one with `WAIT \| SIGNAL \| KILL \| TRANSFER \| DUPLICATE`, child gets one with `CREATE_PROCESS` (and no `KILL` or `SIGNAL` if the child must not signal itself through the job). | Fixed. ADR-0104 items 2 and 4: the broker keeps the full-rights process job handle and returns two duplicates, parent_job (WAIT|DUPLICATE|TRANSFER) and child_job (CREATE_PROCESS|TRANSFER); ADR-0107 item 5 names the child's slot PROCESS_JOB with those rights. |
| 5 | error | ADR-0106 item 3, item 5; ADR-0004 rule 2 (as relied on); INV-0002-5; ADR-0107 item 5 | Every wait point subscribes "the process's own handle" for `REQUEST_PENDING`. No POSIX process is given a handle to itself: INV-0002-5's list is namespace entries, broker channel, image, job and fd slots; ADR-0107's fixed slots are `BOOTSTRAP`, `BROKER`, `JOB`; the first-process convention (ADR-0043) has console, job, directory, power; `grep -rn process_self crates kernel docs` finds nothing. A `snapshot_process` child that duplicated a self handle from its parent would hold the *parent's* process handle and observe the parent's requests. Without a self handle no handler can ever run at a wait point, and INV-0106-3 and INV-0106-4 cannot pass. | Add the process's own handle (with `WAIT`) to the bootstrap handles and INV-0002-5's list, and have the kernel give the snapshot child a self handle at a fixed slot (or add a `process_self` call, a new kernel facility to record). | Fixed. A SELF slot: ADR-0107 item 5 (the spawner sends a WAIT duplicate of the process handle as a second bootstrap message after spawn), ADR-0102 item 3 (snapshot_process installs a handle to the child at self_slot), ADR-0106 item 3, INV-0102-7; INV-0002-5's list change listed in ADR-0107. |
| 6 | error | ADR-0103 item 12; ADR-0107 items 13, 17; ADR-0108 item 2; against ADR-0014 rule 6 as built (ADR-0080) | `open(path, O_CREAT, mode)`, `mkfifo` ("`open` with `CREATE \| EXCLUSIVE` and `posix_mode = S_IFIFO \| ...`") and tcc's output creation all need the mode in the create request. ADR-0014 rule 6 says "A create request carries the mode", but the protocol as built and recorded by ADR-0080 does not: `message Open<'a> = 1, handles 1 { flags, rights, path }` (`crates/zoraya-proto-file/src/lib.rs` 118 to 122); only `MakeDirectory` and `SetAttributes` carry `mode`. No new ADR adds a mode to `open` or says `libposix` sends `SetAttributes` after every create (one more round trip for the 1,317 `O_CREAT\|O_TRUNC` opens). Consequence for the roadmap criterion: tcc's `a.out` is `0o644`, and ADR-0108 item 2 refuses it with `EACCES`. For FIFOs `S_IFIFO` cannot be expressed at all. | In ADR-0103: a new `open` ordinal with a mode (ADR-0052's rule), or `SetAttributes` after a create with its cost stated. Record the tension with ADR-0014 rule 6 and ADR-0080. | Fixed. ADR-0103 item 1 adds open_with_mode (a new ordinal) used for every O_CREAT open and by mkfifo (ADR-0107 item 13); ADR-0108 Context says so; tension with ADR-0014 rule 6 and ADR-0080 listed in ADR-0103. |
| 7 | error | ADR-0109 item 6 ("a `.config` with exactly the T1 applets of `applets-by-tier.tsv`, `cat head ls mv rm sleep touch true uname`"); ADR-0110 items 2, 4; against ADR-0005 | `tools/posix-surface/results/applets-by-tier.tsv` row T1 is `cat head ls mv rm sleep touch true uname` (plus busybox `diff tr`). W10, an acceptance workload of ADR-0005 and ADR-0110, runs `sort`, `wc`, `yes` and `head` (`tools/posix-surface/workloads/w10/w10.sh`: `./mktab 4 \| sort \| wc -l`, `yes w10 \| head -n 3`, `cat ... \| wc -l`). ADR-0110 item 4's W9 script runs `wc` and `sed`. `sort`, `wc`, `sed` are T2 rows of the TSV; `yes` is in no row. The T1 image as specified cannot run W10 or the W9 check. The TSV predates TASK-02b, which ADR-0005 did not notice; this is an unlisted tension with ADR-0005's "the TSV is authoritative". | Add `sort wc yes sed` (and anything else the W8 to W10 scripts need) to the T1 build with the TASK-02b evidence, as ADR-0005 did for `writev`; list the tension; or rewrite the W9 check without `wc`/`sed`. | Fixed. ADR-0109 item 6 adds sort, wc and yes to the T1 build on TASK-02b's W10 evidence, with the ADR-0005 tension listed; ADR-0110 item 4's W9 check rewritten in dash alone; inputs.md I-39 updated. |
| 8 | error | ADR-0102 item 3 ("INV-0003-7 covers the rule"), item 4; against ADR-0003 INV-0003-7 | INV-0003-7: "`snapshot_process` never transfers a handle type marked non-duplicable in the `abi` crate (at least `Port`); such slots are empty in the child." The move list transfers channel ends, a non-duplicable type (ADR-0050: no `DUPLICATE`), into the child's slots. The invariant's statement is false under the recommendation, its check ("a test per non-duplicable type ... asserts the child's slot is empty") no longer describes the design, and the ADR says the invariant "covers" it instead of listing a tension. | List the tension; propose INV-0003-7's replacement: no non-duplicable handle is *duplicated*; a listed move is the only way such a handle reaches the child, and an unlisted slot of that type is empty. | Fixed. ADR-0102 lists the tension and proposes INV-0102-6 as INV-0003-7's replacement. |
| 9 | error | ADR-0104 item 1 ("`init` creates the broker's job with the `posix` flag; that job is the *session job*"), item 9 (`kill(-1)` → `job_signal` on the session job); ADR-0105 item 3 (hangup → `job_signal(session_job, Hangup, 1)`); ADR-0107 item 12 (`tmpd` "in a job of its own inside the session job") | The broker runs *in* the session job, and `tmpd`'s job is inside it. `kill(-1, SIGTERM)` from any POSIX process and the terminal's hangup therefore `process_signal` the broker and `tmpd`, whose dispositions are the default `Terminate` (ADR-0004 rule 2, INV-0004-5). The broker dies, and with it "the session's POSIX state" (ADR-0104's own words), and `/tmp` goes. Nothing sets the two services' dispositions, and `kill(-1)` on Linux excludes the caller and init, which the mapping does not. | Put the broker (and `tmpd`) in a job beside the session job, or make the session job a child job that holds only groups, with the broker above it; exclude the caller from `kill(-1)` by signalling each group job but the caller's process job. | Fixed. ADR-0104 item 1: the broker's job holds two child jobs, the session job (POSIX processes only) and tmpd's; kill(-1) signals every process job but the caller's (item 9); ADR-0105 item 3 and ADR-0107 item 12 follow. |
| 10 | error | ADR-0100 Option B, Cost | "about 90 OS-facing functions (process 51, file 40, signal 12, dir 7, tty 6, memory 6, time 2, `writev`/`readv`)": the parenthesis sums to 126 (51+40+12+7+6+6+2+2). The category totals are right against `libc-surface.csv` (checked: 248 rows, string 53, process 51, stdio 43, file 40, dir 7, signal 12, time 11, other 11, memory 6, tty 6, locale 4, threads 3, network 1; 185 with a `dynamic` entry, 63 without), so the 90 is the false number. | Replace with the sum, or state which process and file rows are pure (for example `getopt`, `qsort` are not in these categories; `vfork`, `wait*`, `exec*` are) and recount. | Fixed. ADR-0100 Option B's cost: the 126 rows named, the pure ones subtracted, about 110 as the estimate. |
| 11 | error | `inputs.md` rows I-26, I-32, I-33, I-51, I-52, I-69, I-75, I-77 | Dispositions not borne out by the ADR text. I-32: "ADR-0105 makes the terminal a byte stream, which cannot refuse a read by caller, so this sentence is deferred", but ADR-0105 items 4 and 8 implement `NotForeground` and the stop. I-51: "ADR-0106 uses `Level` throughout, with one `Edge` use for the request-pending poll"; ADR-0106 item 3 subscribes `REQUEST_PENDING` "(Level)" and names no `Edge` use. I-52 and I-77: "ADR-0106 needs timers for `nanosleep`"; ADR-0106 item 10: "no timer object is needed at stage 6". I-69 (`pselect` over file handles, regular files always ready): "answered: ADR-0106"; `grep -i "always ready\|regular file" docs/adr/0106-*.md docs/adr/0103-*.md` finds nothing. I-75 (`fsync` family, `O_TMPFILE`): "ADR-0103 keeps the mapping"; ADR-0103 does not mention `fsync` or `O_TMPFILE`. I-26: "ADR-0102 and ADR-0104" define the request-at-creation rule; ADR-0104 says nothing about it and ADR-0102 item 5 answers differently from ADR-0004's open question (copied dispositions decide, not "only if `Deliver`"). I-33: "the group handle passed at `spawn` and `snapshot_process`"; ADR-0104 Tensions: "`spawn`'s `group` field is unused by `libposix`". TASK-15's definition of done requires each item to be answered or left open. | Correct each row to what the ADRs say; for I-69 and I-75 either add the sentences to ADR-0103/ADR-0106 or mark the rows open. | Fixed. Rows I-26, I-32, I-33, I-39, I-51, I-52, I-69, I-75, I-77 rewritten to what the ADRs say; ADR-0103 item 14 and ADR-0106 item 3 gained the sentences I-69 and I-75 needed. |
| 12 | error | ADR-0101 Peers, Signals ("`PEER_CLOSED` as above"), Library (blocking `write` retries on `WRITABLE \| PEER_CLOSED`) | `PEER_CLOSED` is defined for the read end only ("asserted when the last write-end handle closes"). The blocking `write` loop waits for `WRITABLE \| PEER_CLOSED` on the *write* end to wake when the reader goes; nothing asserts `PEER_CLOSED` on a write end. In `yes \| head` with the ring full, `yes` blocks in `port_wait`; when `head` exits, `WRITABLE` ("a read-end handle exists") goes false and no signal rises, so `yes` never wakes and W10 hangs. INV-0101-2 tests `PeerClosed` from the call, not the signal. | Define `PEER_CLOSED` on the write end as "no read-end handle exists", add it to the signals paragraph and to INV-0101-2's check. | Fixed. ADR-0101: PEER_CLOSED defined on the write end too, in Peers, Signals and INV-0101-2. |
| 13 | imprecision | ADR-0100 item 4 (auxiliary vector "`AT_PAGESZ`, `AT_RANDOM`, `AT_NULL`"); ADR-0109 item 4 (`AT_PAGESZ`, `AT_PHDR`, `AT_PHNUM`, `AT_RANDOM`, `AT_NULL`) | The two ADRs give different auxiliary vectors for the same `crt1`; ADR-0100's lacks `AT_PHDR`/`AT_PHNUM`, which ADR-0108 item 5 says `__init_tls` needs. Both lack `AT_PHENT`: musl's `static_init_tls` walks the headers with `aux[AT_PHENT]` as the stride `[from the reviewer's memory of musl `src/env/__init_tls.c`, not fetched; verify]`, so with 0 the `PT_TLS` scan never runs. Harmless for programs without TLS, wrong for any with it. | One list, in ADR-0109, cross-referenced from ADR-0100; add `AT_PHENT` (56). | Fixed. One list in ADR-0109 item 4 with AT_PHENT 56, verified against musl's __init_tls.c ([M10], fetched); ADR-0100 item 4 refers to it. |
| 14 | imprecision | ADR-0100 item 2 (replaced: `src/{...,thread,...}`), item 5 ("musl's TLS set-up (`__init_tls`) is kept ... `__set_thread_area` is `msr tpidr_el0`") | `__set_thread_area` lives in `src/thread/aarch64/__set_thread_area.s`, a directory item 2 replaces wholesale; `__init_tls` (`src/env/__init_tls.c`) calls `SYS_mmap` when the TLS block exceeds `builtin_tls` `[not fetched; verify]`, and `mmap` is not in item 2's shim list. The cut is inconsistent for TLS. | Name the two files as retained exceptions, and either add `SYS_mmap` to the shim or state that the builtin TLS always suffices for T1 (and why). | Fixed. ADR-0100 item 2 retains __init_tls.c and __set_thread_area.s as exceptions and adds SYS_mmap and SYS_set_tid_address to the shim; item 5 says what __init_tls does ([M10]). |
| 15 | imprecision | ADR-0100 item 7, INV-0100-1, INV-0100-2 | The checks read `libc.a`'s members "with `zoraya-elf`" to list defined global symbols and to "disassemble the archive's members ... and find `svc`". `zoraya-elf` parses little-endian AArch64 `ET_EXEC` program headers only (`crates/zoraya-elf/src/lib.rs`: `Error::Unsupported` for anything else; no section headers, no symbol table, no instruction decoding); archive members are `ET_REL`. Both checks need an `ar` reader, a `.symtab` reader and an instruction scan that do not exist and are not planned in `plan.md`. Also item 7 excludes names beginning with `_`, which drops `__assert_fail`, `_exit` and `_Exit` from the check although they are tier rows (`libc-surface.csv`). | Name the tool's real scope (an `ar`/`ET_REL` reader in `tools/posix-surface-check`, a byte scan for the `svc #0` encoding `0xd4000001`), or make `zoraya-elf` grow and say so in `plan.md`; list the `_`-prefixed tier rows as checked by link. | Fixed. ADR-0100 item 7 and INV-0100-2: the tool has its own ar and ET_REL symbol-table reader and a byte scan for 0xd4000001; the _-prefixed tier rows are checked by link; plan.md 6b names the tool. |
| 16 | imprecision | ADR-0100 (no "Tensions with accepted ADRs" section); item 7; ADR-0005 INV-0005-1, INV-0005-2 | INV-0005-1's generator rule is "the CSV rows of the accepted tier, minus the struck set-uid rows, plus `writev`". Item 7 strikes four more rows (`dlopen`, `dlsym`, `dlclose`, `dlerror`, all `T1` in the CSV) and so changes the generator; it also resolves ADR-0005's own inconsistency (`dlopen` is both a tier row and on INV-0005-2's deny list, so "disjoint by construction" was false). ADR-0100 is the only one of the eleven with no tensions list. | Add the section; list INV-0005-1's rule change and the `dlopen` inconsistency. | Fixed. ADR-0100 gained a Tensions section with INV-0005-1's rule change and the dlopen clash. |
| 17 | imprecision | ADR-0100 Option B "Evidence for" and "Precedent" ("Fuchsia's libc over fdio (shipping on Nest Hubs, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`)") | The postmortem section never mentions fdio or musl (`grep -n -i "fdio\|musl" docs/research/alternatives-postmortem.md` is empty); the Context marks the musl basis `[outside evidence: fuchsia.dev libc page, not fetched]`, but the option text cites the postmortem for it. ADR-0001 rule 5: a citation must support the claim. | Cite the outside-evidence note in both places, or drop "over fdio" from the precedent. | Fixed. The musl-over-fdio claim is marked outside evidence in Option B and its Precedent. |
| 18 | imprecision | ADR-0102 "The `fork()` wrapper" vs ADR-0104 Protocol, ADR-0103 item 13, ADR-0105 | Three contradictions between the new ADRs. (a) ADR-0102 calls the broker's methods `fork_prepare` and `fork_commit`; ADR-0104 defines `spawn_prepare` and `spawn_commit` and says `fork` calls them. (b) ADR-0102 counts "about 10 connections for a shell (`/` and `/tmp` directory connections, cwd, three stdio descriptions, the terminal control connection)"; ADR-0103 item 13: "Three stdio fds on the terminal are streams. So a quiet shell holds about 4 connections." (c) "terminal control connection" is the broker's privileged connection in ADR-0105 item 5; a process holds a "terminal connection" (item 4). | One name per method and per connection; one count. | Fixed. ADR-0102 uses spawn_prepare/spawn_commit, counts about 4 connections plus one per file, and says 'terminal connection'. |
| 19 | imprecision | ADR-0103 item 8 ("The parent `share`s every `File`, `Dir`, FIFO and terminal connection"); ADR-0105 item 4 (`clone(group pid, server_end)`); ADR-0107 "Who builds it" | `share` is a filesystem request (ADR-0103 item 1); the terminal protocol has `clone` with a group tag, and the tag is what `NotForeground` compares. Which call `fork` uses for the tty connection, and which group it tags (the child's prepared group is known from `spawn_prepare`, so it can be right, but the text says `share`), is unstated. With the parent's tag a foreground child's `read` would be `NotForeground` and stop itself. | Say `fork` and `spawn` use the tty `clone` with the group `spawn_prepare` returned. | Fixed. ADR-0103 items 7 and 8: the terminal connection is cloned by the tty protocol with the group spawn_prepare returned. |
| 20 | imprecision | ADR-0103 item 3 ("the `readdir` cookie stays in the library (ADR-0080), so two fds on one directory description each walk from their own cookie") | ADR-0080: "A `read_directory` cookie is valid for the connection's last reply: the service keeps one name per connection, because the engine continues after a name." Two cookies on one connection are not supported, and POSIX makes `dup`ed directory fds share one position anyway (the description). | Share the position per description as for files; drop the per-fd walk. | Fixed. ADR-0103 item 3: one cookie per description entry in the library; a fork or spawn child's directory stream restarts, stated as a deviation. |
| 21 | imprecision | ADR-0103 item 4 (`O_NONBLOCK` as a per-description-entry flag) | [P16] (fetched): `F_SETFL` flags "are shared by duplicates of one open file description", across processes after `fork`. Here each process's copy of a `share`d description has its own flag. Harmless for make, which toggles `O_NONBLOCK` around each read, but a deviation to state (and a kinder one than Linux's shared toggling). | State the deviation. | Fixed. ADR-0103 item 4 states the per-process O_NONBLOCK deviation; listed in tensions. |
| 22 | imprecision | ADR-0104 item 4 ("the dash patch of ADR-0109 passes `Join` or `New`"), item 8 ("a group job that is not yet a declared group"); ADR-0109 item 9 ("the `fork` group hint ... in `forkshell`, about 5 lines") | `fork()` takes no arguments, so how dash passes the hint (a Zoraya-only function, a global the wrapper reads, an environment of the call) is never said. Item 8's "not yet a declared group" is undefined: by item 1 every process job is inside a group job from `spawn_prepare`, so when is a group job not a declared group? | Name the hint mechanism (for example `zoraya_fork_group(Group)` before `fork`), and define "declared". | Fixed. ADR-0104 item 4 names zoraya_fork_group and the posix_spawn attribute path; item 8 drops 'declared group' and succeeds only as a no-op. |
| 23 | imprecision | ADR-0104 item 4 (`spawn_prepare(group, start_suspended)`), ADR-0109 item 9 ("`vforkexec` to `posix_spawn` with the group attribute for the interactive path"); against ADR-0003 rule 2 and follow-up | ADR-0003 rule 2: for dash's interactive `vforkexec` the parent spawns suspended, "performs group and terminal setup through the broker and the terminal handle, then resumes". Here `start_suspended` is a parameter of a broker request with no stated use, nothing resumes, and no one hands the terminal to a spawned foreground job (dash's child did it; under `posix_spawn` the child cannot). With finding 1 fixed the parent could call `tcsetpgrp` after `spawn`, but then the child may read the terminal before it is foreground. | Specify the interactive path end to end: `spawn` suspended, parent `tcsetpgrp`, `process_resume`; or drop `start_suspended` from the broker request. | Fixed. ADR-0104 item 5: spawn suspended, parent tcsetpgrp, commit, process_resume, through POSIX_SPAWN_TCSETPGRP; start_suspended dropped from spawn_prepare; ADR-0109 item 9 and ADR-0104's tensions updated. |
| 24 | imprecision | ADR-0106 item 2; against ADR-0004 personality mapping ("`sigprocmask` blocking a signal defers taking it (the kernel queue holds it)") | Two things. (a) The mechanism differs from ADR-0004's: a request is *taken* while blocked into a library pending set, and a `Terminate`-disposition signal is blocked by flipping the kernel disposition to `Deliver`. Not listed as a tension. (b) Cost: dash blocks *all* signals around every `sigsuspend` in `waitproc` and in `xtcsetpgrp` (`sigblockall`/`sigclearmask`, [D1]); for each of the roughly 20 `SIG_DFL` signals with `Terminate` that is a `process_set_disposition` per block and per unblock, four times per wait, so about 80 system calls per wait, times 3,751 child waits in the core runs; under TCG at 13 µs that is seconds on W4. The ADR's cost analysis counts only the poll. | List the tension. Either leave `Terminate` dispositions alone while blocked and accept the POSIX deviation (a blocked default signal still kills, which dash's `waitproc` tolerates since it only blocks around `sigsuspend`), or flip only the signals the process has ever received, or count the cost. | Fixed. ADR-0106 item 2: no disposition flipping; a blocked SIG_DFL terminating signal still terminates, stated as a deviation with the cost that motivated it; tension with ADR-0004's mapping listed. |
| 25 | imprecision | ADR-0104 item 6 ("the attribute block built from its dispositions (ignored stays `Ignore`, everything else `Terminate`)") | Stop and continue signals have the kernel disposition `Ignore` by ADR-0106 item 1 while their POSIX disposition is `SIG_DFL`. Built from kernel dispositions, the block marks 18 to 22 as ignored, ADR-0107's `ignored` set then reports them as `SIG_IGN` to the new image's `sigaction`, and a `kill -TSTP` on it is dropped. | Build the block and the `ignored` set from POSIX dispositions. | Fixed. ADR-0104 item 6 builds the block and the ignored set from POSIX dispositions. |
| 26 | imprecision | ADR-0104 item 10 ("a duplicate of that group job's handle with `SIGNAL`") vs ADR-0105 item 5 ("`foreground(group pid, job handle with SIGNAL and KILL)`"), item 3 (SUSP → `job_suspend(fg_job)`) | Two rights lists for the same handle; ADR-0105 needs the suspend right it assumes is `KILL`. | One list, in ADR-0104. | Fixed. One list, SIGNAL|KILL, in ADR-0104 item 10; ADR-0105 item 5 refers to it. |
| 27 | imprecision | ADR-0106 Option B ("dash checks for a pending interrupt at every command through a flag its handler sets ([D1]: `pending_sig`)") | [D1] is `src/jobs.c`. Fetched: `pending_sig` is not defined there and is used only in `waitproc` (ends the `sigsuspend` loop) and `waitcmd` (`128 + pending_sig`). The "at every command" check is in `eval.c`, which is not a source. | Cite the file that has it, or fetch it. | Fixed. ADR-0106 Option B now cites dash's eval.c dotrap() at every evaltree node ([D3], fetched). |
| 28 | imprecision | ADR-0106 and ADR-0110, Considered options; ADR-0001 rule 5, template | ADR-0106's four options cite Linux, ADR-0004, musl and the Linux vDSO; no option is a non-Unix system from `alternatives-postmortem.md` (NT and seL4 appear only in Forces). ADR-0110's options are the TASK-02 harness, this repository's runner, `t_storage` and a host tool; no Linux baseline as such and no non-Unix precedent. | Add the precedents (NT console control, seL4 notifications, Zircon's `fuchsia.test` runner or Hubris's `humility` tests) or say why none applies. | Fixed. ADR-0106 Option A carries NT's fresh-thread form and Option C seL4 notifications; ADR-0110 Option A is named the Linux baseline, with Hubris and Fuchsia as precedents for B and C. |
| 29 | imprecision | ADR-0106 item 1 | Signal 16 (`SIGSTKFLT`, default terminate) is in neither the terminate list ("1 to 15 except 9, 24, 25, 26, 27, 29, 30, 31") nor the ignore list. It keeps ADR-0004's default `Terminate`, so the effect is right by accident. | Add 16. | Fixed. ADR-0106 item 1: 1 to 16 except 9. |
| 30 | imprecision | ADR-0107 Context ("`chdir` was absolute 32 times and relative once; ... (`task-02/callsite-analysis.md#feature-3-global-paths-path-cwd`)") | The counts are in `task-02/conflict-impact.md` X-1 ("`chdir` was called with an absolute path 32 times and a relative path once (`syscall-surface.csv`)", line 63); `callsite-analysis.md` feature 3 has no counts. | Cite `task-02/conflict-impact.md#x-1-...`. | Fixed. ADR-0107 cites task-02/conflict-impact.md X-1 for the chdir counts. |
| 31 | imprecision | ADR-0107 INV-0107-4 ("INV-0006-9's test") | INV-0006-9's three behaviours are: non-blocking read open succeeds at once; write open with an existing reader succeeds at once; `unlink` removes the name while open handles stay valid. INV-0107-4's third is the sub-make's read then `fcntl(O_NONBLOCK)`; the `unlink` behaviour is INV-0107-5. The claim to be the same test is off by one behaviour. | Say INV-0107-4 and -5 together are INV-0006-9's check. | Fixed. INV-0107-4 reworded; INV-0107-4 and -5 together are INV-0006-9's check. |
| 32 | imprecision | ADR-0108 item 1 ("rejecting ... a segment not readable, writable and executable") | Garbles ADR-0033: "a segment that is not readable, a segment both writable and executable". As written it reads as rejecting any readable segment. | Quote ADR-0033. | Fixed. ADR-0108 item 1 quotes ADR-0033's list. |
| 33 | imprecision | ADR-0108 Context ("tcc unlinks its output and recreates it ... (`fs/requirements.md#4-metadata-read-and-written`)"; "Plan 9's `exec` reads the binary through its per-process namespace (`alternatives-postmortem.md#plan-9--inferno`)") | The unlink-then-create fact is in `fs/requirements.md` §2.2 ("tcc does not. tcc unlinks its output and recreates it in place"); §4 has the execute-bit fact only. The postmortem's Plan 9 section says Plan 9 "still used fork/exec rather than file writes" and describes per-process namespaces; it does not say `exec` reads through the namespace. | Cite §2.2; mark the Plan 9 sentence as the author's inference or drop it. | Fixed. ADR-0108 cites fs/requirements.md section 2.2 and marks the Plan 9 sentence as inference. |
| 34 | imprecision | ADR-0109 Context ("tcc's `configure` reports `aarch64-unknown-linux-gnu` through `config.guess` under musl (`task-02/posix-surface.md#key-observations`, item 7)") | Item 7 is about GNU hello's `configure` run with `CC=tcc`: "with tcc, `config.guess` reports `aarch64-unknown-linux-gnu` on musl". tcc's own `configure` has no `config.guess` ([T3]). | "hello's `configure` under `CC=tcc`". | Fixed. ADR-0109 Context: hello's configure under CC=tcc; tcc's own has no config.guess. |
| 35 | imprecision | ADR-0109 item 8 ("`--config-musl` is dropped: it only sets a dynamic linker path") | tcc's `configure` (fetched) also writes `#define CONFIG_TCC_MUSL 1` into `config.h` and `CONFIG_musl=yes` into `config.mak`; what `CONFIG_TCC_MUSL` changes elsewhere was not checked by the author ([T3] lists the option only) or by this review (the fetched `tcc.h` and `tccdefs.h` pages had no `musl`). "Only" is not supported. | "sets the dynamic linker path and `CONFIG_TCC_MUSL`, whose other effects the first toolchain task checks". | Fixed. ADR-0109 item 8: --config-musl sets the interpreter path and CONFIG_TCC_MUSL, whose other effects the first task checks. |
| 36 | imprecision | ADR-0109 item 6 and [T6] ("`libtcc1.a` for arm64 is `lib-arm64.o`, `stdatomic.o`, `atomic.o`, `builtin.o`, `alloca.o`, `alloca-bt.o`, `armflush.o`, `dsohandle.o`") | `lib/Makefile` (fetched): `OBJ-arm64 = $(ARM64_O) armflush.o $(LIN_O)`, where `COMMON_O` adds `runmain.o tcov.o` for native builds and backtrace/bcheck objects when enabled, and `LIN_O` adds `dsohandle.o` "on Linux". For a `Zoraya` target the Makefile's OS branch decides whether `dsohandle.o` is built: one more patch point item 7 does not list. | Name the `lib/Makefile` branch among the tcc patches. | Fixed. ADR-0109 item 6 names lib/Makefile's OS branch as a patch point. |
| 37 | imprecision | ADR-0101 Forces ("`SIGPIPE` by default is the part that did not [hold up] (DM-077)") | DM-077 is classified *tradeoff* ("shell pipelines depend on it"; relevance "low"), not a mistake. | "is a tradeoff (DM-077), kept out of the native ABI by ADR-0004 rule 7". | Fixed. ADR-0101 Forces: DM-077 as a tradeoff. |
| 38 | imprecision | ADR-0105 driver 2 ("CP-25's 'data path as bytes'") | CP-25's statement is "The native terminal protocol keeps data and control apart"; the quoted phrase does not occur. | Paraphrase without quotation marks. | Fixed. ADR-0105 driver 2 paraphrases CP-25. |
| 39 | imprecision | ADR-0102 item 1; ADR-0046 ("Job `TERMINATED` ... asserted exactly while the job's tree has no live process, a new job included, and cleared by `spawn`") | The broker's fresh process job has `TERMINATED` asserted (`kernel/src/job.rs`: a new source starts with it; `add_live` clears it on the first process). ADR-0102 must say `snapshot_process` clears it as `spawn` does, else a parent that subscribes the child's job after `fork` sees an immediate `TERMINATED`. | One sentence in item 1. | Fixed. ADR-0102 item 1: the call clears the job's TERMINATED as spawn does; listed in tensions. |
| 40 | imprecision | ADR-0102 item 3; against ADR-0040 Duplication ("`handle_duplicate(handle, rights)` needs `DUPLICATE`") | Duplicability is per *type* here ("Every slot whose type is duplicable is duplicated ... with the same rights"); ADR-0040's is per *right*. Whether a handle of a duplicable type that lacks `DUPLICATE` is duplicated by the snapshot (a right bypassed) or left empty (a POSIX child missing a handle) is unstated and is a tension not listed. | Decide (empty slot is the safe reading) and list it. | Fixed. ADR-0102 item 3 duplicates by the DUPLICATE right; a handle lacking it leaves an empty slot; listed in tensions and INV-0102-6. |
| 41 | imprecision | ADR-0102 "The `fork()` wrapper" (the child's start-up list) | The child's pid, ppid, pgid and `getpid()` cache must change to the `spawn_prepare` values; the list says only "write the moved handles ..., create a fresh port, re-subscribe nothing, drop the parent's child list". The reply is in copied memory, so it works, but say it. Likewise that the duplicated handles to the parent's *other* children's process jobs are what "drop the parent's child list" closes. | Two clauses. | Fixed. ADR-0102's wrapper paragraph names the pid facts the child takes from its copied memory and the sibling job handles it closes. |
| 42 | imprecision | ADR-0110 item 1, item 3 (`cd "$HOME"`) | The session's `environ` (`HOME`, `PATH`, `TERM`, `SHELL`) is defined nowhere: ADR-0104 item 12 and ADR-0110 item 1 give the command, not the vector; the measurement used `env -i PATH=<set> HOME=<run dir> TERM=dumb SHELL=<set>/sh` (`task-02/method.md`). With `HOME` unset dash's `cd "$HOME"` fails. | State the vector in ADR-0110 item 1 (`/etc/zoraya/session` or the broker's table). | Fixed. ADR-0104 item 12 and ADR-0110 item 1 state the environment vector (PATH, HOME, TERM, SHELL plus the session file's). |
| 43 | imprecision | ADR-0110 INV-0110-1, INV-0110-3; ADR-0105 INV-0105-4; ADR-0103 INV-0103-2 | INV-0110-1 ("every workload runs ... each ending with its marker") and INV-0110-3 ("`make measure-w4` prints ...") are test goals and tooling outputs, not rules that follow from a decision. INV-0105-4's "`grep` finds no termios constant in `kernel/src`" names no identifiers. INV-0103-2's "the service's connection count is what it was before" has no observable: the protocol has no connection-count request (`status` reports failure only). | Keep INV-0110-1 as the acceptance test it is but word it as "`make test` fails unless ..."; drop INV-0110-3 or make it "the report has every counter"; list the identifiers (`ICANON`, `ECHO`, `VINTR`, ...); add a count to `status` or read it through the host test's internal interface. | Fixed. INV-0110-1 worded as a make test failure condition; INV-0110-3 as a property of a report; INV-0105-4 lists the identifiers; INV-0103-2 reads the connection count through the fs-service host test interface. |
| 44 | imprecision | ADR-0103 INV-0103-3; ADR-0104 INV-0104-5; against INV-0002-3 | INV-0002-3 forbids broker IPC on `open`, `read`, `write`, `close`, `stat`, `lseek`, `mmap`, `pipe`. INV-0103-3 omits `stat`; INV-0104-5 omits `stat` and `mmap`. | Copy the list. | Fixed. INV-0103-3 and INV-0104-5 copy INV-0002-3's list. |
| 45 | imprecision | ADR-0102 item 4 ("Up to `MAX_SPAWN_HANDLES` entries"); ADR-0103 item 7; `crates/zoraya-abi/src/spawn.rs` (`MAX_SPAWN_HANDLES: usize = 64`) | A `spawn` or move list carries at most 64 handles. A process with more than about 60 inherited fds plus its namespace handles cannot `spawn` or `fork`, while `OPEN_MAX` is 512. Linux has no such bound. Unlisted against ADR-0042. | State the limit and its `errno` (`E2BIG`?), or raise `MAX_SPAWN_HANDLES` by ADR. | Fixed. ADR-0102 item 4 and ADR-0103 item 7 state the 64-handle bound and its errno; listed against ADR-0042. |
| 46 | imprecision | ADR-0109 item 5; ADR-0042 ("Nothing may follow", `STARTUP_BLOCK_MAX = MAX_SPAWN_SIZE + PAGE`) | The program header table is appended "after the vectors" in a block whose room is one page beyond the 64 KiB `spawn` buffer; with argv plus environment near 64 KiB the headers (up to 8 × 56 bytes) must still fit. Not stated; nor that ADR-0042's "Nothing may follow" sentence changes. | State the room and the sentence change. | Fixed. ADR-0109 item 5 states the room and replaces ADR-0042's 'Nothing may follow'. |
| 47 | note | ADR-0107 item 5 (fixed slot named `JOB` "above `OPEN_MAX`") | `zoraya_abi::JOB` is `Handle(2)` (ADR-0043's first-process convention). Same name, different slot. | Call it `PROCESS_JOB`. | Fixed. The slot is PROCESS_JOB (ADR-0107 item 5). |
| 48 | note | ADR-0107 item 14 ("`O_RDWR` is `EINVAL`") | Linux allows `O_RDWR` on a FIFO (POSIX leaves it undefined); a deviation to state, as item 15's is. | One clause. | Fixed. ADR-0107 item 14 states the O_RDWR deviation. |
| 49 | note | ADR-0104 Context ("`getpid` in W9 for the re-raise (... `task-02b/parallel-surface.md#new-syscalls-and-argument-patterns`)") | That table lists `getpid` only for W3 fifo; the re-raise's `kill(getpid(), SIGINT)` is in `event-timelines.md#w9a-...`. | Cite the timeline. | Fixed. ADR-0104 cites the W9a timeline for the re-raise. |
| 50 | note | ADR-0102 Depends on ADR-0056 (proposed); ADR-0102 items 7, 8 | A proposed ADR in a `Depends on` line; the stage-4 reviews asked that such lines be complete, and this one is, but the dependency on a decision not taken should be flagged. | "ADR-0056 (proposed)". | Fixed. ADR-0102's Depends line reads 'ADR-0056 (proposed)'. |

Cross-ADR contradictions among the eleven, collected: findings 13 (auxiliary vector), 18 (method names, connection count, "control connection"), 19 (`share` vs `clone` for the tty), 26 (foreground handle rights), 7 (applet list vs scripts), 2 (who knows a stop signal's disposition).

## Tensions with accepted ADRs

The authors list 46 tension bullets across ten of the eleven ADRs (counted from each "Tensions with accepted ADRs"
paragraph; ADR-0100 has none). Several bullets restate one tension (the ADR-0003 rule 3 wording appears in ADR-0102 and
ADR-0104; "the moved ends are fresh" appears in ADR-0102 and ADR-0103). This table lists every tension found, one row
each; "Listed by the author?" says whether the ADR that creates the tension names it. Of the 55 rows, 24 are not listed by any
author, or are listed with a wrong exposure (finding 1).

| Accepted ADR | Tension | Listed by the author? | Finding |
|---|---|---|---|
| ADR-0002 rule 3(a) | "snapshot ... into a new process in the same job": the child is in a job of its own | Partly: ADR-0102 lists it against ADR-0003 rule 3 only | |
| ADR-0002 rule 5 | Table entries reached through `/` rather than listed | Yes (ADR-0107) | |
| ADR-0002 INV-0002-3 | `stat` (and `mmap`) missing from the derived invariants' lists | No | 44 |
| ADR-0002 INV-0002-5 | A transient `BOOTSTRAP` handle | Yes (ADR-0107) | |
| ADR-0002 INV-0002-5 | A tty fd is two handles (stream end plus terminal connection) and the process needs a self handle; the enumeration gains both | No | 5, 19 |
| ADR-0003 rule 3 | "copy-on-write" becomes an eager copy | Yes (ADR-0102) | |
| ADR-0003 rule 3 | "in the same job" | Yes (ADR-0102, ADR-0104) | |
| ADR-0003 rule 3 | "registers the child with the broker for a pid" after the call; here the pid comes before it (`spawn_prepare`) | No (minor) | |
| ADR-0003 INV-0003-7 | "never transfers a handle type marked non-duplicable ... such slots are empty": the move list transfers channel ends | No, presented as covered | 8 |
| ADR-0003 rule 1, ADR-0046 Group | `spawn`'s group field unused; the job field is the process job | Yes (ADR-0104) | |
| ADR-0003 rule 2 and follow-up | Interactive `vforkexec` by `start_suspended` plus `resume` with parent-side terminal setup; ADR-0109 substitutes `posix_spawn` with a group attribute and no resume or handoff | No | 23 |
| ADR-0004 rule 2 | Kernel default `Ignore` for 18 to 22 | Yes (ADR-0106) | |
| ADR-0004 personality | `kill(pid)` as `process_signal` → `job_signal`; SIGCHLD on process handles → job handles | Yes (ADR-0104, ADR-0106) | |
| ADR-0004 personality | "`sigprocmask` blocking a signal defers taking it (the kernel queue holds it)": here it is taken into a library set and the kernel disposition is flipped | No | 24 |
| ADR-0004 personality, [P10] | Background `tcsetpgrp` gets `EPERM` instead of SIGTTOU; dash exercises it on every foreground job | Yes, but the exposure is misstated | 1 |
| ADR-0004 personality | "the broker creates every process it spawns directly ... in a sub-job" becomes every POSIX process | Yes (ADR-0104) | |
| ADR-0004 rule 8 | Hangup to the session job reaches the broker and `tmpd`, which live in it | No | 9 |
| ADR-0004 open question | "a request is queued on the new process only if its disposition is `Deliver` at creation": ADR-0102 item 5 answers by copied dispositions, which is the same for `fork` but not stated as the answer | No | 11 (I-26) |
| ADR-0005 In scope, INV-0005-1 | The generator rule gains "minus the `dl*` rows"; the `dlopen` row/deny-list clash | No | 16 |
| ADR-0005 In scope ("the TSV is authoritative") | W10 and the W9 check need `sort`, `wc`, `yes`, `sed`, which are not T1 applets | No | 7 |
| ADR-0005 INV-0005-3 | "compile-time call-log feature" becomes the `stats` feature plus broker aggregation | Yes (ADR-0110) | |
| ADR-0005 INV-0005-4 | "CI runs the TASK-02/02b workload scripts" becomes shell scripts rewritten for Zoraya | Partly (ADR-0110 says they are "the measured ones, as shell commands") | |
| ADR-0006 rule 6 | `stream_read`/`stream_write` never block, no flag | Yes (ADR-0101) | |
| ADR-0006 rule 1 | Terminals as byte streams: output yes, input a connection | Yes (ADR-0101, ADR-0105) | |
| ADR-0006 personality | "a timer object subscribed to the port" for `nanosleep`: the port deadline instead | Yes (ADR-0106) | |
| ADR-0006 personality | `wait4` blocking as `object_wait_one` on the child's process handle: a job handle plus a `reap` round trip | Partly (ADR-0104 item 11) | |
| ADR-0006 rule 1, ADR-0055 deferred | `pselect` over file fds (regular files `NotWaitable`, a file connection's `READABLE` means a reply): no ADR says what `libposix` does | No; `inputs.md` says ADR-0106 answers it | 11 (I-69) |
| ADR-0006 INV-0006-9 | The third behaviour (unlink while open) is INV-0107-5, not INV-0107-4 | No | 31 |
| ADR-0010 personality mapping | `close` → `EIO` through `status`: kept; `O_TMPFILE`/`linkat` → `publish`, `fsync` family not exported: not mentioned by ADR-0103 though `inputs.md` I-75 says so | Partly | 11 (I-75) |
| ADR-0014 rule 6 (as built, ADR-0080) | "A create request carries the mode": the `Open` message has none; `mkfifo`'s `S_IFIFO` and tcc's `0777` cannot reach the service | No | 6 |
| ADR-0014 rule 1, rule 11 | `posix_mode` "stored verbatim" and "`kind` has no FIFO": `S_IFIFO` in the mode now means something to `tmpd` | Yes (ADR-0107) | |
| ADR-0031 | 255 live processes | Yes (ADR-0102) | |
| ADR-0031 follow-up | Copy-on-write for `snapshot_process` deferred | Yes (ADR-0102) | |
| ADR-0033 | Loader reads from an object; "Dynamic linking and `ET_DYN` are outside" stays | Yes (ADR-0108) | |
| ADR-0040 Representation, ADR-0057 | Handle values in a snapshot child's copied memory | No | 3 |
| ADR-0040 Duplication | Type-based duplication by the snapshot vs the `DUPLICATE` right | No | 40 |
| ADR-0040 rights table | A process job handle moved and held thrice without `DUPLICATE`/`TRANSFER` | No | 4 |
| ADR-0040 rights table | `job_suspend`/`job_resume` need a right; `KILL` assumed | Yes (ADR-0105) | |
| ADR-0042 Layout, Versioning | The startup block gains the program header table | Yes (ADR-0108, ADR-0109); the room and "Nothing may follow" not addressed | 46 |
| ADR-0042, `MAX_SPAWN_HANDLES` 64 | A process with more than 64 handles to pass cannot `spawn` or `fork` | No | 45 |
| ADR-0043 open question | An image *holds* a memory object | Yes (ADR-0108) | |
| ADR-0043 convention | Slot name `JOB` reused for a different slot | No | 47 |
| ADR-0046 Job `TERMINATED` | A new process job is `TERMINATED` until something is spawned into it; `snapshot_process` must clear it | No | 39 |
| ADR-0046 Depth | 32 levels against 3 per POSIX process | Yes (ADR-0104) | |
| ADR-0050 No cycles | Moving an endpoint with queued handles; the moved ends are fresh | Yes (ADR-0103) | |
| ADR-0050 Rights | No `DUPLICATE` on endpoints, so a `share` per copy | Yes (ADR-0103 driver 4) | |
| ADR-0051 deferred | A mapping "must stay shared with its other holders": copied instead | Yes (ADR-0102) | |
| ADR-0051 Lifetime, INV-0051-5 | An image as a new holder; `Size` for `image_create` | Yes (ADR-0108) | |
| ADR-0053 | One more service, a second session shape chosen by a file | Yes (ADR-0104, ADR-0110) | |
| ADR-0055 Replies ("does not read a connection whose reply is waiting") | A held FIFO `open` (ADR-0107 item 14) is a request the loop keeps unanswered while serving others; `ttyd` does the same for `read`; the loop's rule for an unanswered request and for `PEER_CLOSED` under it is not stated | No | walkthrough 4 |
| ADR-0055 `MAX_OPEN` | Counts connections, attached buffers and tokens; ADR-0103 item 13 counts connections only | No (minor) | |
| ADR-0055 protocol | New requests `share`, `seek`, `set_flags`, `current` | Yes (ADR-0103) | |
| ADR-0060 | The console's `READ` goes to `ttyd` alone | Yes (ADR-0105) | |
| ADR-0080 Cookies | One cookie per connection; ADR-0103 item 3 assumes one per fd | No | 20 |
| ADR-0004 rule 3 | `job_suspend`/`job_resume` semantics fixed by ADR-0105 item 8 | Yes (ADR-0105) | |

## What was checked and found sound

- **Citations.** Every number sampled against the research files matched: the 248-row surface and its categories,
  185/63 dynamic/static (ADR-0100, recomputed from `libc-surface.csv`); 243 after the strikes (the `dl*` rows are
  `T1`, `setuid`/`setgid` `T0`); `F_SETFD` 5,360, `F_DUPFD_CLOEXEC` 3,894, `F_GETFL`/`F_SETFL` 39/25, `TIOCGWINSZ`
  552, `TCGETS` 12, `TIOCSPGRP` 15, `TIOCGPGRP` 1, `execve` 3,002/2,922/7, `wait4` 4,030/3,307, `lseek` 1,214/7,381,
  `O_APPEND` 393, `O_EXCL` 3, `/bin/sh` 7+136 = 143 by make and 92 by shebang (ADR-0102's "136" is the W4 count),
  1,169/1,167/764/403 forks, 3,751 SIGCHLD, `kill` 7/17, `setpgid` 4, `getpgid` 1, 55 to 60 of 72 to 100 `pselect6`,
  9 lost races, 220 runs with 130/143, W8 13 s and 3.5 s, four partial targets deleted (three in one run), none left,
  no process after 0.5 s, `mkstemp` 2 in T2, `memfd_create` 10, `/tmp` 9, the W5 step list, Xok's 6 ms against under
  1 ms, Redox's 14x and incomplete self-hosting, MINIX's unrecoverable process manager, QNX's resource managers,
  Plan 9's 90 to 95 %, Starnix's runner-held pid table and its RFC's performance warning, ExOS's unprotected tables,
  GR-005's vfork-style `posix_spawn`, DM-048, DM-057, DM-059, DM-060, DM-130, DM-100, GR-010, CP-15, CP-25 (apart
  from finding 38). The `design.md` figures (9 ms `memory_create`, 4061/502 MB/s, 0.105/0.211 s, 875 ns/13 µs,
  two bits per frame, `[fault: data abort, class 0x24]`) are as cited; ADR-0102 labels its copy-rate derivation an
  estimate, as it should.
- **Sources fetched** (16 of 44): [M3] 78 lines, `x8`, `x0` to `x5`, `svc 0`; [M5] `PIPE_BUF 4096`, `NAME_MAX 255`,
  `PATH_MAX 4096`, `ARG_MAX 131072`; [M6] "Since sizeof errno < PIPE_BUF, the write is atomic", `CLONE_VM|CLONE_VFORK|SIGCHLD`,
  caught → `SIG_DFL`, exit 127; [M8] `envp = argv+argc+1`, `AT_HWCAP`, `AT_PAGESZ`, `AT_EXECFN`, `AT_RANDOM`, the
  `AT_UID`/`AT_EUID`/`AT_SECURE` early return, `__init_tls(aux)`; [P1] 65,536 bytes since 2.6.11, `PIPE_BUF` 4096,
  all-or-`EAGAIN` for at most `PIPE_BUF`, 0 on all write ends closed, SIGPIPE/EPIPE; [P5] the four errors and SIGTTIN;
  [P7] 255 characters since 5.1, four levels, caught reset, pid unchanged, close-on-exec; [P10] `EPERM` and the
  background-caller SIGTTOU rule; [T2] "-static ... (default is a shared linked executable)", own linker, `-ar`,
  `-B`, `-nostdlib`; [T3] `--enable-cross`, `--targetos`, `--crtprefix`, `--sysincludepaths`, `--libpaths`,
  `--elfinterp`, and `--config-musl` (finding 35); [T5] `CONFIG_TCC_CRTPREFIX`, `SYSINCLUDEPATHS`, `LIBPATHS`,
  ARM64 `ELFINTERP`, `TCC_LIBTCC1`, `__aarch64__` selection, the `CONFIG_TCC_STATIC` dummy `dl*` declarations (which
  supports ADR-0100 item 8's outside-evidence note); [T6] (finding 36); [K1] `mkfifo` then `O_RDONLY|O_NONBLOCK` then
  `O_WRONLY`, `pselect` then non-blocking `read` retried on `EAGAIN`, `fifo:` and `R,W`, `-2,-2`, `FD_CLOEXEC`
  toggling for pipe style only; [D1] `setjobctl`, `forkchild`, `forkparent`, `waitproc`, `waitforjob`, `getstatus` as
  described (and finding 1); [C1] the non-copy-on-write fork, `CreateProcess`, `.data`/`.bss`, stack and heap, same
  layout, retries, "inevitable", "inefficient under Win32", 20 to 30 %. Also fetched beyond `sources.md`:
  `arm64-link.c` (`ELF_START_ADDR 0x00400000`, `ELF_PAGE_SIZE 0x10000`), which confirms ADR-0108 item 5's
  outside-evidence claim.
- **Repository facts** stated by the ADRs: `rust-lld`, `rust-objcopy` and `gcc-ld/` in the pinned toolchain;
  BSD-2-Clause `LICENSE` and the patches' README; the CI `timeout-minutes: 30`; the runner's `<`, `>`, `!`, `~ MS
  TEXT`, `o` steps; `test-boot`/`test-files` with `--keys`/`--disk`; `MAX_HANDLES` 1024, `MAX_SPAWN_SIZE` 64 KiB,
  `MAX_INLINE` 60 KiB, `MAX_OPEN` 896, `OBJECT_TYPES`, the rights constants; `zoraya-elf` skipping non-`PT_LOAD`
  headers and `MAX_SEGMENTS` 8; job `TERMINATED` set at live 0.
- **Options.** Each rejected option is given its strongest case I could find, with these exceptions: finding 28
  (missing precedents); ADR-0100 Option A is fair to Starnix; ADR-0102 Option A honestly concedes the time saved;
  ADR-0103 Option B states `fuchsia.io`'s choice correctly against `stage-4/sources.md#services`; ADR-0104 Option D
  and ADR-0105 Option D are not strawmen; ADR-0107 Option E is fairly treated; ADR-0109 Option C's compiler risk is
  real. The Linux baseline is present in every ADR but ADR-0110 (finding 28).
- **Arithmetic.** 2^31 pids at 1,000/s ≈ 24.9 days; 1 MiB at 4 GB/s ≈ 0.25 ms and at 500 MB/s ≈ 2 ms, times 764;
  28 nested sessions × 3 levels plus the root path under 32 (approximately); `OPEN_MAX` 512 = half of 1024; the
  connection count of about 60 for 11 processes with 4 each plus files; 128 MiB in 9 ms ≈ 14 GB/s.
- **Template.** All eleven have the template's headings in order plus the task's two sub-headings; `adr-check` is
  clean apart from the expected index rows; every `Evidence` line matches its body (INV-0001-3 passes).
- **The `exec`-by-job-`TERMINATED` trick** (walkthrough 2) is sound in the kernel as built.
- **The byte-stream design** (ADR-0101) is sound for pipes and the bad-stdin case; its atomicity rule reproduces
  [P1] for at most 4096 bytes and the blocking semantics by retry; its rights and teardown fit ADR-0041 and
  ADR-0050's cycle rule.
- **`plan.md`** orders the work consistently with the ADRs' dependencies; 6a lists the kernel objects all five
  walkthroughs need except the self handle (finding 5) and the create mode (finding 6).

## What was checked and not checked

- **Checked:** every claim quoted above; the sections of `task-02/posix-surface.md` (M2, absolute paths, key
  observations, jumps), `conflict-impact.md` (X-1, X-2, X-3), `callsite-analysis.md` (features 1 to 4, M4),
  `method.md` (pinned inputs, tiers, known limits), `task-02b/parallel-surface.md` (setup, new syscalls, interrupted
  calls, W9), `event-timelines.md` (W9a, W10), `task-02b/callsite-analysis.md` §4, `fs/requirements.md` §1.2, §1.3,
  §2.2, §2.4, §2.6, §3.2, §4 and its sources table; the DM, GR, CP and X entries named in "found sound" plus DM-001,
  DM-002, DM-003, DM-005, DM-007, DM-009, DM-017, DM-020, DM-022, DM-023, DM-030, DM-032, DM-036, DM-041, DM-069,
  DM-070, DM-071, DM-072, DM-077, DM-085, DM-160 (headings and classifications; bodies read for DM-001, DM-048,
  DM-057, DM-059, DM-077), GR-001, GR-002, GR-004, GR-006, CP-01 to CP-28 (headings), X-5, X-6, X-8, X-10, X-15.
- **Not fetched:** [M1], [M2], [M4], [M7], [M9], [R1] to [R4], [P2] to [P4], [P6], [P8], [P9], [P11] to [P17], [K2],
  [K3], [T1], [T4], [T7], [D2], [B1] to [B4], [S1]. The `tcc.h` and `tccdefs.h` pages came back without any `musl`
  text, which may be truncation; finding 35 is worded accordingly.
- **Not verified from a source:** musl's `__init_tls.c` behaviour (findings 13 and 14 rest on the reviewer's memory
  and are marked); the exact `eval.c` location of dash's `pending_sig` check (finding 27 says only that [D1] lacks
  it); whether Apple's clang builds musl's files (an open question in the ADR, left as such); the W4 configure
  figures beyond the fork counts; the W8 `src/` makefiles beyond `leaf.mk` and the top-level files.
- **Not walked:** W9b (SIGTERM) end to end, Ctrl-Z/`fg`/`bg`, the pipe-style jobserver under `fork`, `exec` of a
  `#!` script, `memfd_create`, and the T2 rows. The kernel and crates were read only where an ADR made a claim about
  them (`job.rs`, `zoraya-elf`, `zoraya-abi`'s `spawn.rs` and constants, `zoraya-proto-file`'s messages).
- **Sampling:** at least five citations per ADR were opened (ADR-0100: 7; ADR-0101: 6; ADR-0102: 7; ADR-0103: 7;
  ADR-0104: 8; ADR-0105: 7; ADR-0106: 8; ADR-0107: 8; ADR-0108: 6; ADR-0109: 7; ADR-0110: 6). Mismatches found are
  findings 17, 27, 30, 33, 34, 35, 36, 37, 38, 49; every other opened citation supports its sentence.

## Disposition

For the author. Findings 1 to 12 are errors; 1, 2, 3, 4, 5, 6, 9 and 12 each stop one of the five walkthroughs or
the roadmap criterion as written, and should be fixed before the architect reads the batch.

### Disposition of the review (author, 2026-11-02)

Every finding is fixed in the ADRs, `inputs.md`, `sources.md` or `plan.md`; the Disposition column says where. Two
sources were fetched for the fixes and added to `sources.md`: musl's `src/env/__init_tls.c` ([M10], for findings 13
and 14) and dash's `src/eval.c` ([D3], for finding 27). Of the tensions table's 24 rows not listed by an author, each
now has a bullet in the ADR that creates it: INV-0002-5 (ADR-0107), ADR-0003 rule 3's registration order and
INV-0003-7 (ADR-0102), ADR-0003 rule 2 (ADR-0104), ADR-0004's blocking mapping (ADR-0106), ADR-0004 rule 8
(ADR-0104), the ADR-0004 open question (`inputs.md` I-26), INV-0005-1 and the `dlopen` clash (ADR-0100), the applet
list (ADR-0109), INV-0005-4's scripts (ADR-0110), `pselect` over files (ADR-0103, ADR-0106), INV-0006-9's third
behaviour (ADR-0107), ADR-0010's mapping (ADR-0103 item 14), ADR-0014 rule 6 as built (ADR-0103), ADR-0040 and
ADR-0057 generations, duplication by right and the process job's rights (ADR-0102, ADR-0104), the 64-handle bound
(ADR-0102, ADR-0103), ADR-0043's `JOB` name (ADR-0107), ADR-0046's `TERMINATED` (ADR-0102), ADR-0055's held open
(ADR-0107), `MAX_OPEN`'s count (ADR-0103), ADR-0080's cookie (ADR-0103), and ADR-0006's `wait4` (ADR-0104). The
walkthroughs' verdicts after the fixes are for the check that follows this section.

## Check of the fixes (second reviewer)

Checker: a second subagent that wrote neither the ADRs nor the first review. Compared 78072b8 (reviewed) with 10a6458
("independent review and its fixes"), on 2026-11-02, over `git diff 78072b8..10a6458 -- docs/adr
docs/research/stage-6/{inputs,sources,plan}.md` (14 files, 413 insertions, 202 deletions). Scope: the changed lines
and the unchanged lines they depend on. Fetched for this check: [M10] (`src/env/__init_tls.c`, 153 lines), [D3]
(`src/eval.c`, 1,169 lines), and, for two changed claims, `man7/fifo.7` and `man2/kill.2` from the man-pages mirror.
`cargo run -q -p zoraya-adr-check -- docs` at 10a6458: "59 ADRs checked", the eleven `INV-0001-5 ... is not
listed` errors, 17 `not yet applicable` warnings, nothing else.

**Totals:** 46 confirmed fixed, 4 partly fixed (findings 1, 10, 15, 22), 0 not fixed. New problems: 13 (2 errors,
7 imprecisions, 4 notes), listed after the tables.

| # | Verdict | Evidence |
|---|---|---|
| 1 | partly fixed | ADR-0104 item 10 ("any process of the session may call it ... [D1]"), ADR-0105 item 7 and its tensions bullet, INV-0105-2's two dash calls: as the disposition says. Residue: ADR-0105 item 8 still reads "`SIGTTOU` is sent nowhere (`TOSTOP` off; `tcsetpgrp` from the background is `EPERM`)", contradicting item 7 (N4). |
| 2 | confirmed fixed | ADR-0104 item 8a `set_stop_disposition(sig, Default \| Ignore \| Handler)` for 18, 20, 21, 22; item 9 acts on it ("`SIGCONT` is `job_resume` always, plus `job_signal(Custom(18), 18)` when the target has a handler"); ADR-0105 items 3 and 8, ADR-0106 items 1 and 9 agree. The record's life across `exec` and `fork` is not stated (N2) and the terminal's path is described differently in ADR-0104 and ADR-0105 (N3). |
| 3 | confirmed fixed | ADR-0102 item 3 "with the same rights and the same generation", item 4 "at the generation the *caller's* slot `slot` carries"; INV-0102-7 added; INV-0102-2's check gains "the generation of each moved slot"; tension bullet "*ADR-0040 and ADR-0057*" present. See N11 for the `self_slot` generation. |
| 4 | confirmed fixed | ADR-0104 item 2: full-rights handle `DUPLICATE \| TRANSFER \| WAIT \| SIGNAL \| CREATE_PROCESS \| KILL` (exactly ADR-0040's job row); item 4 returns `parent_job` (`WAIT \| DUPLICATE \| TRANSFER`) and `child_job` (`CREATE_PROCESS \| TRANSFER`); ADR-0107 item 5 names `PROCESS_JOB` with those rights. |
| 5 | confirmed fixed | `SELF` slot in ADR-0107 item 5 (second bootstrap message with a `WAIT` duplicate), ADR-0102 item 3 (`self_slot` gets "a handle to the *child* process itself with `WAIT`"), ADR-0106 item 3, INV-0102-7, ADR-0107's INV-0002-5 bullet. `spawn` returns the child handle with `ProcessObject::RIGHTS` = `DUPLICATE \| TRANSFER \| WAIT \| SIGNAL \| RESUME` (`crates/zoraya-abi/src/lib.rs`, `kernel/src/syscall.rs` line 269), so the spawner can make the `WAIT` duplicate. Wording residue in N10. |
| 6 | confirmed fixed | ADR-0103 item 1 `open_with_mode` ("a new ordinal under ADR-0052; the stage 4 `Open` message has `flags`, `rights` and `path` only"); ADR-0107 item 13 uses it; ADR-0108 Context says so; ADR-0103 tensions name ADR-0014 rule 6 and ADR-0080; plan.md 6c lists it. |
| 7 | confirmed fixed | ADR-0109 item 6 adds `sort`, `wc`, `yes`; checked: `applets-by-tier.tsv` T1 toybox row is `cat head ls mv rm sleep touch true uname`, `sort` and `wc` are in the T2 row, `yes` in none; `w10.sh` runs `sort`, `wc -l`, `yes`, `head`, `cat`. ADR-0110 item 4's W9 check uses only dash (`for f in t*.h; do [ -e "$f" ] && n=$((n+1)); done`); I-39 updated; ADR-0005 tension bullet present. The rewritten check has a different defect (N1). |
| 8 | confirmed fixed | ADR-0102 tensions "*ADR-0003 INV-0003-7* ... Accepting this ADR replaces that invariant by INV-0102-6 below"; INV-0102-6 states the three clauses the finding asked for. |
| 9 | confirmed fixed | ADR-0104 item 1: two child jobs, `tmpd`'s and the session job "which holds only POSIX processes"; item 9: `-1` is "`job_signal` on every process job of the session but the caller's"; ADR-0105 item 3 and ADR-0107 item 12 refer to item 1. The attribution of the exclusion to POSIX is wrong (N8). |
| 10 | partly fixed | The false 90 is gone, but the new parenthesis does not sum to its headline "the 126 rows": 51+40+12+7+6+6 = 122, plus `time` 11 = 133 (or plus the 5 that touch the clock = 127), plus the five stdio rows = 138 (or 132). Recounted from `libc-surface.csv` (T0+T1 rows: process 51, file 40, signal 12, dir 7, tty 6, memory 6, time 11, stdio 43): the categories are right, 126 is not. `readv` and `writev` are `file` rows (`unused-applets`, `T3`), not stdio. "About 110" is consistent with 132 minus the 22 pure rows named (N7). |
| 11 | confirmed fixed | I-26 matches ADR-0102 item 5 ("the child receives requests from the moment the kernel adds it to its job, under the dispositions it copied"); I-32 matches ADR-0105 items 4 and 8; I-33 matches ADR-0104 items 4, 5, 8; I-51 "no `Edge`" matches ADR-0106 item 3; I-52 and I-77 match ADR-0106 item 10 and ADR-0104 item 11; I-69 and I-75 point at the new ADR-0103 item 14 and ADR-0106 item 3 sentences, which exist; I-39 as finding 7. |
| 12 | confirmed fixed | ADR-0101 Peers: "`PEER_CLOSED` on the write end when the last read-end handle closes"; Signals: "a writer blocked on `WRITABLE \| PEER_CLOSED` wakes when its reader goes"; INV-0101-2 asserts "both ends' `PEER_CLOSED` (through `port_wait`)" and the blocked-writer wake. |
| 13 | confirmed fixed | ADR-0109 item 4 is the one list (`AT_PAGESZ`, `AT_PHDR`, `AT_PHENT` 56, `AT_PHNUM`, `AT_RANDOM`, `AT_NULL`); ADR-0100 item 4 refers to it. [M10] fetched: `for (p=(void *)aux[AT_PHDR],n=aux[AT_PHNUM]; n; n--,p+=aux[AT_PHENT])` and `if (phdr->p_type == PT_TLS) tls_phdr = phdr;`, as sources.md says. See N9 on which headers the block must carry. |
| 14 | confirmed fixed | ADR-0100 item 2 retains `src/env/__init_tls.c` and `src/thread/aarch64/__set_thread_area.s`, adds `set_tid_address` and `mmap` to the shim; item 5 describes `__init_tls`. [M10] fetched: `__init_tp` calls `__set_thread_area` and `__syscall(SYS_set_tid_address, ...)`; `static_init_tls` uses `builtin_tls` or `__syscall(SYS_mmap2, ...)` with `#define SYS_mmap2 SYS_mmap` when `mmap2` is absent (it is on AArch64). |
| 15 | partly fixed | New text right: "an `ar` reader and an `ET_REL` symbol-table reader, which `zoraya-elf` (program headers of `ET_EXEC` only) does not have"; INV-0100-2 scans for `0xd4000001`; plan.md 6b names the tool. The `_`-prefixed tier rows are `_exit` (T0), `__assert_fail` (T1), `__errno_location` (T0), checked against the CSV (`_Exit` is T3, rightly absent). Residue: item 7's first sentence still says the tool "reads `libc.a`'s members with `zoraya-elf`" (N6). |
| 16 | confirmed fixed | ADR-0100 gained "**Tensions with accepted ADRs.**" with the INV-0005-1 rule change and "ADR-0005 lists `dlopen` both as a `T1` row and on INV-0005-2's deny list". |
| 17 | confirmed fixed | Option B Evidence for and Precedent both carry `[outside evidence: fuchsia.dev libc page, not fetched]`; the postmortem is cited only for "shipping on Nest Hubs". |
| 18 | confirmed fixed | ADR-0102 wrapper: `spawn_prepare`, `spawn_commit`; "about 4 for a quiet shell (`/`, `/tmp`, cwd, the terminal connection ...) plus one per open file"; "terminal connection" throughout; ADR-0103 item 13 agrees. |
| 19 | confirmed fixed | ADR-0103 item 7: "a terminal connection is `clone`d by the terminal protocol with the child's group as `spawn_prepare` returned it (ADR-0105 item 4)"; item 8 and ADR-0102's wrapper say the same; `spawn_prepare` now returns `pgid`. |
| 20 | confirmed fixed | ADR-0103 item 3: one cookie "per description entry in the library, so `dup`ed directory fds share one stream"; the restart after `fork`/`spawn` is a stated deviation; ADR-0080 bullet in tensions. |
| 21 | confirmed fixed | ADR-0103 item 4: "`O_NONBLOCK` is therefore per process after `fork` or `spawn`, where POSIX shares it ... ([P16]): a stated deviation"; tensions bullet "*[P16]*". sources.md [P16] carries the shared-flags sentence. |
| 22 | partly fixed | ADR-0104 item 4 names `zoraya_fork_group(Inherit \| New \| Join(pid))` and the `POSIX_SPAWN_SETPGROUP` path; item 8 now succeeds only as a no-op ("anything else would move a process between jobs and is `EPERM`"). Residue: INV-0104-4 (unchanged) still says "or declares its own lone job a group", a case item 8 no longer has (N5). |
| 23 | confirmed fixed | ADR-0104 item 5: "spawns with `START_SUSPENDED`, calls `tcsetpgrp` (item 10) for the child's group, commits, and then `process_resume`s the child", through `POSIX_SPAWN_TCSETPGRP`; `start_suspended` gone from item 4's signature; ADR-0109 item 9 and ADR-0104's "*ADR-0003 rule 2*" bullet agree; `START_SUSPENDED` is ADR-0042's flag. |
| 24 | confirmed fixed | ADR-0106 item 2: "The kernel's disposition does not change on block ... a stated deviation", with the 80-calls-per-wait cost and the "No measured program blocks a default-action signal and receives it while blocked" argument; tensions bullet "*ADR-0004*'s mapping has a blocked signal held in the kernel queue". |
| 25 | confirmed fixed | ADR-0104 item 6: block and `ignored` set "built from its *POSIX* dispositions"; stop and continue signals "reported by their POSIX disposition". (The broker's own record of them across `exec` is N2.) |
| 26 | confirmed fixed | ADR-0104 item 10: "a duplicate of that group job's handle with `SIGNAL \| KILL` (`KILL` for `job_suspend`, ADR-0105 item 5)"; ADR-0105 item 5: "with the rights ADR-0104 item 10 gives it (`SIGNAL \| KILL`)". |
| 27 | confirmed fixed | ADR-0106 Option B cites "`dotrap()` in `evaltree`, [D3]". [D3] fetched: `evaltree` calls `dotrap()` at its start (line 230) and under `out:` (line 328), as sources.md says. |
| 28 | confirmed fixed | ADR-0106 Option A has NT's fresh-thread form, Option C seL4 notifications (both DM-017); ADR-0110 Option A is "the baseline" with the Alpine runs as precedent, Option B cites Hubris (`alternatives-postmortem.md#hubris`, heading exists at line 794), Option C Fuchsia's test manager marked outside evidence. |
| 29 | confirmed fixed | ADR-0106 item 1: "1 to 16 except 9, and 24 to 27, 29 to 31". |
| 30 | confirmed fixed | ADR-0107 Context cites `task-02/conflict-impact.md#x-1-capability-only-access-...` for the 32/1 counts. |
| 31 | confirmed fixed | INV-0107-4: "with INV-0107-5 this is INV-0006-9's check (its third behaviour is the unlink while open)". |
| 32 | confirmed fixed | ADR-0108 item 1 now reads "a segment that is not readable; a segment both writable and executable; segments outside the user range or overlapping; `memsz < filesz`", which is ADR-0033's list. |
| 33 | confirmed fixed | `fs/requirements.md#22-atomic-rename` added to Evidence and Context (heading "### 2.2 Atomic rename" at line 179, the tcc sentence at line 195); the Plan 9 sentence is "the author's inference from the two". |
| 34 | confirmed fixed | ADR-0109 Context: "GNU hello's `configure`, run with `CC=tcc` ... tcc's own `configure` has no `config.guess` ([T3])". |
| 35 | confirmed fixed | ADR-0109 item 8: "it sets the dynamic linker path and `CONFIG_TCC_MUSL` ([T3]), whose other effects in tcc's sources the first toolchain task checks". |
| 36 | confirmed fixed | ADR-0109 item 6: "`lib/Makefile` chooses `dsohandle.o` and the native-only objects by OS branch, one more patch point for a `Zoraya` target". |
| 37 | confirmed fixed | ADR-0101 Forces: "`SIGPIPE` by default is a tradeoff that shell pipelines depend on (DM-077), kept out of the native ABI by ADR-0004 rule 7". |
| 38 | confirmed fixed | ADR-0105 driver 2: "(CP-25 keeps data and control apart; the data side is bytes)", no quotation marks. |
| 39 | confirmed fixed | ADR-0102 item 1: "Creating the child clears the job's `TERMINATED` signal, as `spawn` does (ADR-0046)"; tensions bullet "*ADR-0046*'s job `TERMINATED` is cleared by the call". |
| 40 | confirmed fixed | ADR-0102 item 3: "whose handle carries `DUPLICATE` ... A handle of a duplicable type that lacks `DUPLICATE` leaves its slot empty"; tensions bullet "*ADR-0040*'s duplication is by right"; INV-0102-6 tests it. |
| 41 | confirmed fixed | Wrapper paragraph: "take its pid, parent pid and group from the `spawn_prepare` reply that is in its copied memory ... close the duplicated handles to the parent's other children's process jobs and drop that child list". Wording note in N11 (the reply has no parent pid field). |
| 42 | confirmed fixed | ADR-0104 item 12 and ADR-0110 item 1: `PATH=/bin:/usr/bin`, `HOME=/home`, `TERM=dumb`, `SHELL=/bin/sh`; a `/home` directory added to the image; `task-02/method.md` line 68 confirms `env -i PATH=<set> HOME=<run dir> TERM=dumb SHELL=<set>/sh`. |
| 43 | confirmed fixed | INV-0110-1 "`make test` fails unless ..."; INV-0110-3 a property of the report; INV-0105-4 lists `ICANON`, `ECHO`, `ISIG`, `VINTR`, `VQUIT`, `VSUSP`, `VERASE`, `VKILL`, `VEOF`, `ONLCR`, `TOSTOP`, `termios`, `winsize`; INV-0103-2 reads the count "through the `fs-service` crate's host test interface", which exists (`open_count` at `crates/zoraya-fs-service/src/lib.rs` line 280). |
| 44 | confirmed fixed | INV-0103-3 and INV-0104-5 now list `open`, `read`, `write`, `close`, `stat`, `lseek`, (`dup`,) `mmap`, `pipe`: INV-0002-3's eight. |
| 45 | confirmed fixed | ADR-0102 item 4 ("64, ADR-0042 ... `EAGAIN` from `fork`") and ADR-0103 item 7 ("`E2BIG` from `posix_spawn` and `exec`") state the bound; both ADRs list it against ADR-0042. Two errnos for one bound, one per call family (N13, note). |
| 46 | confirmed fixed | ADR-0109 item 5: "ADR-0042's 'Nothing may follow' becomes 'then the program headers, and nothing may follow'"; room `STARTUP_BLOCK_MAX` (`MAX_SPAWN_SIZE + PAGE` in `crates/zoraya-abi/src/spawn.rs`), overflow `LimitReached`. The "8 headers" aside is N9. |
| 47 | confirmed fixed | ADR-0107 item 5: `PROCESS_JOB` "a different slot from ADR-0043's `JOB` of the first process". |
| 48 | confirmed fixed | ADR-0107 item 14: "`O_RDWR` is `EINVAL` (Linux allows it and POSIX leaves it undefined, [P2]; a stated deviation ...)". `fifo.7` fetched: "Under Linux, opening a FIFO for read and write will succeed ... POSIX leaves this behavior undefined." (N12: sources.md's [P2] summary does not carry the sentence.) |
| 49 | confirmed fixed | ADR-0104 Context: "`kill(getpid(), SIGINT)` as each make's re-raise (`task-02b/event-timelines.md#w9a-...`)"; the timeline's line 54 has it. |
| 50 | confirmed fixed | ADR-0102 Depends on: "ADR-0056 (proposed), ADR-0057". |

### The tension rows the first review found unlisted

The first review counted 24 rows "not listed by any author, or listed with a wrong exposure"; the four "Partly" rows
are added at the end, so that every row that was not a plain "Yes" is checked.

| Tension | Bullet present? | Where |
|---|---|---|
| ADR-0002 INV-0002-3: `stat` (and `mmap`) missing from derived invariants | n/a (the lists were corrected) | INV-0103-3, INV-0104-5 now copy INV-0002-3's list |
| ADR-0002 INV-0002-5: tty fd is two handles; a self handle | yes | ADR-0107 tensions, "*ADR-0002 INV-0002-5*: its list gains `SELF` ... a tty fd is two handles ... Accepting this ADR replaces the invariant's list by it" |
| ADR-0003 rule 3: pid registered before the call | yes | ADR-0102 tensions, "Rule 3 has the wrapper register the child for a pid after the call; here the pid comes before it" |
| ADR-0003 INV-0003-7: the move list transfers channel ends | yes | ADR-0102 tensions, "*ADR-0003 INV-0003-7* ... replaces that invariant by INV-0102-6" |
| ADR-0003 rule 2: interactive `vforkexec` path | yes | ADR-0104 tensions, "*ADR-0003 rule 2*: ... keeps rule 2's shape (spawn suspended, set the group and the terminal from the parent, resume)" |
| ADR-0004 personality: blocked signal held by the kernel queue | yes | ADR-0106 tensions, first bullet |
| ADR-0004 / [P10]: background `tcsetpgrp` (exposure misstated) | yes, restated | ADR-0105 tensions ("dash exercises the background call on every foreground job"); ADR-0104 tensions "*[P10]*" |
| ADR-0004 rule 8: hangup reaches broker and `tmpd` | yes | ADR-0104 tensions, "*ADR-0004 rule 8*'s 'hangup to the session job' reaches only POSIX processes" |
| ADR-0004 open question: request queued at creation | no bullet in an ADR | `inputs.md` I-26 only; ADR-0102 item 5 says "(ADR-0004's open question)" inline but its tensions list has no bullet. Minor: the item answers it. |
| ADR-0005 INV-0005-1: the `dl*` rows; the `dlopen` clash | yes | ADR-0100 tensions, "*ADR-0005 INV-0005-1*" |
| ADR-0005 "the TSV is authoritative": `sort`, `wc`, `yes` | yes | ADR-0109 tensions, "*ADR-0005*'s 'the TSV is authoritative' for applets" |
| ADR-0006 rule 1 / ADR-0055 deferred: `pselect` over file fds | stated as behaviour, no bullet | ADR-0103 item 14 and ADR-0106 item 3 (sentence present; a tensions bullet is not needed since the ADRs now say what `libposix` does) |
| ADR-0006 INV-0006-9: third behaviour | yes (in the invariant) | INV-0107-4's check sentence |
| ADR-0014 rule 6 as built (ADR-0080): no mode in `Open` | yes | ADR-0103 tensions, "*ADR-0014 rule 6 and ADR-0080*: the mode of a create reaches the service only now" |
| ADR-0040 Representation, ADR-0057: generations in copied memory | yes | ADR-0102 tensions, "*ADR-0040 and ADR-0057*" |
| ADR-0040 Duplication: by type vs by right | yes | ADR-0102 tensions, "*ADR-0040*'s duplication is by right" |
| ADR-0040 rights table: process job handle thrice without rights | n/a (removed by the fix) | ADR-0104 items 2 and 4 give the rights; the handle is no longer "moved thrice" |
| ADR-0042 Layout: room and "Nothing may follow" | yes (in the item) | ADR-0109 item 5; ADR-0109 tensions "*ADR-0042*: the startup block's layout grows (item 5)" |
| ADR-0042 `MAX_SPAWN_HANDLES` 64 | yes | ADR-0102 tensions "*ADR-0042*'s `MAX_SPAWN_HANDLES` of 64 bounds a `fork`'s connections"; ADR-0103 tensions "*ADR-0042*'s `MAX_SPAWN_HANDLES` bounds the fds a `spawn` can pass" |
| ADR-0043 convention: slot name `JOB` | yes (in the item, not the tensions list) | ADR-0107 item 5, "a different slot from ADR-0043's `JOB` of the first process" |
| ADR-0046 Job `TERMINATED` | yes | ADR-0102 tensions, "*ADR-0046*'s job `TERMINATED` is cleared by the call as by `spawn` (item 1)" |
| ADR-0055 Replies: a held FIFO `open` | yes | ADR-0107 tensions, "A held open is a request the loop leaves unanswered ... `PEER_CLOSED` on a connection with a held open drops the request" |
| ADR-0055 `MAX_OPEN` counts buffers and tokens too | yes (in the item) | ADR-0103 item 13, "which also counts attached buffers ... and tokens" |
| ADR-0080 Cookies: one per connection | yes | ADR-0103 tensions, "*ADR-0080*'s one cookie per connection makes a directory stream restart" |
| (Partly) ADR-0002 rule 3(a) "in the same job" | yes | ADR-0102 tensions, "(ADR-0002 rule 3(a) says 'in the same job' too)" |
| (Partly) ADR-0005 INV-0005-4: rewritten scripts | yes | ADR-0110 tensions, "*INV-0005-4* says 'CI runs the TASK-02/02b workload scripts' ... rewritten as dash scripts and runner scripts" |
| (Partly) ADR-0006 personality: `wait4` as `object_wait_one` | yes | ADR-0104 tensions, "*ADR-0006*'s personality text has a blocking `wait4` as `object_wait_one`" |
| (Partly) ADR-0010 personality mapping: `fsync`, `O_TMPFILE`, `linkat` | yes (in the item) | ADR-0103 item 14; ADR-0100 item 6 maps `linkat` with `AT_EMPTY_PATH` to `publish`, which is ADR-0010's personality mapping line ("`linkat(fd, "", dir, name, AT_EMPTY_PATH)` on such a file → `publish`") |

### New problems in the changed lines

- **N1 (error) ADR-0110 item 4, the W9 script.** The rewritten line types `cd /work/w8 && make -j4 >/dev/null;
  echo W9-STATUS $?; n=0; for f in t*.h; ...` as one command at an interactive prompt and then sends `\x03`. When
  the foreground make dies of `SIGINT`, interactive dash's `waitforjob` does `raise(SIGINT)` ([D1] in
  `sources.md`: "`raise(SIGINT)` when the job died of `SIGINT`"), its handler calls `onint`, and the rest of the line
  is abandoned: `echo W9-STATUS $?` never runs and the runner waits for `W9-STATUS 130` forever. The measurement's
  own harness says so and asks separately (`tools/posix-surface/remote/w9_session.py` lines 93 to 97: "after Ctrl-C
  an interactive shell abandons the rest of the line, so the status is asked for separately", then
  `echo MAKE_STATUS=$?` on its own line). Under ADR-0106 item 8 the `raise` runs the handler inline, so Zoraya's
  dash behaves the same. The defect existed in shape at 78072b8 (the first review did not catch it); the fix
  rewrote the line and kept it. Fix: make `W9-STATUS` and `W9-PARTIAL` separate steps after the prompt, as item 3
  does for W5's `echo status=$?`.
- **N2 (error) ADR-0104 items 6 and 8a.** The broker's recorded stop and continue dispositions (item 8a) are not
  reset by `exec_commit` and not copied by `spawn_prepare`. After `exec` of a program whose predecessor had a
  `SIGTSTP` handler, the record still says `Handler`, so `kill(pid, SIGTSTP)` sends `Custom(20)` and no stop (item
  9), and the new image's kernel disposition for 20 is `Ignore` (ADR-0106 item 1): the signal is dropped. POSIX
  resets a caught signal to `SIG_DFL` on `exec` ([P7]) and keeps the parent's dispositions on `fork`. Item 6 should
  reset the four to `Default` (or `Ignore` where ignored), and item 4 should say what a new pid's record starts as
  (the caller's for `fork`; `Default`, or `Ignore` per the attribute block, for `posix_spawn`).
- **N3 (imprecision) ADR-0104 item 9 vs ADR-0105 item 3.** ADR-0104: "A stop from the terminal (ADR-0105 item 3)
  is the `Default` path on the foreground group." ADR-0105: the broker "applies the foreground group's recorded
  dispositions for `SIGTSTP` (... `job_suspend` ..., or `Custom(20)` to a handler, or nothing when ignored)". One
  says always suspend, the other says by disposition. Neither says how a group whose processes differ (the
  dispositions are per pid, item 2) is handled for `-pgid` or for the terminal's `suspend(20)`: one `job_suspend` of
  the group job, or per process job.
- **N4 (imprecision) ADR-0105 item 8**, unchanged sentence in a changed item: "`tcsetpgrp` from the background is
  `EPERM`" contradicts item 7 and ADR-0104 item 10 (finding 1's residue).
- **N5 (imprecision) ADR-0104 INV-0104-4**: "or declares its own lone job a group" describes the case item 8 dropped
  (finding 22's residue); the invariant no longer matches its item.
- **N6 (imprecision) ADR-0100 item 7**: the first sentence still has the tool read `libc.a` "with `zoraya-elf`"
  while the new last sentence says `zoraya-elf` cannot and the readers are the tool's own (finding 15's residue).
- **N7 (imprecision) ADR-0100 Option B, Cost**: "the 126 rows" does not equal its own parenthesis (133 with `time`
  11, or 127 with the 5 clock rows; plus the five stdio rows: 138 or 132); `readv`/`writev` are `file` rows, not
  stdio (finding 10's residue; the "about 110" estimate is defensible from 132 minus the 22 pure rows named).
- **N8 (imprecision) ADR-0104 item 9**: "as POSIX excludes the caller ([P13])". `kill.2` (fetched): "POSIX.1
  requires that `kill(-1,sig)` send `sig` to all processes that the calling process may send signals to, except
  possibly for some implementation-defined system processes. Linux allows a process to signal itself, but on Linux
  the call `kill(-1,sig)` does not signal the calling process." The exclusion is Linux's behaviour, which the ADR
  rightly copies; the attribution should say Linux, and sources.md's [P13] summary should carry the sentence.
- **N9 (imprecision) ADR-0109 item 5**: "`zoraya-elf` already parses them (at most `MAX_SEGMENTS` loadable, 8
  headers of 56 bytes at most for a static tcc output)" conflates the loadable segments `zoraya-elf` keeps with the
  program header table musl needs. `static_init_tls` ([M10], fetched) looks for `PT_TLS`, `PT_PHDR`, `PT_DYNAMIC`
  and `PT_GNU_STACK`, none of which is `PT_LOAD`, and `zoraya-elf` skips non-`PT_LOAD` headers
  (`crates/zoraya-elf/src/lib.rs`). The block must carry the whole table (`e_phnum` entries), and the room bound
  should be stated for `e_phnum`, not `MAX_SEGMENTS`. Harmless for a program without TLS; wrong for one with it,
  which is the case finding 13 was about.
- **N10 (note) ADR-0107 item 5**: "`BOOTSTRAP` (the channel end, read once and closed)" in the same item that now
  has the child read two messages; ADR-0100 item 4 still says "the bootstrap message" (singular) where ADR-0109
  item 4 says "messages".
- **N11 (note) ADR-0102**: the wrapper says the child takes "its pid, parent pid and group from the `spawn_prepare`
  reply", but the reply is `{pid, pgid, parent_job, child_job, child_broker_end}` (ADR-0104 item 4), with no parent
  pid; the child's `ppid` is the parent's own pid, elsewhere in copied memory. Item 3 does not say which generation
  the `self_slot` handle gets; INV-0102-7's check ("compares the raw handle values ... slot by slot") implies the
  parent's generation of `SELF`, which item 3 should state.
- **N12 (note) sources.md [P2]**: cited by ADR-0107 item 14 for Linux's `O_RDWR` on a FIFO; the claim is true
  against `fifo.7`, but the [P2] entry's summary does not mention it.
- **N13 (note) ADR-0102 item 4 vs ADR-0103 item 7**: the same 64-handle bound is `EAGAIN` from `fork` and `E2BIG`
  from `posix_spawn`/`exec`. Defensible (each call's errno set), but worth one sentence saying it is deliberate.

Item references: every "item N" in a changed line was resolved against the current numbering of ADR-0100 to
ADR-0110 and `inputs.md`; all point at the item they describe (ADR-0104's new `8a` included). The new Evidence
anchors (`task-02/method.md#tiers-runs-and-sets`, `fs/requirements.md#22-atomic-rename`,
`alternatives-postmortem.md#hubris`) exist.

### The five walkthroughs after the fixes

1. **`fork()` of dash: works as written.** The `SELF` slot, the generation rule, `parent_job`/`child_job` rights,
   the tty `clone` with `spawn_prepare`'s `pgid`, and the method names are consistent across ADR-0102, 0103, 0104,
   0105, 0106 and 0107 (N11 is wording).
2. **`exec` of `/bin/sh` with the parent's `waitpid`: works as written.** `parent_job` has `WAIT`, `PROCESS_JOB`
   has `CREATE_PROCESS`, the attribute block comes from POSIX dispositions, and the spawner can hand the new image
   its `SELF` handle because `spawn` returns `DUPLICATE`. N2 sits beside this path, not on it.
3. **Ctrl-C during `make -j4`: the signal path works as written; the acceptance script still fails at the
   `W9-STATUS` step (N1).** `tcsetpgrp` is allowed for both dash calls, `job_signal(fg_job, Interrupt, 2)` reaches the
   group, each make's `SELF` subscription takes the request, the re-raise and `reap`'s `WTERMSIG` are as before; but
   dash abandons the typed line after the job dies of `SIGINT`, so `W9-STATUS 130` is never printed by the script of
   ADR-0110 item 4.
4. **The FIFO jobserver: works as written.** `open_with_mode` carries `S_IFIFO`; the held open is now a listed
   behaviour of ADR-0055's loop with its `PEER_CLOSED` rule.
5. **`tcc hello.c && ./a.out`: works as written.** `a.out` is created `0o777 & ~umask` through `open_with_mode`,
   `AT_PHENT` is in the one auxiliary vector; N9 does not bite a program without TLS.

### What was checked and not checked

- **Checked:** every hunk of the diff; the 50 dispositions against the changed text; every "item N" reference in
  a changed line against the current numbering; [M10] and [D3] against the fetched files; `fifo.7` and `kill.2` for
  the two changed claims that cite them; `libc-surface.csv` (T0+T1 rows: 248; categories; the `_`-prefixed rows;
  the rows named as pure), `applets-by-tier.tsv`, `w10.sh`, `w9_session.py`, `task-02/method.md` line 68,
  `event-timelines.md` line 54; `ProcessObject::RIGHTS` and `JobObject::RIGHTS` in `crates/zoraya-abi/src/lib.rs`,
  `kernel/src/syscall.rs`'s `spawn` (rights checked per entry, child handle inserted with `ProcessObject::RIGHTS`),
  `MAX_SPAWN_HANDLES`, `STARTUP_BLOCK_MAX`, `MAX_SEGMENTS`, `open_count` in `zoraya-fs-service`; ADR-0040's rights
  table, ADR-0042's Layout line, ADR-0003 rules 1 to 3 and INV-0003-7, ADR-0002 rule 3(a) and INV-0002-3/-5,
  ADR-0004 rules 2 and 8 and its blocking sentence, ADR-0006 rule 1 and INV-0006-9, ADR-0010's personality mapping
  and rule 7, ADR-0013 rule 8, ADR-0033's rejection list, ADR-0046's `TERMINATED` sentence, ADR-0055's Replies and
  `MAX_OPEN`, ADR-0005's INV-0005-1/-2/-4; `adr-check`.
- **Not checked:** unchanged text beyond what a changed line depends on; the other 42 `sources.md` entries; whether
  tcc's static output has more than 8 program headers (N9 stands on musl's loop regardless); the options and costs
  of the eleven ADRs except where a changed line touched them; W9b, Ctrl-Z/`fg`/`bg` and the pipe-style jobserver;
  the arithmetic of ADR-0100's "about 110" beyond the recount above.

### Disposition of the check (author, 2026-11-02)

All thirteen new problems are fixed in the commit that follows this section: N1 (ADR-0110 item 4: `W9-STATUS` and
`W9-PARTIAL` are separate lines after the prompt), N2 (ADR-0104 item 8a: a new pid's record starts as the caller's
for `fork` and `Default`, or `Ignore` per the attribute block, for `posix_spawn`; `exec_commit` resets `Handler` to
`Default`), N3 (ADR-0104 item 9 and ADR-0105 item 3: a group stop goes process job by process job, each by its own
record), N4 (ADR-0105 item 8), N5 (INV-0104-4 reworded to item 8's rule), N6 (ADR-0100 item 7's first sentence),
N7 (ADR-0100's Cost: 133 rows plus three stdio rows, less the pure ones), N8 (ADR-0104 item 9 attributes the caller
exclusion to Linux; `sources.md` [P13] carries the sentence), N9 (ADR-0109 item 5: the whole program header table,
bounded by `e_phnum`, with an accessor in `zoraya-elf`), N10 (ADR-0107 item 5 and ADR-0100 item 4: "messages"),
N11 (ADR-0102: the parent pid is the parent's cached pid; the `SELF` handle takes the parent's generation of that
slot), N12 (`sources.md` [P2] carries the `O_RDWR` sentence), N13 (ADR-0102 item 4 says why `fork` and `spawn`
report the bound with different `errno` values). The four "partly fixed" findings (1, 10, 15, 22) are the residues
N4, N7, N6 and N5 and are closed by the same edits.

## Check of the second-round fixes (third reviewer)

Checker: a third subagent that wrote neither the ADRs, nor the first review, nor the second check. Compared 10a6458
("independent review and its fixes") with dadec47 ("fixes from the check of the review fixes"), on 2026-11-02, over
`git diff 10a6458..dadec47 -- docs/adr docs/research/stage-6/sources.md` (8 files, 47 insertions, 35 deletions).
Scope: the changed lines and the unchanged lines they refer to. Read for this check: `tools/posix-surface/remote/w9_session.py`
(the Ctrl-C step and the separate `echo MAKE_STATUS=$?`), `crates/zoraya-elf/src/lib.rs` (what the parser keeps),
`docs/research/task-02/libc-surface.csv` (the T0/T1 counts behind ADR-0100's Cost), ADR-0103 item 7, ADR-0104 items
2 to 8, ADR-0105 item 7, ADR-0107 item 5 and its bootstrap record, and the `sources.md` entries [P3], [P7], [D1], [M10].

**Totals:** 12 confirmed fixed, 1 partly fixed (N9), 0 not fixed. New problems: 7 (0 errors, 2 imprecisions, 5
notes), listed after the table.

| # | Verdict | Evidence |
|---|---|---|
| N1 | confirmed fixed | ADR-0110 item 4 now types `cd /work/w8 && make -j4 >/dev/null`, then `~ 1500`, then `\x03`, "then wait for the prompt", and only then "separate lines at the prompt: `echo W9-STATUS $?`, then `n=0; for f in t*.h; ...; echo W9-PARTIAL $n`". It gives the reason ("An interactive dash whose foreground job died of `SIGINT` raises `SIGINT` on itself and abandons the rest of the typed line ([D1] ...)"), which matches [D1]'s summary ("`raise(SIGINT)` when the job died of `SIGINT`") and the harness: `w9_session.py` sends `\x03`, comments "after Ctrl-C an interactive shell abandons the rest of the line, so the status is asked for separately", waits for the prompt and then writes `echo MAKE_STATUS=$?\n` on its own. Same shape as W5's `echo status=$?` in item 3. |
| N2 | confirmed fixed | ADR-0104 item 8a: "A new pid's record starts at `spawn_prepare` as the caller's record for `fork` (POSIX keeps dispositions across `fork`, [P3]) and as `Default` for `posix_spawn`, with `Ignore` where the attribute block says so; `exec_commit` resets `Handler` to `Default` and keeps `Ignore`, as `exec` does ([P7])". [P7]'s summary carries "caught signals reset to default, ignored stay ignored". The fix lives in item 8a rather than in items 4 and 6 as N2 asked; items 4 and 6 (unchanged) say nothing that contradicts it, and item 6's `exec` path already reports the stop signals "by their POSIX disposition". Residues are P3 and P4 below (wording, and the [P3] summary). |
| N3 | confirmed fixed | ADR-0104 item 9: "For a group (`-pgid`, or the terminal's `suspend(20)` on the foreground group, ADR-0105 item 3) the broker goes process job by process job, each by its own record, since the records are per pid; a `job_suspend` of the whole group job happens only when every member's record is `Default`." ADR-0105 item 3: the broker "applies each foreground process's recorded disposition for `SIGTSTP` (ADR-0104 items 8a and 9: `job_suspend` of its process job, with the pid marked stopped ..., or `Custom(20)` to a handler, or nothing when ignored)". The two now describe the same per-process rule; the old "always suspend" sentence is gone. ADR-0105 does not mention ADR-0104's whole-group shortcut (P5, equivalent in effect). |
| N4 | confirmed fixed | ADR-0105 item 8: "`SIGTTOU` is sent nowhere (`TOSTOP` off; `tcsetpgrp` from the background succeeds, item 7)". Item 7 (unchanged): "any process of the session may make a group of the session the foreground group ... `pgid` outside the session is `EPERM`". Agrees with ADR-0104 item 10. |
| N5 | confirmed fixed | INV-0104-4: "`setpgid` never moves a process between jobs: it succeeds only when the process's group is already the one named, and changes nothing. *Check:* ... the pipeline case (`Join` then `setpgid`) and the refused case (`EPERM`)". Item 8 (unchanged): "If the process's group pid already equals `pgid`, the call succeeds and changes nothing ...; anything else would move a process between jobs and is `EPERM`". The invariant matches its item again. |
| N6 | confirmed fixed | ADR-0100 item 7, first sentence: the tool "reads `libc.a`'s members with its own `ar` reader (an archive is a header per member) and `ET_REL` symbol-table reader, and lists defined global symbols". The last sentence (unchanged) says the same: the readers "are part of the tool, not of `zoraya-elf`". No contradiction left. |
| N7 | confirmed fixed | ADR-0100 Cost: "the 133 rows of the machine-facing categories (process 51, file 40, signal 12, dir 7, tty 6, memory 6, time 11) plus `read`, `write` and `close` from stdio". Checked against `task-02/libc-surface.csv` (T0 and T1 rows): process 51, file 40, signal 12, dir 7, tty 6, memory 6, time 11 = 133; `read`, `write`, `close` are stdio rows; `readv`/`writev` no longer listed as stdio. The 22 pure rows named (9 process, 3 signal, four `dl*` in process, six time rows: `ctime`, `localtime`, `localtime_r`, `mktime`, `strftime`, `strptime`, leaving `clock_gettime`, `gettimeofday`, `nanosleep`, `time`, `times` as the 5 that touch the clock) give 136 − 22 = 114, which "about 110 `[estimate]`" covers. See P6 on the `dl*` label. |
| N8 | confirmed fixed | ADR-0104 item 9: "for `-1`, `job_signal` on every process job of the session but the caller's, which is Linux's behaviour ([P13]: POSIX lets the caller be included)". `sources.md` [P13] now ends: "POSIX requires `kill(-1, sig)` to reach every process the caller may signal, and on Linux the call does not signal the caller itself." Attribution moved to Linux as asked; the parenthesis understates POSIX (P2). |
| N9 | partly fixed | ADR-0109 item 5: "The whole table goes, all `e_phnum` entries of 56 bytes, not only the `PT_LOAD` ones `zoraya-elf` keeps: musl's `__init_tls` looks for `PT_TLS`, `PT_PHDR`, `PT_DYNAMIC` and `PT_GNU_STACK` ([M10]), which `zoraya-elf` skips when loading". That is what `crates/zoraya-elf/src/lib.rs` does: `parse` reads `phnum` from `e_phnum`, keeps only `PT_LOAD` (`if u32_at(ph, 0) != Some(PT_LOAD) { continue; }`) into `segments: [Segment; MAX_SEGMENTS]`, and `Program` holds `entry`, `segments`, `count` and nothing of the table. "An accessor for the raw table" is a consistent requirement: `parse` has already checked that `phoff..phoff + phnum * 56` lies inside `file`, so `Program` can keep `phoff` and `phnum` and return the slice of the caller's file bytes. Not fixed in the same sentence: "`zoraya-elf` gains ... a bound on `e_phnum` (64 `[assumption]`, refused above)". The crate already has one, `const MAX_PHDRS: usize = 32;` with `if phentsize != PHDR_SIZE \|\| phnum > MAX_PHDRS { return Err(Error::BadHeaders); }` (P1). |
| N10 | confirmed fixed | ADR-0107 item 5: "`BOOTSTRAP` (the channel end, read at start-up and closed)", and the same ADR's "the child's start-up reads both messages before anything else"; ADR-0100 item 4: "initialises `zoraya-posix` from the bootstrap messages of ADR-0107"; ADR-0109 item 4 (unchanged): "reads the bootstrap messages of ADR-0107". All plural. |
| N11 | confirmed fixed | ADR-0102 item 3: `self_slot` receives "a handle to the *child* process itself with `WAIT`, at the generation the parent's slot carries", which is what INV-0102-7's check ("compares the raw handle values of the child's table with the parent's, slot by slot") needs. The wrapper paragraph: the child takes "its pid and group from the `spawn_prepare` reply that is in its copied memory and its parent pid from the parent's own cached pid", so the non-existent parent pid of the reply (`{pid, pgid, parent_job, child_job, child_broker_end}`, ADR-0104 item 4) is no longer cited. See P7 on "cached". |
| N12 | confirmed fixed | `sources.md` [P2] now ends: "under Linux opening a FIFO for reading and writing succeeds, which POSIX leaves undefined", which is the claim ADR-0107 item 14 cites it for. |
| N13 | confirmed fixed | ADR-0102 item 4: "`EAGAIN` from `fork` (the `errno` POSIX gives `fork` for a resource limit; `posix_spawn` and `exec` report the same bound as `E2BIG`, their own 'too many arguments' error, ADR-0103 item 7)". ADR-0103 item 7 (unchanged) says `E2BIG` from `posix_spawn` and `exec` for the same `MAX_SPAWN_HANDLES` of 64. The difference is now stated as deliberate, with the reason. |

### New problems in the changed lines

- **P1 (imprecision) ADR-0109 item 5.** "`zoraya-elf` gains an accessor for the raw table and a bound on `e_phnum`
  (64 `[assumption]`, refused above)". The crate already bounds `e_phnum`: `crates/zoraya-elf/src/lib.rs` has
  `const MAX_PHDRS: usize = 32;` and refuses `phnum > MAX_PHDRS` with `Error::BadHeaders`. The sentence should say
  whether `MAX_PHDRS` stays 32 (then the block's bound is 32 × 56 = 1,792 bytes, and `[assumption]` is not needed) or
  is raised to 64 and why. Either fits the one-page room the item claims (64 × 56 = 3,584 bytes), so the design is
  unaffected; the claim about the existing code is not true as written.
- **P2 (imprecision) ADR-0104 item 9.** "([P13]: POSIX lets the caller be included)". `kill.2`, as the new [P13]
  summary itself says, has POSIX *require* `kill(-1, sig)` to reach "every process the caller may signal", which
  includes the caller; Linux's exclusion is the deviation. "lets" should be "requires", or the parenthesis should
  quote [P13]'s sentence.
- **P3 (note) ADR-0104 item 8a.** "as `Default` for `posix_spawn`, with `Ignore` where the attribute block says so".
  In an item about `posix_spawn`, "the attribute block" reads as POSIX's `posix_spawnattr_t`, which can only reset
  signals to default (`POSIX_SPAWN_SETSIGDEF`) and never set `Ignore`. Item 6 uses the same words for Zoraya's spawn
  attribute block, whose `Ignore` entries are the caller's `SIG_IGN` dispositions; that is the meaning that agrees
  with POSIX (ignored signals stay ignored in the child). One qualifier ("the spawn attribute block of item 6", or
  "where the caller ignores it") removes the double reading.
- **P4 (note) `sources.md` [P3]**, cited by ADR-0104 item 8a for "POSIX keeps dispositions across `fork`". The [P3]
  summary lists what `fork` does not inherit and says nothing about dispositions; the claim is true (the man page's
  "exact duplicate ... except" list does not name them), but the summary does not carry it, the pattern N8 and N12
  were about.
- **P5 (note) ADR-0104 item 9 vs ADR-0105 item 3.** Only ADR-0104 has the shortcut "a `job_suspend` of the whole group
  job happens only when every member's record is `Default`"; ADR-0105 item 3 describes the terminal's `suspend(20)`
  as `job_suspend` of each process job. The effect is the same (every member stopped, every pid marked), so this is
  not a contradiction, but the earlier unchanged sentence of item 9 ("`job_suspend` of the target's job (the process
  job, or the group job for `-pgid`)") now reads as the special case of the sentence that follows it; one of the two
  could be dropped.
- **P6 (note) ADR-0100 Option B, Cost.** "the four `dl*` rows" are counted among "the rows that are pure and stay musl
  C", while item 7 removes `dlopen`, `dlsym`, `dlclose`, `dlerror` from the tier list ("the ports are built without
  them") and item 8 has tcc define its own dummies. They are subtracted either way, so the "about 110" is unchanged;
  the label is not. Pre-existing in substance (the previous text said "the `dl*` rows"); the changed line keeps it.
- **P7 (note) ADR-0102, the wrapper paragraph.** "its parent pid from the parent's own cached pid" is the first use
  of "cached" in the ADRs. The pid `libposix` holds in memory is the bootstrap record's `pid` (ADR-0107's record:
  "`pid`, `ppid`, `pgid`, `sid`, ..."), so the sentence is true; naming the record would say where the value comes
  from.

Item references in the changed lines were resolved against the current numbering: ADR-0104 item 9 → ADR-0105 item 3
(Signals), ADR-0105 item 8 → item 7 (Job control calls), ADR-0105 item 3 → ADR-0104 items 8a and 9, ADR-0102 item 4
→ ADR-0103 item 7 (`spawn`), ADR-0109 item 5 → [M10], ADR-0110 item 4 → [D1], ADR-0104 item 8a → [P3] and [P7]: all
point at what they describe. Every new claim in the changed lines carries a label or a citation except those P1 to P7
name. Line length: five added lines exceed the ADRs' usual 120 columns (ADR-0102 item 3 and the wrapper paragraph,
ADR-0105 item 3, the two `sources.md` entries); no lint bounds it and every ADR in the set already has such lines, so
this is not listed as a problem.

### adr-check

`cargo run -q -p zoraya-adr-check -- docs` at dadec47: "59 ADRs checked against INV-0001-1..5", the eleven
`INV-0001-5: index.md: 01NN-....md is not listed` errors for ADR-0100 to ADR-0110, 17 `not yet applicable (stage 6)`
warnings (INV-0002-2 to INV-0006-9, as at 10a6458), "11 errors, 17 warnings". Nothing else.

### What was checked and not checked

Checked: every added or changed line of the eight files in the diff, against the unchanged text around it and the
items, invariants and source entries it names; N1 against `w9_session.py` and [D1]; N7's arithmetic against
`libc-surface.csv`; N9 against `crates/zoraya-elf/src/lib.rs`; N2, N3, N4, N5, N13 against the items of ADR-0103,
ADR-0104 and ADR-0105 they now rely on. Not checked: anything outside the changed lines (the first review and the
second check cover it); the removed lines beyond confirming that what they said is gone or moved; the man pages
themselves (the [P2], [P3], [P7], [P13] summaries in `sources.md` were taken as the second check left them, with the
one mismatch P2 names being between the ADR and the summary, not between the summary and the page); `plan.md` and
`inputs.md`, which the commit does not touch; `make test`.

### Disposition of the third check (author, 2026-11-02)

The third check found no error, so the review loop ends here as TASK-15 prescribes. Its seven small problems are fixed
all the same in the commit that follows, without a further check: P1 (ADR-0109 item 5 names `zoraya-elf`'s existing
`MAX_PHDRS` of 32), P2 (ADR-0104 item 9: POSIX requires the caller to be included, Linux leaves it out), P3 (item 8a
names the Zoraya spawn block of item 6), P4 (item 8a cites ADR-0004 rule 6 instead of [P3]), P5 (ADR-0105 item 3
carries the whole-group shortcut), P6 (ADR-0100's Cost says the `dl*` rows are not built), P7 (ADR-0102's wrapper
takes the parent pid from the bootstrap record). These edits were not checked by a reviewer.
