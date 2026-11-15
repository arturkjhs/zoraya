# TASK-31: review of part 10c, a POSIX program has a terminal from `zosh` (branch `work/10c`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...work/10c` at `8d64e80` (64 files): `crates/zoraya-tty`, `crates/zoraya-proto-tty`,
`crates/zoraya-shell` (`tty_host.rs`, `terminal.rs`, `paste.rs`, `grants.rs`), `crates/zoraya-broker`,
`crates/zoraya-proto-posix`, `crates/zoraya-posix` (`jobs.rs`, `poll.rs`, `termios.rs`, `ffi/procs.rs`, `ffi.rs`),
`user/src/bin/zosh.rs`, `user/src/bin/broker.rs`, `user/src/posix_child.rs`, `user/src/bin/term.rs`, the tests and
scripts, and the proposed ADR-0386 to ADR-0389. Read against ADR-0331 (items 4 to 9), ADR-0332, ADR-0333, ADR-0338,
ADR-0342 (items 3 to 5, 12), ADR-0158, ADR-0172's decision of 2026-11-15, the trust-boundary paragraph of
`docs/tasks/TASK-31-stage-10.md`, `docs/tasks/TASK-31-part-10c.md`, and what `TASK-31-review-poll.md` (item 12) and
`TASK-31-review-10b.md` (finding 2) left to 10c. The part's claims were re-run where this review needed them, not taken.

## What was run

In the worktree of `work/10c` and a detached scratch copy of `8d64e80` (breaks reverted after each),
`CARGO_BUILD_JOBS=3`, HVF:

| Command | Result |
|---|---|
| `make check` (scratch copy) | `make test: all checks passed` |
| `cargo test -p zoraya-tty -p zoraya-shell --lib` | 45 and 203 pass |
| `cargo test -p zoraya-posix -p zoraya-broker` | 237 pass, 0 fail |
| `make test-posix POSIX_SCRIPTS=<probe> POSIX_ACCELS=auto`, a probe script (below) | fails where finding 1 and 2 say |

The probe (`zosh` on the serial console, the image of `zosh.script`):

```
> sh
> /bin/sleep 60 &
> /bin/sleep 30          (then Ctrl-C)
> jobs; echo probe-jobs-end
> i=0; while [ $i -lt 70 ]; do /bin/sleep 0; i=$((i+1)); done; echo looped $i
> /bin/wc -c             (then abc, Enter, Ctrl-D; expects 4)
```

Its console (`target/test-logs/posix-zosh-rv10c-auto.log` in the `work/10c` worktree):

```
$ /bin/sleep 30
^C
[1] + Interrupt                  /bin/sleep 60
$ jobs; echo probe-jobs-end
...
looped 70
$ /bin/wc -c
[1] + Stopped (tty input)        /bin/wc -c
$ abc
sh: 6: abc: not found
FAIL: timed out waiting for "4"
```

Not run: the GUI steps (`make test-tour-terminal`, `make test-gui`), any TCG run, `make mutants-host` (the survivors
of the report were judged from its list in `build/mutants/cargo/mutants.out/missed.txt`, and ten of them were
applied by hand, below).

### Hand breaks (each alone, reverted)

