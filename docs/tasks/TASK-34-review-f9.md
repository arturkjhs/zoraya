# TASK-34 review: part F9, independent look (each room has its own loopback; proposed ADR-0442)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f9` at de151a6e, 2026-11-16, read against
`CLAUDE.md`, ADR-0441 (its `### Decision`: accepted with a per-room loopback), the proposed ADR-0442, ADR-0172, ADR-0262, ADR-0268,
ADR-0269, ADR-0434 and `docs/tasks/TASK-34-review-f7c.md` for its bug classes. The earlier read-only review's findings were not
taken as given; every claim below was derived again from the code. Code read around the diff: `Service::handle` (connect, listen,
narrow 6/7/9, duplicate), `listener_on`, `pick_port`, `listener_closed`; `Connection::decide`, `narrowed`, `Room`; `netd.rs`'s
`serve`, `loopback`, the listener close routes; `zoraya_net::loopback::deliver`; `init.rs`'s `root_connection` and its two callers;
the shell's `net_grant`, `posix_net`, `net_room`, the native spawn path, `enter_room`, `room_builtin` (`remove`, `--leave`);
`zoraya_shell::netgrant` (the rights it gives); `zoraya-panel`'s `terminal` and `launch` (no network slot); `zoraya-posix`'s
`GRANT_NET`; the room keeper's `launch` (`crates/zoraya-rooms/src/lib.rs`).

Run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("all checks passed").
- `cargo test -p zoraya-net -p zoraya-proto-net -p zoraya-shell`: exit 0; `zoraya-net` 130 passed, `zoraya-proto-net` 10,
  `zoraya-shell` 218, 0 failed.
- `make test-net-posix`: exit 0, `net-posix-auto` PASS 124 steps; the log has `serving A on 9100`, `serving A on 9102`,
  `serving B on 9100` (the same port in two rooms, no `EADDRINUSE`), `probe 9100 A x3 ok` (ra), `probe 9100 B x3 ok` and
  `probe 9102 none x70 ok` (rb), `probe 9100 none x3 ok` and `probe 9102 none x3 ok` (rc), `probe 9100 A x3 ok` and
  `probe 9102 A x3 ok` (ra again), `probe 9100 none x3 ok` (home).
