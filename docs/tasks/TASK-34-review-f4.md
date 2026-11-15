# TASK-34 review: part F4 (`posix_spawn` with what each child is given; Unix sockets and memory descriptors crossing spawn)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f4` at 1cc2ca72, 2026-11-15, read
against `docs/tasks/TASK-34-foundations.md`, `docs/research/browser-engine/plan.md` row F4, ADR-0411 and ADR-0412
(accepted), the proposed ADR-0433, ADR-0434 and ADR-0435, the rule-6 notes on ADR-0287, 0310, 0341, 0411, 0412, 0430 and
0432, and the lessons of `TASK-34-review-f2.md` and `-f3.md`. Code read around the diff: `Posix::resolve`,
`open_place`, `getcwd` (`crates/zoraya-posix/src/process.rs`), `zoraya_resolve::resolve`, `net_call_with`
(`net.rs`), `RtSys::channel_write` and `channel_send` (`ffi.rs`), the F2 send path and `after_fork_in_*`
(`unix_io.rs`), `memfd.rs` `wire`/`sent`, `netd`'s `Service::add`, `Narrow` and `Connection`, the kernel's handle
rights check in `channel_write` (`kernel/src/syscall.rs`), musl's `system.c` (kept by `ports/musl/musl.mk`), and
CPython 3.15.2's `Lib/subprocess.py`, `configure.ac` and `Modules/posixmodule.c` from the pinned tarball.

Tests run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("make test: all checks passed"; adr-check, host paths and test caps among them).
- `cargo test -p zoraya-posix -p zoraya-net -p zoraya-rng -p zoraya-abi`: exit 0, 513 tests passed, none failed.
- `make test-libposix`: exit 0, `PASS: 153 steps`; `t_spawn` `table`, `table2`, `default`, `refuse`, `grants`
  (`parent_random=1`), `unix`, `memfd`, `addopen` and `spawnp` each printed `passed`.
- Two temporary host tests (added to `tests/spawn.rs`, run, then removed with `git checkout`) that reproduce finding 1.
- Nine mutations applied by hand, one at a time, each followed by `cargo test -p zoraya-posix` and `git checkout`
  (finding 9). The tree is clean.

Not run: the python lanes (`test-python`, `-regr`, `-ext`), the CPython build with patch 0007 (it was dry-run with
`patch --dry-run` on the pinned source: it applies cleanly), and a full `make mutants-host`.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