| Break | Invariant | Result |
|---|---|---|
| B1 `SetAttr` from a line not in front accepted (`zoraya-tty` `lib.rs:438`) | INV-0331-2, INV-0386-2 | caught: `a_program_of_another_line_cannot_read_or_change_modes` |
| B2 raw-mode `readable` ignores `VMIN` (`discipline.rs:400`) | INV-0331-3 | caught: `poll_is_ready_only_when_a_read_would_not_block` |
| B3 `SIGWINCH` to a native line too (`tty_host.rs:348`) | INV-0331-6, INV-0387-2 | caught: `a_native_line_is_restored_but_never_told_a_size` |
| B4 `/dev/tty` tagged with group 0 (`jobs.rs:839`) | INV-0389-3 | caught (`--test tty`) |
| B5 a clone's line is the group it declares (`lib.rs:490`) | INV-0331-2 | caught: `a_clone_carries_its_parents_line_whatever_group_it_declares` |
| B6 the shell adds `COLUMNS` (`terminal.rs`) | INV-0333-3 | caught |
| B7 `TERM` without the terminal (`terminal.rs`) | INV-0333-2 | caught |
| B8 `TIOCSTI` not refused by `termios::ioctl_refusal` | INV-0338-1 | caught: `no_request_injects_input_and_only_the_window_sets_the_size` |
| B9 the look sends a new `PollReadable` each time | INV-0389-1 | caught: three `tests/poll.rs` tests |
| B10 `Flush` from a line not in front accepted | INV-0331-2 | caught |
| B11 `PollReadable` from a line not in front held | INV-0331-2 | caught |
| B12 a pasted newline sent without the question | ADR-0386 item 4 | caught |
| B13 `Join` across lines allowed (broker) | INV-0388-1 | caught |
| B14 `tcsetpgrp` of another line's group passed on (broker) | INV-0388-1 | caught |
| B15 a line's new group made in the session job (broker) | INV-0388-1 | caught |
| B16 `foreground_asked`'s `\|\|` as `&&` (`tty_host.rs:232`) | ADR-0388 item 4 | **survives** (finding 6) |
| B17 `leave` does nothing (`tty_host.rs:329`) | INV-0331-2 after a stop | **survives** (finding 6) |

B8 note: in the FFI file the default arm of `ioctl_tty` is `ENOTTY` for every unknown request, so `TIOCSTI` stays
`ENOTTY` even with B8; the host test guards `ioctl_refusal`, the FFI arm is not host-tested.

## Findings

### 1. must-fix: after 63 jobs in one `sh`, no new job can read the terminal (the 64-group cap, never pruned)

*Where:* `crates/zoraya-shell/src/tty_host.rs:210-217` (`group_created`: `l.groups.len() < 64`), `:356-369`
(`tcsetpgrp` refuses a group not in `groups`), `user/src/bin/zosh.rs:2245-2280` (`host_notices`), ADR-0388 item 3
("The broker answers that `tcsetpgrp` at once with success").

