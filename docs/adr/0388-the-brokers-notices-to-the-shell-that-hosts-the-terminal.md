# ADR-0388: The broker's notices to the shell that hosts the terminal

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0052, ADR-0104, ADR-0105, ADR-0153, ADR-0331, ADR-0387
- **Evidence:** CP-07, stage-10/survey.md#the-terminal-as-built

## Context

ADR-0331 item 6 says the broker beside `zosh` "gains a control channel to it and passes the request on", and that the
shell accepts a `tcsetpgrp` only for a group the broker says was created inside the line in front. It does not say
which way the channel is made, whether the broker waits for the shell's answer, or how the broker knows a line.

**What is known.**

- `zosh` calls the broker synchronously (`posix_child::Broker::call` waits for its reply), and the broker's call to
  `ttyd` (`control_call`) waits too: a broker that waited for `zosh` while `zosh` waited for the broker would stop
  both.
- The broker beside `zosh` is started by `init` and by the panel (`user/src/beside.rs`), which hand `zosh` a client
  connection at slot 7; a new slot would change both launchers.
- A process that `zosh` starts is in a `Group(job)` group of its own outside the session; a new group made by one of
  its processes was made in the session job (`Form::New`), outside the line's job, so the line's Ctrl-C and its end
  did not reach it (`crates/zoraya-broker/src/lib.rs`).
- Moving a process between jobs is never done (CP-07).
- What the terminal looks like to a POSIX shell is in `stage-10/survey.md#the-terminal-as-built`.

## Decision drivers

1. No call in either direction waits on the other.
2. No new slot and no change to the launchers.
3. A group made inside a line stays inside the line's job.
4. A process of one line can neither take another line's group into the foreground nor join it.

## Considered options

### Option A: the client asks for a notice channel; notices without replies

- **Description:**
  1. A group made by a process outside the session is made inside its group's job and belongs to the same line (its
     root: the `Group(job)` group `zosh` started).
  2. `Join` outside the session is allowed only within the caller's line.
  3. `zosh` sends `HostTerminal` (ordinal 16 of `zoraya-proto-posix`) once on its client connection and receives the
     client end of a channel. On it the broker sends, with no reply, `GroupMade { group, by }` (17) when a process
     of group `by` makes `group`, and `ForegroundAsked { group, by, root }` (18) when a process of group `by`, of
     the line whose root is `root`, asks `tcsetpgrp(group)` for a group of its own line. The client tells the broker
     which lines are in the background (`Background { group, back }`, 19, a request with a reply, from the client
     only: for a line started with `&` before its program runs, on a stop, and on `fg`); a `tcsetpgrp` of such a line
     is `EPERM` at once and no notice goes. Otherwise the broker answers that `tcsetpgrp` at once with success,
     remembers the group for the line, and answers `tcgetpgrp` from it (the caller's own group until one is
     asked). A `tcsetpgrp` for a group of another line, or of the session, is `EPERM`.
  4. `zosh` knows each line's roots (the groups it started the line's POSIX programs in) and gives `ForegroundAsked`
     to `Host::foreground_asked`, which refuses it unless the line with that root is in front; that `group` is of
     the line the broker has checked. `GroupMade` decides nothing (it is informative): the review of 10c (finding 1)
     found that a list of the groups made, capped at 64 and never pruned, ended an interactive `sh`'s job control
     after 63 commands, and that one dropped notice did the same for good.
  5. A notice the channel cannot take is dropped; the broker does not wait. A dropped `ForegroundAsked` leaves that
     job stopped on its first read; `sh`'s `fg` asks again.
  6. A signal to a group that has groups made inside its job (a POSIX shell with jobs) goes to its members one by
     one, not to its job, which holds those groups too; once a group is chosen, the shell sends the signal
     characters and `SIGWINCH` to it through the broker (`kill(-group)`), not to the line's job (the review of 10c,
     findings 2 and 3).
- **Evidence for:** Drivers 1 to 4. The client end travels on the connection `zosh` already has.
- **Evidence against:** The caller hears success even when `zosh` refuses (a background line's shell): its
  `tcgetpgrp` then names a group that cannot read, and its reads stop it on `SIGTTIN`, which is what POSIX gives a
  background job anyway. A `ForegroundAsked` dropped on a full channel stops that one job until it is asked again.
- **Cost:** *(measured)* about 120 lines in `zoraya-broker` and its test, 25 in the broker's binary, 30 in `zosh`,
  three messages.
- **Precedent:** ADR-0105's control connection, turned around: the broker tells, the terminal decides.

### Option B: a control channel at a new slot, with replies

- **Description:** The launchers pass `zosh` a second channel to the broker; `tcsetpgrp` waits for `zosh`'s answer.
- **Evidence for:** The caller hears the shell's refusal.
- **Evidence against:** The two synchronous calls can wait on each other (driver 1); two launchers change (driver 2).
- **Cost:** *(estimate)* Option A's, plus a deferred reply in the broker and the launchers' slots.
- **Precedent:** `ttyd`'s control channel.

## Recommendation

**Recommendation (not a decision): Option A.**

**Tensions with accepted ADRs.** ADR-0104 item 10 lets any session process make a session group the foreground; for
a client's processes item 3 narrows it to their own line, as ADR-0331 item 6 asks. ADR-0153 rule 10 gave the broker
beside `zosh` no channel to it; this adds one, asked for by the client.

### Effect of the vision

Target 2: a line's processes cannot reach another line's groups through the broker; the shell, not the broker,
decides what reads the keys. Target 3: nothing here is per-machine.

## Consequences

- **Positive.** A POSIX shell's job control can work in a window once `libposix` opens `/dev/tty`; its jobs stay
  inside its line.
- **Negative.** A `tcsetpgrp` the client refuses after the broker said yes (the line changed in front between the two,
  a window of one message) is reported as done.
- **Follow-up.** The in-QEMU test with `sh`'s own job control (needs `open("/dev/tty")`, REMAINS 3 of part 10c).

## Invariants

- **INV-0388-1.** A group made by a process of a client's line is inside that line's job; a process of one line
  cannot join, or ask the foreground for, a group of another line or of the session; a signal to a group reaches no
  group made inside it. *Check:* `cargo test -p zoraya-broker`
  (`a_client_that_hosts_the_terminal_hears_of_groups_and_tcsetpgrp_of_its_lines`,
  `a_signal_to_a_group_with_groups_made_inside_it_reaches_its_members_only`).

## Open questions

- Whether the caller should hear the shell's refusal (Option B without the wait: a deferred reply). Decided by the
  architect, 2026-11-15: a refused `tcsetpgrp` returns `EPERM`, not success. Recorded by an agent: built as the
  `Background` request above (the broker decides from the line's root; the broker never waits for the client, so
  no call can wait on another).

## Revisit triggers

- A POSIX shell is found that misbehaves when its `tcsetpgrp` succeeds but its group cannot read.

## Architect's decision
(Left empty for the architect.)
