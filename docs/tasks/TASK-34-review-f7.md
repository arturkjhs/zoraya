# TASK-34 review: part F7 (socket files in the line's `/tmp`; TCP listening as a design only)

Independent review by the `opus-review` subagent of `git diff origin/main..HEAD` on `work/f7` at b6cab66b (four commits),
2026-11-16, read against `CLAUDE.md`, `docs/tasks/TASK-34-foundations.md`, ADR-0412 item 5 and INV-0412-3, ADR-0430, the
proposed ADR-0439 and ADR-0440, and the bug classes of `TASK-34-review-f2.md` and `-f4.md`. Code read around the diff:
`Service::ready`, `send`, `flush_held`, `open_as`, `rename`, `publish`, `remove`, `close_connection`
(`crates/zoraya-fs-service/src/lib.rs`), `zoraya_protocol::decode` (exact handle count), the kernel's `channel_write`
(`kernel/src/syscall.rs`: rights containment, `may_travel`, the writer's payment and `OverQuota` from `charge.rs`),
`Volume::open_mut` (exclusive create before a link is followed) and `next_id` (ids never reused), the broker's
`start_tmpd` (the only `fsd --memory`), `pass_to_child`, the `SCM_RIGHTS` plan and `inherit_shaped` (`unix_io.rs`,
`jobs.rs`), `install`/`release` (`process.rs`), `Record::decode` (`unix.rs`), `errno::FILE`.

Tests run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `cargo test -q -p zoraya-fs-service --test socket`: exit 0, 16 passed.
- `cargo test -q -p zoraya-posix --test socketfile`: exit 0, 16 passed.

Not run (not allowed in this review): QEMU (`t_unix file`, `make test-libposix`), `make check`, the hand mutants of
`tools/mutants/sockets-f7.toml`, `make mutants-host`. Nothing in this review claims they pass.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

1. **warning**: any program with `WRITE` on a socket file can become its listener; `listen` is not tied to the
   connection `bind` made. `crates/zoraya-fs-service/src/lib.rs:1683-1735` (`socket_listen`) checks only `WRITE`, the
   type, that the connection is on a socket node and that the node has no listener. ADR-0439 item 3 says "the connection
   is the one `bind` made and the socket keeps", and `docs/design.md` says "held for the connection `bind` made"; neither
   is enforced. Scenario A (race): server P does `bind("/tmp/s")`; program Q of the same line opens `/tmp/s` with
   `WRITE` (the raw protocol; libposix's `open` is `ENXIO` but the service accepts the open) and sends `socket_listen`
   before P's `listen`. P's `listen` gets `EXISTS` (`EADDRINUSE`), and every client's `connect("/tmp/s")` lands in Q's
   channel. Scenario B (takeover): P's listener goes (P crashes); the node stays, inert (item 2); Q listens on it with no
   `unlink` and no `bind`, and clients that retry reach Q. On Linux neither is possible: only the bound socket can listen
   on its address, and a dead socket file only refuses. The right that lets a program connect is exactly the right that
   lets it listen. The test `the_table_of_nodes_and_of_listeners_is_bounded` (`tests/socket.rs:747-756`) itself listens
   through fresh `connection()`s, not the binding ones, so the gap is pinned as behaviour. *Fix:* record the creating
   connection in `SocketNode` when `open_as` inserts it (`owner: Option<usize>`, cleared in `close_connection`), and
   refuse `socket_listen` from any other connection (`ACCESS_DENIED`); a node whose owner has gone is never listened on
   again (only `unlink` and a new `bind` serve the name). Add a test that a second `WRITE` connection cannot listen
   before or after the owner, and a mutant that drops the check.

2. **warning**: a quota refusal of the service's write kills a live listener for good, and the listening program then
   spins. `user/src/bin/fsd.rs:155` maps every status but `WouldBlock`, `NoMemory` and the three of `Refused` to
   `Outgoing::Closed`, including `Status::OverQuota`, which `channel_write` returns when the writer's job (the `/tmp`
   service's, `tmp_job`, a sibling of the session's job under the broker's) cannot pay `message_cost` (ADR-0261 point 7:
   the writer pays while the message is queued). `socket_connect` (`lib.rs:1782-1791`) then calls
   `socket_listener_gone`: the service closes its end and clears `listening`. The listening program's next `accept`
   reads `PeerClosed` and returns `ECONNABORTED` (`unix_file.rs:324`), poll reports the listener readable forever
   (`socket.rs`, `READABLE | PEER_CLOSED`), and a second `listen` returns `Ok` without asking the service
   (`unix_file.rs:149`, `u.listening` is still true), so nothing recovers. On Linux `ECONNABORTED` from `accept` is
   transient; `socketserver`'s loop and most C servers retry it, so a server whose listener died this way busy-loops at
   full CPU. Scenario: the session runs short of memory for a moment (a build), one `connect` arrives then, the server
   never serves again. Who pays makes it worse: the queued connections are charged to the service, not to the connector
   (ADR-0439's "Effect of the vision" says the full queue "costs the writer the pair it made"; the pair's objects are the
   connector's, but each queued message is the service's), so one program can bind and listen 64 nodes and queue 64
   connections on each, 4096 messages paid by `fsd`. *Fix:* map `Status::OverQuota` to `Outgoing::NoMemory` in
   `fsd`'s `write` (the connect is then `EAGAIN`, the listener lives; replies keep their current handling); make a
   listener whose service end went `EINVAL` or `ECONNABORTED` once and then clear `listening` so a `listen` can ask
   again, or say in ADR-0439 item 5 that it is permanent; state who pays the queue in ADR-0439. Add a service test with
   the fake transport answering `NoMemory` (the listener lives) and record the status classification of `fsd.rs` (not
   host tested; finding 7).

3. **warning**: ADR-0439 item 7 and `docs/design.md:2215` say a listening (or bound) socket is "not inherited by
   `spawn` (`EINVAL`)". The code gives `EBADF` both ways: an unlisted Unix socket is `EBADF` in `inherit_shaped`
   (`jobs.rs:1120-1122`), a listed one goes to `pass_to_child`, which refuses a socket that is not `Connected` with
   `EBADF` (`unix_io.rs:416-418`; a listener stays `State::Fresh`). The F7 notes of the report say the same (`EBADF`, "found
   by the in-QEMU test"), so the documents contradict each other. No host test spawns with a bound or listening socket.
   *Fix:* write `EBADF` in item 7 and the design bullet (or change the code to `EINVAL` and say why), and add a
   `tests/socketfile.rs` case for `posix_spawn` with a listener listed in the file actions and inherited.

4. **warning**: ADR-0439's "Effect of the vision" says a program with `WRITE` on the file can fill the 64-message
   queue "and nothing else" (`0439-socket-files-as-built.md:110`). Also true from the code: it can become the
   listener (finding 1); it can take every one of the service's 64 listener slots (`MAX_LISTENERS` counts
   service-wide, `lib.rs:1708-1715`), after which every other program's `listen` is `ENFILE`; it can fill the 256
   nodes (`bind` `ENOMEM` for the line until someone unlinks), which the backlog records but the ADR's sentence does
   not; and it makes the service pay for the queued messages (finding 2). *Fix:* replace the sentence with the list,
   and either bound listeners per account as ADR-0440 does for `netd` (4 per account) or record the shared 64 next to
   the shared 256 in the backlog and the ADR's negative consequences.

5. **warning**: the unmet "done when" and the request to split F7 are not in the architect's queue. The report,
   `docs/backlog.md`, ADR-0440 and the design bullet say honestly and consistently that TCP `bind`/`listen`/`accept`
   are `EOPNOTSUPP`, so `python3 -m http.server` does not serve a file to `curl`, and why (no `listen` in `netd`'s
   protocol, no loopback interface, no network step run). That statement is complete. But ADR-0440 asks that the TCP
   half become "its own plan part", which changes the plan, and ADR-0439 has an open question "for the architect" (the
   own-directory place); `docs/architect-queue.md` has neither (F6 put its question there). F7 cannot be called done
   by the orchestrator without that decision. One more precision: ADR-0440 driver 5 says the agent "could not run"
   `make test-net`; `CLAUDE.md` has an agent run only `make check` and `make test-changed` and leaves the step to CI,
   so the honest reason is the session's scope, as the report says ("judged out of reach"). *Fix:* add both questions
   to `docs/architect-queue.md`; align driver 5 with the report.

6. **note**: errno differences from Linux. (a) `bind`, then `unlink` of the name, then `listen`: the service removed the
   node with no listener (`socket_unnamed`), `socket_node_of` finds nothing, the status is `INVALID_ARGUMENT`, and
   `socket_errno` (`unix_file.rs:37`) turns it into `EPROTOTYPE`, which `listen` never returns on Linux (there it
   succeeds, with an unreachable name). Either keep the node until its binder's connection closes, or answer `EINVAL`
   for `listen`. (b) `listen` of an unbound `SOCK_DGRAM` socket is `EINVAL` (`unix_file.rs:152-158`: the bound check
   comes before the type check); Linux gives `EOPNOTSUPP`. The test at `tests/socketfile.rs:323-325` says "A datagram
   socket neither binds nor listens" and pins `EINVAL`. (c) A far end the kernel refuses (`Outgoing::Refused`) is
   `INVALID_ARGUMENT`, the same status as a type mismatch, so the connector sees `EPROTOTYPE`; only a hostile or buggy
   connector reaches it, so it is acceptable, but say it in item 4.

7. **note**: what the tests and documents claim beyond what they check. (a) INV-0439-3's Check (ADR-0439:133 and
   `invariants.md:746`) cites "the property over every length and byte pattern in `tests/socketfile.rs`"; the loop at
   `tests/socketfile.rs:533` tries 64 lengths times 256 arithmetic sequences, of which almost none decode; it shows no
   panic and nothing else. Say "a sweep of lengths 0 to 63", or decode every 24-byte record from mutated valid ones
   (each flag bit flipped, each field at its edges). (b) `tests/socket.rs:436`: "A stream connect forwards three."
   precedes an assertion that the stream connect is refused (`INVALID_ARGUMENT` into a packet listener); the next test
   checks the three. (c) The `Refused` classification lives in `fsd.rs:154`, which no host test runs: the service test
   uses the fake's `refuse` set, and `t_unix file` never makes the kernel refuse a far end, so "test and mutant" in the
   report covers the service's handling, not which kernel statuses are `Refused`. (d) The library tests never use a
   read-only `/tmp` entry for `connect` (only the QEMU case's `/bin`, which holds no socket); the "read-only `/tmp` cannot
   connect" sentence rests on the resolver's generic `Access::Write` check and the service's `ACCESS_DENIED` test.

8. **note**: report bookkeeping. `TASK-34-report.md` F7 notes say "23 hand mutants ... 23 caught", but
   `tools/mutants/sockets-f7.toml` has 25 (step 4 added `F7-dead-listener-keeps-its-owner` and
   `F7-publish-over-keeps-the-entry`); they end "`make mutants-host CRATE=zoraya-posix --in-diff` below" with nothing
   below; line 8 still says F7 has not started and line 28 still lists "no `bind`/`listen`/`accept` (F7)" as a limit.

9. **note**: `accept` trusts the kinds of the handles it is given. `decode_connect` checks the record and the count;
   `desc_from` then installs whatever objects came (a channel end where a stream end should be, a memory object). A
   hostile connector can only hand the server a socket that fails its own reads (its own objects), so this is not a
   rights problem, but it is a byte-trusting shape INV-0439-3 says is excluded. The service could refuse a
   `socket_connect` whose handles are not the expected kinds (`transport.kind`), cheaply, before forwarding.

## What was checked and found sound

- Rights: the service forwards each handle with its type's full rights (`rights_of`), and the kernel refuses an entry
  whose rights are not contained in the held handle's (`syscall.rs`, `AccessDenied`) and a channel end that holds
  handles (`BadState`); nothing is widened, and the refusal is an `Outgoing::Refused` that keeps the listener (tested).
  `connect` and `listen` need `WRITE` on the node connection; a directory connection or a plain file is `NOT_FOUND` /
  `INVALID_ARGUMENT`; `S_IFSOCK` bits are kept only by a service that `enable_sockets()`, which only `fsd --memory`
  (the broker's `/tmp`) does, so INV-0412-3 holds on the service side too, whatever a program sends. A link is never
  followed by `bind` (the volume refuses an exclusive create over any existing name before looking at links; the
  equivalent-mutant note is right), and the entry is checked again after `open_resolved`.
- Handles on refusal paths, service: every early return of `socket_listen` and `socket_connect` leaves the request's
  handles in the vector `ready` drains and closes; the handle count is exact (`decode`); a failed `channel_write` has
  no effect (INV-0050-1), so `Full`/`NoMemory`/`Refused`/`Closed` leave the handles to `ready`; `Sent` clears them. The
  listener end is charged once and released once (`socket_listener_gone` on connection close and on `Closed`, with the
  owner's `listening` cleared). Library: `unix_bind`, `unix_listen`, `unix_connect` close their node connection, the
  listener's own end and the near ends on every refusal; `call` moves the far ends whatever happens; `release` closes
  `node`; `install` releases the description when the table is full; `accept` checks room before reading.
- State keyed by object id: volume ids come from `next_id` and are never reused; every way a name leaves the volume
  (`unlink`/`rmdir`, `rename` replace, `publish` replace) updates the table; a rename onto itself is handled; there are
  no hard links. A `bind`/`unlink` loop leaves the table as it was (tested).
- `fork` with a bound or listening socket is `ENOTSUP`; `SCM_RIGHTS` of a listener is `EINVAL` (tested); `accept4`'s
  `SOCK_NONBLOCK`/`SOCK_CLOEXEC` apply to the new socket alone and the wait follows the listener's `O_NONBLOCK`
  (tested); a listener is readable when a connection waits and never writable.
- The C boundary: `bind` copies at most 128 bytes after a null/length check; `accept_with` checks the flags, the
  null pairing, and closes the new fd if writing the address faults; `put_unix_addr`'s copy is bounded by the
  caller's `addrlen` and the address's length. The `// SAFETY:` comments state C's contract, which is the invariant.
- `invariants.md`: 859 rows, Total 859; INV-0440-1/-2 are `not yet applicable` (true); INV-0412-3's narrower place is
  stated in its row.
- Mutants marked equivalent: `!r.shared` (`Record::decode` requires `has_anc != shared`), `NO_FOLLOW` (exclusive create
  before link), the library's `S_IFSOCK` check in `connect` (the service answers `NOT_FOUND` or `UNSUPPORTED`, both
  `ECONNREFUSED`): all three are equivalent as stated.
- No wall-time assertion in the host tests (the fake clock is 0); `t_unix file`'s one `poll` timeout waits on a
  connection already queued.

## Verdict

**Fix findings 1 to 3 before merge** (1 and 2 are small service and `fsd` changes with a test each; 3 is a document
and a test), and record 4 and 5 (ADR text, architect's queue). 6 to 9 may go to the backlog. The plan's "done when"
(`python3 -m http.server` served to `curl`) is **not met**; the part's documents say so honestly, and the decision to
split the TCP half into its own plan part is the architect's.