- `make test-python-serve`: exit 0, `python-serve-auto` PASS 26 steps (the `PermissionError` of the line without `+listen` is the
  script's expected line).
- Four hand mutants of `service.rs`, each run with `cargo test -q -p zoraya-net` and reverted: the room dropped from
  `listener_on` (killed by three tests), the `ROOM` check replaced by `if false` (killed by
  `only_a_connection_with_the_room_right_names_a_room_and_it_is_inherited`), `pick_port` looking in `Room::HOME` (killed by
  `port_zero_is_picked_per_room_and_shows_nothing_of_other_rooms`), the per-listener peer port fixed at 0 (killed by two tests).

Not run: `make mutants`, `make mutants-host`, `make test-changed`, `make test-qemu`. Nothing here claims they pass.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

### 1. warning: "no leak of ... errors or counters across rooms" is not true of the machine-wide bounds

*Where:* `crates/zoraya-net/src/service.rs:597` (the listen charge), `:692` (`carve_with` of a `+listen` line), `:687`/`:742`
(the connection table full: `BUSY`); `docs/adr/0442-each-room-has-its-own-loopback.md:80-82` and `:99`; `docs/design.md` (the F9
paragraph says only "one room can use the machine's listener budget").

*What:* ADR-0442's driver 3 says "no error code, port number or counter may differ with what another room does", and its
Consequences say "no leak of ports, errors or counters across rooms". The loopback lookup does meet that (a cross-room connect is the
same `Action` as no listener, checked by the test and by reading `deliver`). The bounds do not: the 8 listener charges, the carve of
an account with one listener for every POSIX program on a `+listen` line, and `netd`'s connection table are one pool for every
room. Item 4 says "a connect never reveals it (only the program that listens sees its own account's answer)", but that answer is
itself the leak: whether it is `OVER_QUOTA` depends on how many `+listen` programs run in *other* rooms.

*Scenario:* room `a` runs eight `+listen` programs (they need not listen). In room `b`, `+listen 9000 tcptest --serve 9000 B`
gets no connection at all (the carve fails whole; `posix_net` turns the refusal into `None`), and a native command's `listen`
gets `OVER_QUOTA`. Room `b` learns that `a` holds at least eight; two cooperating rooms have a one-bit channel at will by starting
and ending `+listen` programs. No port number or queue is exposed, so ADR-0441's stated gate condition (ports, the 64-slot queue)
is met; the broader "nothing about other rooms" claim is not.

*Fix:* correct ADR-0442 (driver 3's claim scoped to the loopback lookup; item 4 and Consequences to say the bounds are a shared
pool whose exhaustion one room can observe and signal through) and add an open question for the architect: per-room bounds (with the
keeper's accounting) or accept the threshold channel for the gate. Same sentence in `docs/design.md`.

### 2. warning: the architect-queue line says the gate's loopback condition "is met" without saying what it rests on

*Where:* `docs/architect-queue.md:92`; `user/src/bin/zosh.rs:3748-3765` (`enter_room`'s held path).

*What:* the condition holds because the only holder of `ROOM` that names rooms is the console shell, which is trusted, and net
grants in an untrusted room are refused today (`refuse_net_in_untrusted_room`). ADR-0442 records the untrusted launcher's
requirement as an open question; the queue line, which is what the architect reads, does not. A second dependency is unstated
anywhere: a room's own shell (the desktop terminal, `zosh --room NAME`) builds its `Room` from whatever name is typed when the table
is unreadable (`name: name.into()` at `zosh.rs:3751`). It has no network slot today (`zoraya-panel`'s `terminal` gives none), so
nothing is open now; if it were ever given a root from `init` with `ALL`, `room enter a` typed in room `b`'s terminal would put its
programs in `a`'s loopback.

*Fix:* the queue line says "met for the rooms the console shell enters; open: the untrusted room's launcher must get from `init` a
connection with its room fixed and no `ROOM` (ADR-0269), and no launcher whose room name is not from the table may hold `ROOM`".
Add the second clause to ADR-0442's open questions. Also, "head 4a0bf633" is not the branch head (de151a6e); harmless.

### 3. warning: INV-0442-4 is "checked" by a test that covers only the POSIX path

*Where:* `user/src/bin/zosh.rs:1310` (native path), `docs/adr/invariants.md` (INV-0442-4's Status `checked: make test-net-posix`).

*What:* every probe and server in `net-posix.script` is a POSIX program (`posix_net`, `zosh.rs:1152`). The native path
(`narrow_room(root, rights, 0, &scope, &self.net_room())`) is checked by nothing: replacing `&self.net_room()` with `""` passes
every host test and every QEMU step, and the native command in room `b` would then be in `home`'s loopback, reaching any `home`
listener. ADR-0442's open questions say so; the invariants table says "checked" without qualification.

*Scenario:* a later edit of the native spawn path (e.g. back to `netclient::narrow`, which the diff replaced) loses the room for
native commands; no test fails.

*Fix:* a step in `net-posix.script` with a native command against a room's listener, e.g. in rb
`+net 127.0.0.1:9100 fetch http://127.0.0.1:9100/` against rb's `tcptest --serve 9100 B` (a bad HTTP answer, not a refusal) and
the same line in rc (connection refused); or write the Status as "checked: `make test-net-posix` (POSIX programs); native commands
by reading" until it exists.

### 4. note: a room removed without `--kill` and a new room of the same name share a loopback; so do they share a job

*Where:* ADR-0442's open question (`:126-127`); `crates/zoraya-rooms/src/lib.rs:750` (`launch` finds the room by `r.name == room`).

*What:* the report's admission is accurate, and it is not specific to `netd`: the room keeper also knows rooms only by name, so a
new room `a` launched while the removed `a` still runs joins the old room's job (same limits, killed together by a later
`room remove a --kill`). The loopback follows the system's existing notion of a room's identity; it does not widen it. Severity:
the user who removes and recreates a name gets the old programs in the new room in every respect, not only the loopback.

*Decision for the architect:* one fix for both, in `room new` / the keeper (refuse a name whose job still runs, or a generation
number the keeper hands out and the shell passes as part of the room name). Not a merge blocker for F9.

### 5. note: the 120 s idle limit of `tcptest --serve` can fail the room step on a slow run

*Where:* `tests/posix-net/tcptest.c:388`; `tests/shell/net-posix.script` (ra's `--serve 9102` gets no client from its start until
the final ra probe).

*What:* ra's listener on 9102 sits idle through all of rb's and rc's lines (73 + 6 connects, four room changes). Under HVF this is
seconds. Under TCG or a loaded host, if that stretch passes 120 s the server exits and the final `probe 9102 A` fails. That would be
a failure, not a vacuous pass: the final probe is also what proves ra's listener was alive during rb's 70 refused connects (the
server never restarts), which is the positive control the step needs, and it is there.

*Fix:* a longer idle limit (the job is killed by `room remove --kill` anyway), or poll with no timeout.

### 6. note: two test comments say more than the assertions

*Where:* `crates/zoraya-net/src/service_tests.rs:1802`, `:1946`.

*What:* ":1802 A room that listens on nothing else still sees nothing of a's other port" asserts `BUSY` for `server_b` on 8000,
which is b's *own* listener (added at the line before): it checks BUSY within room b, not anything about a. ":1946 A name over the
bound is not encoded at all" has no assertion on a 25-byte name; `decode::<NarrowRoom>` refusing it (`Action::Close`) and
`netclient::narrow_room` refusing it (`INVALID`) are untested.

*Fix:* reword :1802; add a raw 25-byte `room` field test that expects `Action::Close`.

### 7. note: the `kernel_work_is_bounded` failure in this worktree's logs is not `contiguous create: NoMemory`

*Where:* `target/test-logs/auto-smp4-2g.log` (22:16 today, this worktree).

*What:* the failing run's last `t_bound` lines are "map-loop: a kill ended a call between steps in round 2" and
"run 3 went over the time limit", then `kernel_work_is_bounded ... left: ExitRecord { code: 3 ...` (t_bound's exit 3 after three runs
over the time bound, with several "over the limit 2000000: the run is repeated" lines): the timing flake of a loaded host (another
agent's QEMU was running), not the contiguous-create race of `docs/backlog.md`. `auto-default` and `net-auto` of the same session pass
`kernel_work_is_bounded`. F9 changes no file under `kernel/` and not `t_bound.rs`, so either failure is unrelated to F9; the claim
should name the panic the log shows. Not re-run here.

## Checked and found sound

- **Who holds `ROOM`.** `init`'s control connection (`Connection::root`, `ALL`) and the root `init` makes for the console shell
  (`root_connection`, `init.rs:432`, called only at `:814` and on a network restart at `:978`). No other launcher gets a network
  root: the panel's `terminal` and `launch` have no network slot, the broker gets none. Every program's connection is a narrow of the
  shell's root with rights from `netgrant` (`REACH`, `LISTEN`, `STATUS` only, `netgrant.rs:281`, `:293`). `Narrow` refuses a right
  the caller lacks (`connection.rs:205-209`), so no connection without `ROOM` can make one with it (tested); `Duplicate` clones the
  connection (`service.rs:729`), so `GRANT_NET` children keep the room; `narrowed` keeps it (`connection.rs:219`, tested in `tests.rs`).
  `t_netd` holds `ALL` as a test program, as ADR-0442 says.
- **Forging a room.** Only the shell names rooms, from `self.room`, which comes from `/config/rooms` (`is_room_name`: lower-case
  ASCII, digits, `-`, 1 to 24 bytes, never `home`), so no name equals `home`'s empty key, contains NUL or non-ASCII, or exceeds
  24; `Room` compares length and bytes (zero-filled), so `a` and `a\0` differ; the proto bound and the settings bound are tied by a
  `const` assertion (`service.rs:1061`). An empty name keeps the caller's room, so a program's own `NarrowRoom` with no name cannot
  change it, and with a name is `NOT_GRANTED` (tested for `a`, `b`, `home`).
- **Nested rooms, `room enter`, leaving.** The shell has one current room (`enter_room` replaces it, `--leave` and `room home` clear
  it, removing the current room clears it); there are no subrooms. A background job keeps the room it was started in.
- **The loopback.** `listener_on` is keyed by (room, port): `EADDRINUSE` per room, the same port in two rooms, port 0 searched per
  room with no shared cursor; a cross-room connect returns the very `Action` of no listener (same `peer_port` 49152, same reply;
  `deliver` sends `Failed { REFUSED }` and closes both ends) and never reaches the other room's channel, so it cannot fill its queue;
  the peer counter is per listener. The name path stays `UNREACHABLE` (INV-0441-3's test passes). Every listener close route
  (channel peer closed: process exit, crash, kill of the room's job; or `netd` unable to serve it) goes through `listener_closed`,
  and `netd` keeps no per-room state beyond listener entries, so nothing of a room outlives its listeners.
- **ADRs and docs.** No status or "Architect's decision" edited; ADR-0441's change is a rule-6 open-question note; INV-0441-2's row is
  unchanged, as `adr-check` requires; the four INV-0442 rows match the ADR, and the Total, recomputed from the per-ADR counts and
  from the rows, is 871. No `unsafe`, no host paths, no new `allow`.

## Verdict

Merge after finding 2's queue line is corrected (a documentation fix); findings 1 and 3 may be fixed in this part or recorded as the
ADR's open questions and the invariant's Status before merge. The code of the room key is correct as far as this review could test
and read it.