*What is wrong:* `Line::groups` only grows: nothing removes a group whose processes ended, and past 64 entries a new
group is silently not recorded. An interactive `sh` with job control makes a new process group for every command
(each is a job), so after 63 commands its `GroupMade` notices are dropped and each later `tcsetpgrp` is refused by
`Host::tcsetpgrp`, while the broker has already told `sh` it succeeded. The foreground stays on `sh`'s own group; the
new job's first read is `NotForeground` and it stops on `SIGTTIN`. `fg` asks `tcsetpgrp` again, is refused again, and
the job stops again: from that point the `sh` session cannot run any program that reads the terminal. The same
permanent state follows from one `GroupMade` dropped on a full channel (ADR-0388's own "Evidence against").

*Scenario:* reproduced above: after `while [ $i -lt 70 ]; do /bin/sleep 0; ...`, `/bin/wc -c` is
`Stopped (tty input)` and the typed `abc` goes to `sh`.

*Fix:* make the broker the source of the line's groups, which it already checks (`ask_foreground` refuses a group
whose `root` differs from the caller's): put the root in the notice (`ForegroundAsked { group, root }`, where `root`
is the group `zosh` started the line with, which `Host` knows from `start_line`) and accept it when `root` is the
front line's first group; then `groups` and `GroupMade` are not needed for the decision, and neither the cap nor a
dropped notice can lose a group. If `groups` stays, prune it (a `GroupEnded` notice when a group's last member goes)
and size it from the broker's limit. Tests: a `tty_host` test with 70 groups created and the 70th chosen; the probe's
loop followed by `wc -c` in `tests/posix/zosh.script`. Amend ADR-0388 (proposed) accordingly.

### 2. must-fix: Ctrl-C and Ctrl-\ in `sh` kill `sh`'s background jobs too

*Where:* `crates/zoraya-tty/src/lib.rs:663-692` (`signal` sends `Action::Signal { job }` with the foreground's
job), `crates/zoraya-shell/src/tty_host.rs:261` and `:367` (`set_foreground(group, l.job)`: the job is always the
line's), `user/src/bin/zosh.rs:2302` (`job_signal(job, kind, detail)`); ADR-0388 item 1 puts every group a line makes
inside the line's job, and `job_signal` reaches every process of the job's tree (INV-0004-3).

*What is wrong:* once `sh` has chosen a group with `tcsetpgrp`, Ctrl-C (and Ctrl-\) still goes to the whole line's
job tree: to the foreground job, to `sh` (which ignores it) and to every background job of `sh`, which, under job
control, does not ignore `SIGINT`. The part fixed this for Ctrl-Z (`signal_chosen_group`, `zosh.rs:3160`) but not for
the other two signal characters. The part's claim "`sh` has job control in a window" does not hold for its most
common use.

*Scenario:* reproduced above: `/bin/sleep 60 &`, then Ctrl-C on a foreground `/bin/sleep 30`, and `sh` reports
`[1] + Interrupt /bin/sleep 60`. A person who starts a build with `&` and presses Ctrl-C in `less` loses the build.

*Fix:* when the line has a chosen group that is not one of the line's own starting groups, send `SIGINT`/`SIGQUIT`
to that group through the broker (`broker.signal_group`, as for `SIGTSTP`), and keep `job_signal` of the line's job
only while no group is chosen (`set_foreground_line`). Test: the probe's first three lines plus `jobs` expecting
`Running` in `zosh.script`; a `tty_host`/`zosh` host test that an interrupt with a chosen group is not a line-job
signal.

### 3. should-fix: a window resize terminates the native elements of a line that has the terminal

*Where:* `crates/zoraya-shell/src/tty_host.rs:343-352` (`size`), `crates/zoraya-tty/src/lib.rs:695-705` (`resize`:
`RequestKind::Custom(28)` to the foreground job, the line's), `user/src/bin/zosh.rs:2971`; native elements are spawned
by `spawn_in_job` (`zosh.rs:2332`) with no disposition set, and `Encoder`'s default is `Disposition::Terminate` (0,
`crates/zoraya-abi/src/lib.rs:502-505`).

*What is wrong:* `SIGWINCH` is `job_signal` of the line's whole job tree, which holds the line's native elements too.
POSIX processes get `Ignore` for it from `posix_child` (default-ignore signals), native ones get the kernel's
`Terminate`. INV-0387-2 checks that a *native line* gets no `SIGWINCH`, not a native *element* of a POSIX line.

*Scenario (read from the code, not run):* in a window, `sh -c 'while :; do sleep 1; echo x; done' | cat` (`cat` is
native, `user/src/bin/cat.rs`); `sh` has the terminal on fd 0 and 2, so the line takes the keys; drag the window's
edge: `cat` is terminated by request 32, `sh` gets `EPIPE`. Any native program that uses custom request slot 28 for its
own purpose gets a phantom request.

*Fix:* send `SIGWINCH` through the broker to the line's POSIX groups (or the chosen group) instead of `job_signal` of
the line's job, or have `zosh` set `Ignore` for `Custom(28)` when it spawns a native element of a line that can have
the terminal. Test: a GUI or host test with a native element in a POSIX line and a `Size`.

### 4. should-fix: `ESC [ ? 2004 h` in any output of a line turns the newline confirmation off for the rest of the line

*Where:* `crates/zoraya-shell/src/tty_host.rs:448-454` (`relay` scans every byte of the line's output),
`:399-421` (`paste`: no question when `bracket.on()`), `Line::bracket` lives as long as the line; ADR-0386 item 3.

*What is wrong:* 2004 is a property of the program that reads, but it is read from all output of the line, kept after
the program that wrote it ended, and believed for a reader in canonical mode, which cannot interpret brackets. Inside
`sh`, `cat notes.txt` of a file containing `ESC [ ? 2004 h` (data, not a program) makes every later paste with a
newline go to `sh` without the question. This is the path finding 2 of the 10b review warned about.

*Scenario:* `sh` in a window; `cat downloaded.txt` (contains the sequence); the person copies a command from a page
whose selection carries a hidden newline and a second line; pastes into `sh`: no question, `sh` gets
`ESC[200~visible` (an error) and then runs the hidden second line on its own.

*Fix:* honour 2004 only while the discipline is not canonical (`on && !self.tty.discipline.canonical()`: a program
that set raw mode and 2004 is an editor that handles brackets; a canonical reader always gets the question), and reset
the line's `Bracket` on each change of the chosen group. Test: `relay` of the sequence, canonical attributes, a paste
with a newline gives `Question(Some(..))`. Record the rule in ADR-0386 (proposed).

### 5. should-fix: one program can exhaust the window's 256 terminal connections

*Where:* `crates/zoraya-tty/src/lib.rs:487-503` (`Clone`: no per-line limit, accepted from a line not in front),
`MAX_CONNECTIONS = 256` per `Terminal` (one per window, `Host::tty`); `user/src/posix_child.rs` ("A fd whose
connection cannot be made is its plain stream").

*What is wrong:* a program of any line, also a background one, can clone until the table is full. Every later
program of the window then starts without a terminal (no `isatty`, no `/dev/tty`, raw mode refused), and `sh`'s
forks in the front line fail to clone. Ending that program frees the table, but nothing tells the person why every
command lost its terminal.

*Scenario:* `t_flood &` (a C program that calls `open("/dev/tty")` in a loop and keeps the fds); `vi` started after it
has no terminal.

*Fix:* a per-line cap (for example 64 connections, four per program for 16 programs) checked in `Clone` and in
`Host::connect`, refused with `NO_SPACE`; a `zoraya-tty` test that line A's clones past the cap are refused while line
B still gets a connection.

### 6. should-fix: the mutation survivors of `tty_host.rs` are holes, two of them in the security rules; several in `zoraya-posix` are confirmed

*Where:* `crates/zoraya-shell/src/tty_host.rs`; `crates/zoraya-posix/src/jobs.rs`, `poll.rs`.

*What is wrong:* none of the 10 `tty_host.rs` survivors is equivalent:

- `foreground_asked` `||` to `&&` (B16 above, survives): no test gives a `ForegroundAsked` whose `by` is of a line not
  in front for a group that the front line also lists; the only guard left would be the front line's group list.
- `leave` to `()` (B17, survives): no test checks that after `stop_front` or `end_line` a request of that line (a
  stopped line continued with `bg`) is refused. Without `leave` the stopped line stays in front of the discipline and
  its `SetAttr` is accepted: INV-0331-2.
- `end_line` `!=` to `==` (the `retain`): ending one line would drop every other line's record; untested.
- `has_line` to `true`/`false`, `Host::front` to `None`: `zosh`'s calls are not covered.
- `Bracket::scan` `|=` to `&=` and `==` to `!=` at line 100: `ESC [ ? 1 ; 2004 h` (several parameters) is untested.
- `group_created` `<` to `<=`, `lines_of` `+` to `-`: the cap (finding 1) and the question's count.

In `zoraya-posix`, of the 64 the report lists, this review applied ten by hand. Caught now (the report's later
tests): `tty_get_attr` to `Ok([0; SIZE])`, `tty_set_attr`'s word index, `name_into` `<` to `<=`, `make_raw`'s `& !`.
**Still surviving:** `jobs.rs:513` (`read_tty` keeps a spent readiness), `:639` (bytes kept in `tty_stash` not seen
as ready), `:651` (a held `Read`'s `READABLE`), `:786` and `:787` (`tty_flush_conn` keeps `tty_stash` and
`tty_poll_ready`: `TCSAFLUSH` before a password prompt then still returns bytes typed before it), `poll.rs:236`. The
`|` to `^` survivors between disjoint mask bits in `make_raw` and `set_speed` are equivalent as the report says; the
two `|` to `^` on `now` in `poll_once` (`:140`, `:162`) are equivalent only while `probes` puts no input bit in `now`
for a terminal, which it does not today.

*Fix:* tests for each listed survivor; at least B16, B17 and the two `tty_flush_conn` ones before the merge. Add the
hand mutations above to a list in `tools/mutants/` (CLAUDE.md).

### 7. note: a refused `tcsetpgrp` reported as success (ADR-0388)

With finding 1 fixed, the refusal is left only for a line that is not in front (a shell in a background line), and its
visible effect is the `SIGTTIN` stop POSIX gives a background job anyway: acceptable as ADR-0388 records it. Without
finding 1's fix it is a silent permanent failure in the front line, which is why finding 1 is must-fix. The broker's
`asked` map (`crates/zoraya-broker/src/lib.rs`, per root) is never cleared when a line ends; pids are never reused
(`next_pid` only grows, `LAST_PID`), so this is a small leak, not a confusion.

### 8. note: no kill offer for a line with the terminal

Proposed ADR-0387 item 4 drops ADR-0158 rule 3's offer for such a line and names the tension; the architect decides.
The consequence is wider than its text says: a program that ignores `SIGINT`, `SIGQUIT` and `SIGTSTP` (or that has
chosen a group, so Ctrl-Z is only a `SIGTSTP`) cannot be stopped from its window, and in a session with one window
there is no "elsewhere" to type `kill` in. Worth one line in ADR-0387 ("closing the window is the only way") or a
key that no program sees (as ADR-0337's System key), for the architect.

### 9. note: dismissing a held paste with an arrow key sends the rest of the key to the program

`tty_host.rs:382-388` consumes one byte; `zosh` feeds keys one byte at a time (`zosh.rs:3072`), so Up drops the paste
with its `ESC` and gives `[A` to the program. Drop the whole key (the paste router knows key boundaries) or treat
`ESC` as "drop and swallow the sequence".

### 10. note: INV-0386-1 holds for the default control characters only

A program may set `VINTR`, `VEOF` and the rest to printable bytes, including `[`, `~` or a digit of the marks; then
pasted text or the shell's own marks are signal characters for it. Only its own line is affected, so no boundary is
crossed; reword INV-0386-1 ("...for the default control characters; a program's own `c_cc` applies to a paste as to
typed keys").

### 11. note: what stays open inside one line, and the `GetAttr` of other lines

ADR-0331 item 4 accepts that a background job of `sh` (same line, another group) can `SetAttr`, `Flush` and clone with
the chosen group, and `TOSTOP`/`SIGTTOU` stay deferred (ADR-0331, line 261): not a defect of this part. `GetAttr` from
a line not in front is answered (item 4), so a background program can see that echo is off; also accepted.

### 12. note: `select` and `poll` with two fds of one connection

`tty_input_ready` spends an answer per connection (`jobs.rs:643-645`, `:684`). Two pollfds (or two `select` bits) on
`dup`ed descriptors of one connection: the first is ready, the second sends a new request and is reported not ready.
Linux reports both. Cheap fix: look once per connection per call.

### 13. note: INV-0172-4 is neither done nor deferred in this part's report

The architect's decision of 2026-11-15 (ADR-0172, line 119) gives INV-0172-4's check to "part 10c or the next part
that touches `zosh`'s rooms". This part changes `zosh.rs` but none of its rooms code, so deferral is within the
decision's words, but `TASK-31-part-10c.md`'s Remains does not say so and INV-0172-4's Status still reads `not yet
applicable (stage 7)`, which is no longer true. Add a Remains line naming the part that will do it (and the Status
fixed to say what waits).

### 14. note: the order of the broker's notices and a child's first read

`ForegroundAsked` is written before the broker answers `tcsetpgrp`, and the child reads after that answer, so `zosh`
sees the notice first as long as its port delivers packets in the order their sources became readable. Nothing tests
it under a slow host; a `zosh.script` run with QEMU on efficiency cores (`taskpolicy -b`) of the `sh` section would show
a spurious `Stopped (tty input)` if the order ever breaks.

## Checked and sound

- **Line tags and refusals** (focus 1): a clone keeps its parent's line whatever group it declares (B5); `Read` from
  another line is `NotForeground`, `PollReadable`, `SetAttr`, `Flush` are `IO` (B1, B10, B11); with no line in front
  nobody reads (`Host::new` calls `set_front_line(None)`); a held read of a line that leaves is answered
  `NotForeground` (`background_reads`). `Foreground`, `Session`, `ClearForeground` need a control connection, and
  `zosh`'s `Host` never makes one, so a program cannot set the foreground through `zoraya-proto-tty`.
- **VMIN/VTIME**: `VTIME` is at most 25.5 s, every sum is `saturating_add`, `deadline` only counts connections that
  may read, one held `Read`, `PollReadable` and `Drain` per connection (`BUSY` otherwise): no overflow and no unbounded
  queue per connection.
- **Broker** (focus 2): a group made in a line is made inside the line's job; join and `tcsetpgrp` across lines or into
  the session are `EPERM` (B13 to B15); a line cannot cause a group in another line. `HostTerminal` is refused to a
  POSIX process and after the first time. Notices are written without waiting (`broker.rs`).
- **The question in the title is not forgeable**: `zoraya-term` ignores every OSC title (`tests_parser.rs:697`), so
  no program output reaches it.
- **The paste's text** goes through `paste::clean`, which drops `ESC`, C0, C1 (`U+009B` included) and the format
  characters of the 10b review; the bracket's end cannot be forged from pasted text (INV-0386-1 within finding 10).
- **Restore timing**: a line's end sends `Restore` after `finish_streams`, a stop after `stop_job`'s `drain_relays`,
  so no output written before the stop or end reaches `term` after the restore. The removed stop-gap
  (`zoraya_shell::reset`) has no remaining caller; the native line's `Restore` before the next prompt is kept.
- **libposix** (focus 4): `open("/dev/tty")` clones with the caller's group and `ENXIO` without an entry (B4);
  `ttyname`, `ttyname_r` (`ERANGE` below 9 bytes), `ctermid`, `tcsetattr`'s three actions, `tcflush`, `tcdrain`,
  `TIOCSWINSZ` `EPERM`, `TIOCSTI` `ENOTTY`, the deny list (B8, the export check). The readiness request is
  idempotent across deferred waits (B9), and the terminal arm of `poll` and `select` waits on the connection instead of
  "ready at once"; `tests/poll.rs` passes. Every new `unsafe` block in `ffi/procs.rs` has a `// SAFETY:` comment that
  states C's contract it relies on (`select`'s `timeval`, `cfmakeraw`, `with_termios`, `ctermid`, `ttyname_r` after
  its size check).
- **Invariant rows** (focus 5): INV-0331-1, -3, -6, INV-0333-2, -3, INV-0338-1, INV-0342-3 and INV-0389-3 fail when
  their behaviour is broken (B2 to B8). The "partly" rows say what is missing and that is accurate; INV-0331-2's
  in-QEMU password case and INV-0331-4's kill in a window are still to do.
- **`make test-tour-terminal`** is in `TEST_STEPS`, `plan-lib.sh`, the lanes and `ci-tests.txt` (TCG at scale 1 and
  1920x1200); `make check-ci-plan` and `check-test-lanes` pass. Not run here.

## Recommendation

Do not merge as is. Findings 1 and 2 break the part's own deliverable (`sh` with job control in a window) in
ordinary use and were reproduced; both are small changes in `tty_host.rs`/`zosh.rs` and ADR-0388 (proposed). Fix 1,
2 and 4, add the tests of finding 6 (B16, B17, `tty_flush_conn`), then one check of the changed lines; 3 and 5 can be
fixed in the same pass or recorded in `docs/backlog.md` with their scenarios before the merge.