1. **error**: `addopen` gives the child the wrong file, or fails with `EBADF`, when an earlier action names the number
   of the parent's temporary fd. Where: `crates/zoraya-posix/src/spawn.rs:163-172` (`FileAction::Open` becomes
   `FileAction::Dup2(t, *fd)`, `t` a *parent* fd number) read by `crates/zoraya-posix/src/jobs.rs:952-957`, where the
   source of a `Dup2` is looked up in `map` keyed by *child* fd numbers after the earlier actions. What is wrong: the
   resolved `Dup2(t, fd)` is read as "the child's fd `t` as it stands now", and an earlier action may have changed the
   child's `t`. Scenario (reproduced): the parent's lowest free fd is 0; actions `addopen(1, "/x")`, `addopen(0, "/y")`.
   The temps are 0 (`/x`) and 1 (`/y`); the resolved list is `Dup2(0, 1)`, `Dup2(1, 0)`; the second reads the child's
   new fd 1 (= `/x`). The child gets `/x` at both 0 and 1 and never `/y` (temporary test: `child fd 0 -> "X", 1 -> ""`,
   with and without `POSIX_SPAWN_EMPTY_NAMESPACE`). Second scenario (reproduced): `addclosefrom_np(0)` then
   `addopen(0, "/x")` without the flag is `EBADF` (the temp's map entry was removed by the close-from), where POSIX
   opens the file. A child given its log in place of its input, or a writable file at the fd meant for a read-only one,
   is the wrong-descriptor case DM-002 is about. Fix: carry the open as its own action to the end (e.g.
   `FileAction::Opened(parent_fd, child_fd)` whose source is the parent's fd, never looked up in `map`), and add the two
   scenarios as tests.

2. **error**: a spawn without any attribute of this part now moves an inherited (unlisted) packet socket to the child
   and takes an inherited stream socket's ancillary channel, which ADR-0412 item 6 does not allow and no ADR records.
   Where: `crates/zoraya-posix/src/jobs.rs:1031-1037` (every Unix socket in the fd map goes to `pass_to_child`, listed
   or only inherited), `unix_io.rs:441-446` and `:450-460`. ADR-0412 item 6 (accepted): "A sequenced-packet or datagram
   socket *listed in a spawn's file actions* moves to the child"; ADR-0433 item 6 extends it to "inherited, or the target
   of a `dup2`" and flags only the `SCM_RIGHTS` difference as a deviation. Scenario: a C program holds a
   `socketpair(AF_UNIX, SOCK_DGRAM, 0)` end (no `SOCK_CLOEXEC`, the default in C) and calls `system("date")`; musl's
   `system.c` (kept, `ports/musl/musl.mk:24`) calls this `posix_spawn` with no file actions. The socket moves to
   `/bin/sh`, which exits; the parent's end is `EPIPE` for ever. A stream socket in the same program loses its ancillary
   channel to the shell and `SCM_RIGHTS` on it is `EOPNOTSUPP` from then on. Before F4 the same `system()` failed
   loudly (`EBADF`); now it silently breaks the parent. INV-0411-4 ("gets what it gets today") and ADR-0433 driver 4
   ("the old spawn is untouched") are checked only for files (`a_spawn_without_the_attributes_gives_what_it_gave_before`
   opens no socket). Fix: move a packet socket, and move a stream's ancillary channel, only for a socket that a `dup2`
   action names (or only with `POSIX_SPAWN_EMPTY_NAMESPACE`); give an inherited stream socket as shared streams alone
   (`r.shared = true`, channel kept, as `fork` does); and record the choice in ADR-0433 as a deviation for the architect.

3. **error**: `netd`'s `Duplicate` lets any program that holds a network connection fill `netd`'s whole connection
   table (64, `crates/zoraya-net/src/service.rs:31`), so every later program start that needs a narrowed connection,
   and every other program's `GRANT_NET`, gets `BUSY`. Where: `crates/zoraya-net/src/service.rs:493-512` (no right, no
   per-connection count: `self.add(connection.clone())`). ADR-0434's "Negative" says so; its INV-0434-1 ("refused only
   by `BUSY` at the table's bound") writes the hole into the invariant. ADR-0262's budget does not bound it (it counts
   TCP connections, `Kind::Connections`, not `netd` channels). The same defect in `rngd` was review finding 3 of an
   earlier part and is why ADR-0288 item 6 exists; ADR-0435 keeps that bound for randomness while ADR-0434 drops it
   for the network, which is the scarcer table. Before F4 a program could make no connection at all (no `narrow`
   right). Scenario: a program given `127.0.0.1:8080` loops on `posix_spawn(..., GRANT_NET)` of a child that sleeps, or
   sends `Duplicate` itself on its connection: 63 requests, and the shell cannot start another networked program.
   Fix: count live duplicates per connection and refuse past a small bound (as `rngd`'s `PROGRAM_CHILDREN`), with a
   test at the bound; change INV-0434-1 to say so.

4. **warning**: a stream socket handed to a child gives the parent and the child two byte counts for one stream while
   the parent keeps its copy, and the child's `SCM_RIGHTS` then lands on the wrong bytes at the peer. Where:
   `crates/zoraya-posix/src/unix_io.rs:422-448` (record with `read_off`/`write_off` and `has_anc`, `shared = false`),
   `:492-499` (the parent marked shared). This is the deviation from ADR-0412 item 6 that ADR-0433 asks the architect to
   accept, and the F2 bug class (both sides sending). Scenario: the parent writes 10 bytes on the end it passed after the
   spawn; the child then sends a descriptor with one byte: the child's tag says offset `w`, the stream's position is
   `w + 10`, so the peer attaches the child's descriptor to the parent's bytes. The reverse (the parent reads) delivers
   the peer's descriptors to the child late. The ADR's own condition ("once the parent has closed its own") is not
   enforced and is not in any test (`a_stream_socket_crosses_a_spawn...` writes on `a`, the other end). Fix, for the
   architect to choose: move the stream socket whole (the parent's copy dead, `EPIPE`, as a packet socket) when its
   ancillary channel moves, or keep ADR-0412's "`EOPNOTSUPP` in both".

5. **warning**: a refusal of the third message after the second was sent loses the parent's window channel and its
   moved network connection, against INV-0433-2 ("a spawn that is refused ... changes nothing of the parent's ...
   connections"). Where: `crates/zoraya-posix/src/ffi/procs.rs:1881-1890` (`second_sent` then
   `p.spawned(&Inherit::empty(), &granted)`), `:1735-1746` (`channel_send` of `ZPBF` mapped to `EBUSY`/`EAGAIN`).
   Scenario: `posix_spawn` with `GRANT_WINDOW` and a socket fd; the kernel refuses the `ZPBF` write (`BadState`): the
   window went with the `ZPBX` message to a child that ends with 127, `posix_spawn` returns `EBUSY`, and the program has
   no window. Fix: validate everything the third message carries before the second is written (the kernel refuses a
   write only for reasons the library can check first), or send the descriptors in the second message, or record the
   exception in INV-0433-2 and ADR-0433 item 5.

6. **warning**: the terminal entry of a chosen table (`/dev/tty` listed by name: `clone_tty` for the child's group) is
   untested on the host and in QEMU. Where: `crates/zoraya-posix/src/spawn.rs:263-270`; no `dev/tty` in
   `tests/spawn.rs` or `tests/posix-core/t_spawn.c`. The mutants "delete match arm `EntryKind::Terminal`" and
   "`len() - 1` with `+`" survive (the second confirmed by hand, finding 9). Fix: list `/dev/tty` in `t_spawn table`
   (the child's `isatty` and a write to it) and a host test that the entry's handle is a clone for the child's group.

7. **warning**: the step-4 test for a shared stream does not test what it says, and the fork-then-spawn case it should
   cover is the F2 bug class. Where: `crates/zoraya-posix/tests/spawn.rs:1196-1212` (the socket is shared because a
   spawn took its channel, so `ctl` is `None` and the guard `Some(c) if !u.shared` at `unix_io.rs:441` is never
   reached). The mutant "guard `!u.shared` with `true`" still survives (confirmed by hand). Scenario it lets through: a
   process `fork`s (its stream is shared, the channel kept, `after_fork_in_parent`) and then spawns with the socket:
   with the mutant the channel moves and the spawned child believes it is the stream's only reader and may send
   descriptors while the fork child reads the same bytes. Fix: a test that forks (or sets `shared` with the channel
   kept), spawns with the socket, and asserts `rec.shared && !rec.has_anc` and that the parent still has `ctl`.

8. **note**: `addopen` runs before the entries and grants are checked, so a spawn refused for a bad entry has already
   created or truncated the file (`spawn.rs:160-176`, before `chosen_entries`); `posix_spawnp` repeats every attempt,
   `addopen` and `Duplicate`/`Share` included, for each `$PATH` directory and also goes on after an `EACCES` that came
   from an entry or a grant, not from the file (`procs.rs:2478-2496`). `O_CLOEXEC` in `addopen`'s flags is ignored
   (the resolved `Dup2` clears it; POSIX closes that fd at the exec). Fix: check the shape and entries first, then open;
   stop `posix_spawnp` on an error that is not about the file; drop the target for an `O_CLOEXEC` open. Record in
   ADR-0433 item 4 otherwise.

9. **note** (mutation testing): `build/mutants/cargo/mutants.out/missed.txt` predates step 4 (20:02; step 4 committed
   20:06). Hand re-runs at 1cc2ca72: `parse_spec` `>`→`>=`, `spawn_refused`→`()`, the bound's `>`→`==` and `+`→`*` are
   now caught; still missed: `unix_io.rs:441` guard→`true` (finding 7, a real hole), `unix_io.rs:465` `>`→`>=` (the
   test has 63 and 66 handles, never 64: a small boundary hole), `jobs.rs:955` guard→`true` (a real hole: no test of
   `addclose(n)` then `adddup2(n, m)`, which must be `EBADF`), `spawn.rs:88` `&`→`|` (a real host hole: `MOVE_NET`
   alone through `Shape::check` is never tested; `t_spawn grants` covers it in QEMU), `spawn.rs:269` (finding 6).
   Equivalent or dead, not holes: the five `child_fds` replacements (`jobs.rs:926`, no caller is left; delete the
   function), `spawn.rs:41` and `:277` `|`→`^` (disjoint bits), the three `retain` `!=`→`==` at `unix_io.rs:467-469`
   (on `E2BIG` the whole `Inherit` is dropped, so the roll-back is dead code; delete it), and `Passed::is_empty`→`true`
   (used only by `send_record`, not host-testable; `t_spawn unix`/`memfd` kill it in QEMU). Each real hole wants a
   line in `tools/mutants/` once it has a test.

10. **note**: `send_record` leaks `me` (the WAIT|TRANSFER duplicate of the process) when the `ZPBX` write fails
    (`procs.rs:1726-1729`: the kernel leaves the handles with the caller and only `granted.made()` is closed). The
    leak was there before F4 (the `SELF` write); the new code path keeps it.

11. **note**: CPython (patch 0007, `PYTHON_ABSENT`). The patch applies to 3.15.2 cleanly. Every spawn function
    `posixmodule.c` calls (`posix_spawn`, `posix_spawnp`, `addopen`, `addclose`, `adddup2`, `addclosefrom_np`,
    `setflags`, `setpgroup`, `setsigmask`, `setsigdefault`, `setschedpolicy`, `setschedparam`, init/destroy) is in
    `libc.a`, and `addclosefrom_np` is declared under `_GNU_SOURCE`, so `configure` finds it and
    `_HAVE_POSIX_SPAWN_CLOSEFROM` holds: the build risk is small. What the lanes run barely touches it:
    `regr-ignore.txt` leaves out every case that starts another interpreter, and `smoke.py` uses `os.fork`, so
    `subprocess`'s new path is not exercised by any lane; the patch's only evidence is `t_spawn`, not Python. The
    candidate run maps `ports/patches/python/` to `test-python`, `-regr` and `-ext` (`tools/test-map.txt:190`), so it
    must show those green before the merge. A Python program that holds a non-close-on-exec Unix socket is safe from
    finding 2 only because Python makes sockets non-inheritable and `close_fds=True` adds `closefrom(3)`.

12. **note**: `rngd` (ADR-0435). `may_share` (`crates/zoraya-rng/src/lib.rs:279-285`) gives depth 2 eight live
    children and depth 3 none; a refused share counts nothing; the test reaches the bound, releases one and checks a
    depth-3 leaf. One program cannot exhaust the table (8 of 256); 32 programs can, as the ADR says. Sound as written;
    the architect decides whether that is enough.

## Checked and found sound

- Rights of chosen entries are the service's: `:ro` opens with `READ | CLONE` and `PATH` with `ALL` through the
  parent's connection, and the tests ask the real file service (a write in `:ro` is `EACCES`, `Clone` wider is
  refused, `/ro` asked read-write of a read-only parent entry is `EACCES`, a path no entry covers is `EACCES` with
  nothing left made). `t_spawn refuse` shows the same on the real kernel (`/bin` read-write is `EACCES`).
- Memory descriptors: the object handle is copied with `WRITE` removed when the descriptor is not writable
  (`read_only` list), the kernel refuses a wider right (`syscall.rs`, `held.rights.contains`), the child's handle is
  read-only at the fake kernel and `PROT_WRITE` is refused; `writable_out`/`pinned` are set only after the spawn.
- Handle accounting on refusal: `inherit_shaped`, `grant_connections` and `spawn_into` close what was made on every
  path read (temps, entries, slots, `Duplicate`/`Share` channels; `Sys::channel_write` closes the moved end on a
  failed write). Passed descriptors are the parent's own handles, copied or moved by the message, so a refused spawn
  owns nothing new.
- Decoders: `decode_passed` checks the magic, the record count against the handles, increasing fds below `OPEN_MAX`,
  the kinds, and the exact length; the bit-flip test shows no panic and no handle left. `Bootstrap::decode` accepts
  `NO_ENTRY` only with an empty cwd; `ext_handles(EXT_FDS)` adds no handle; the child ends with 127 on an unreadable
  third message. A third message fits the child's 4096-byte buffer (64 records of 28 bytes at most).
- A child with no cwd: `zoraya_resolve::resolve` and `getcwd` fall back to `/` for an entry index out of the table,
  with no indexing panic.
- `GRANT_NET` and `MOVE_NET` together, unknown grant bits, a list without the flag and a path twice are `EINVAL` at
  the setter or the spawn; `MOVE_NET` and `GRANT_WINDOW` take the parent's handle only after the spawn happened.
- `unsafe` in `procs.rs`: `names_of`, `drop_names`, the attribute setters and `spawn_args` have `# Safety` sections
  and `// SAFETY:` comments that state the C contract; the offsets 280 and 288 are inside musl's 336-byte
  `posix_spawnattr_t` padding after `__fn`, and `init` zeroes the 336 bytes.
- `posix-surface-added` block of ADR-0433 and the seven new rows of `tools/libposix-widths.txt` match;
  `make check` (adr-check: invariants Total 838, host paths, test caps) passes.

## Verdict

**Fix first.** Findings 1 to 3 are errors: 1 gives a child the wrong file (reproduced), 2 changes what a default
spawn does to the parent's sockets against accepted ADR-0412 item 6 without an ADR, 3 lets one program deny the
network to all. Findings 4 to 7 need a fix or a record in this part; 4 is the architect's call on ADR-0433 item 6.

## Re-check (2026-11-15, head 09b9c876)

Re-check by the `opus-review` subagent of `git diff f2348e91 origin/work/f4` (bd2fea0e, 22c35974, 8a49df1b, c37628b7,
9d0da4c2, 497d0693, 09b9c876). `origin/main` (514b7d42) is an ancestor of the head (merged before f2348e91; no merge
since). Run on the worktree, one at a time, `CARGO_BUILD_JOBS=2`: `make check` exit 0 (adr-check: 838 rows, Total
matches); `cargo test -p zoraya-posix -p zoraya-net -p zoraya-rng -p zoraya-abi` exit 0, 520 passed, 0 failed; clippy
`-D warnings` on the four crates for the host (`--all-targets`) and `aarch64-unknown-none` exit 0; `make test-libposix`
exit 0, `PASS: 159 steps`, every `t_spawn` case passed (`inherited` and `queued` new). Two temporary tests (run, then
removed with `git checkout`) reproduce R1 and R2. Not run: `make test-python` (CPython is not built in this worktree;
a cross build of it next to QEMU does not fit this 8 GB host safely), so the candidate run must show the python lanes.

Fixed and sound: 1 (`FileAction::Opened(parent, child)` is never looked up in the child's map; both scenarios are
tests; `O_CLOEXEC` becomes `Close`, which ignores an absent fd), 2 (only targets of `dup2`/`addopen` move; an
unlisted Unix socket is `EBADF` with the parent's socket untouched, host test and `t_spawn inherited`; no action
sequence puts an unlisted socket at a listed number), 4 (every handle of a stream moves, the parent's is `EPIPE`, no
shared counts), 5 for a refused `ZPBF` (`t_queued`), 6, 7 (fork-shared stream: no channel, the parent's closed), 8
(entries before `addopen`; `posix_spawnp` recorded), 9 (dead roll-back and `child_fds` gone), 10 (`me` closed). ADR
edits touch only proposed ADR-0433/0434 and ADR-0412's own rule-6 note; no status or decision section changed.

R1. **error** (finding 3 not fixed): the `Duplicate` bound is bypassed by closing the connection that asked.
`crates/zoraya-net/src/service.rs:187-193` clears `origins` of every descendant when their origin closes, so each
becomes a family of its own with 8 more places. Reproduced: one program asks 8, closes the asker, asks 8 from a
duplicate, and so on: it holds 63 of the 64 slots. A child given `GRANT_NET` whose parent exits does the same without
trying. ADR-0434 and INV-0434-1 ("at most 8 live at once") are false as built. Fix: a family token that is not a slot
id (assigned to a connection made by `add` from outside, inherited by its duplicates, counted while any member
lives), or `rngd`'s depth rule; test: close the root, ask from a duplicate, still `BUSY`.

R2. **error** (new, INV-0433-2): with `POSIX_SPAWN_EMPTY_NAMESPACE`, the entries are made first
(`spawn.rs:163-174`), and the two later early returns, a failed `addopen` (`:195-198`) and a failed `child_fds_with`
(`:204-209`), close the temps but not `out.message`. Reproduced: entry `/d` and `addopen(7, "/gone")`: `ENOENT`,
live handles 5 before, 7 after. `posix_spawnp` repeats this per `$PATH` directory; a terminal entry also leaves its
clone at the tty service. Fix: close `out.message` on both paths (one helper for all three), with that test.

R3. **note**: `send_record` duplicates `me` after `ZPBF` is sent (`procs.rs:1703-1708`), so a failed duplicate (or a
failed `SELF` write) is a refused spawn whose sockets are dead in the parent. Duplicate `me` before `ZPBF`; record the
`SELF`-write case in ADR-0433 item 6.

R4. **note**: an `exec` has no file actions, so an image holding a non-close-on-exec Unix socket now gets `EBADF` from
`execve` (a child given a socket that runs `sh -c 'exec prog'` fails). ADR-0433 item 6's "`exec` takes the same path"
should say so. Stale text: "third message" in `docs/design.md:2214` and `unix_io.rs:503`.

**Verdict: fix first** (R1, R2).

## Re-check 2 (2026-11-15, head 477c560a)

Re-check by the `opus-review` subagent of `git diff 090952ee..477c560a` (cad9d2a7, 964fa36a, 9d772ab7, 477c560a). Run on
the worktree, one at a time, `CARGO_BUILD_JOBS=2`: `make check` exit 0; `cargo test -p zoraya-net -p zoraya-posix` exit 0,
428 passed, 0 failed (`closing_the_asker_does_not_reset_the_bound` and `a_refusal_after_the_entries_closes_them` among
them); `cargo clippy -p zoraya-net -p zoraya-posix --all-targets -- -D warnings` (host) exit 0; `make test-libposix` exit
0, `PASS: 159 steps`.

R1 fixed. `families` holds a token from a monotonic counter, never a slot id: `add` gives every connection from outside
(and every `Narrow`) a fresh token, `Duplicate` copies the asker's token with the flag set, `remove` clears only the closed
slot. Closing the origin changes no other slot's token, a duplicate of a duplicate carries the same token, and a reused
slot gets a fresh one, so ask-close-ask, duplicating a duplicate and closing the origin all stay at 8 live duplicates (the
count filters on a live connection, the flag and the token). A refused `add` (`BUSY`) changes nothing; a removed slot's
entry is `(0, false)` and never counts. The new test drives 40 rounds of close-one, ask-two with the root closed.

R2 fixed. `abandon` closes the slots' made ends, `out.message` and the temps on all four refusals of `inherit_shaped`.
No double close: `chosen_entries` closes `opened.conn` only before pushing it (`ENOTDIR`), a failed `clone_tty` pushes
nothing, `out` is dropped on the error path (no `Drop` closes it) and the callers close only what an `Ok` returned
(`spawn_into`, `spawn_refused`). Each `posix_spawnp` attempt is a fresh `inherit_shaped`, so the loop leaks nothing; the
test repeats three refusals three times with the handle count unchanged.

R3 fixed. `me` is duplicated before `ZPBF` is sent; its failure returns `(EAGAIN, false)` with nothing moved, and a
refused `ZPBF` closes it. A refused `SELF` after `ZPBF` is recorded as a limit in ADR-0433 item 6. R4 fixed (ADR-0433
item 6 states the `execve` `EBADF`; "third message" gone from `docs/design.md` and `unix_io.rs`).

N1. **note** (not in this diff): a connection that holds `narrow` can still make connections without bound by `Narrow`
(each starts a family), as before F4. A program's connection lacks `narrow` (ADR-0434 context), so this does not reopen
R1; it bounds only what ADR-0434 bounds.

**Verdict: merge** (after the candidate run is green; `make test-python` was again not run here).
