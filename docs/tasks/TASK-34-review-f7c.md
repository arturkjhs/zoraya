# TASK-34 review: part F7b, independent look (TCP `bind`, `listen`, `accept` over `netd`; `python3 -m http.server` serves `curl`)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f7b` at af113862, 2026-11-16, read against
`CLAUDE.md`, `docs/tasks/TASK-34-foundations.md`, plan row F7, ADR-0220, ADR-0223 item 4, ADR-0224, ADR-0225 item 5, ADR-0262,
ADR-0434, the proposed ADR-0439, ADR-0440 and ADR-0441, and the earlier reviews of TASK-34 (f2 to f7b) for their bug classes.
Code read around the diff: `Service::handle` (connect, listen, narrow_account, duplicate), `Accounts::carve_with`, `free`, `dead`,
`release`; `Connection::decide`; `zoraya_abi::net` (`allows_address`, `names_address`, `is_public`, `NOT_PUBLIC`, `within`);
`conn.rs`'s `next_candidate`; `netd.rs`'s `serve`, `attach_listener`, `drop_listener`, `loopback`; libposix `net.rs` (`net_call_with`,
`connect`, `bind`, `listen`, `accept`, `getsockname`), `socket.rs` (`wait_source`), `poll.rs`'s socket probes, `jobs.rs`'s fork guard,
`process.rs`'s `release` and `install`; the shell's `syntax.rs`, `eval.rs`, `netgrant.rs`; `tests/posix-net/tcptest.c`, the two
scripts, `tests/python/regr-*.txt` and CPython 3.15's `test_asyncio/test_base_events.py` and `socketserver.py`.

Run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("all checks passed").
- `cargo test -p zoraya-net -p zoraya-posix -p zoraya-proto-net -p zoraya-shell`: exit 0; `zoraya-net` 122 passed,
  `tests/tcplisten.rs` 12, `zoraya-shell` 217, every suite 0 failed.
- `make test-net-posix`: exit 0, `net-posix-auto` PASS 65 steps (the log has the five `tcptest: ... ok` lines and `all ok`; the two
  negative lines print `FAIL line 113: l >= 0 (errno 13)` ... and `FAIL line 125: c >= 0 (errno 13)` ...).
- `make test-python-serve`: exit 0, `python-serve-auto` PASS 26 steps (`Serving HTTP on 127.0.0.1 port 8000`, the file twice,
  `PermissionError: [Errno 13]` for the line with no `+listen`).
- One hand mutant, reverted afterwards: `peer_port: (EPHEMERAL_FIRST * self.next_peer)` in `service.rs` is killed by
  `a_loopback_connect_needs_a_rule_that_names_the_address_and_finds_the_listener` (the report's "killed but not rerun" holds).

Not run: `make mutants`, `make mutants-host`, `test-python-regr`, `test-python`. Nothing here claims they pass.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

1. **warning**: the two negative lines of `tests/shell/net-posix.script:79-87` do not check what their comment says (INV-0440-1 and
   INV-0440-2 in QEMU). They wait for `tcptest: FAIL line`, any line. Scenario: a regression makes `netd` grant `listen` with no
   listen rule. `+net 127.0.0.1 tcptest` then passes `test_basics`, and `test_refusals` prints `FAIL line 155` (the 9100 listen
   succeeded), which matches the expected line, so the step stays green while the grant it claims to check is gone. The same
   holds for the reach line. The run shows the lines it really prints. Fix: wait for `tcptest: FAIL line 113: l >= 0 (errno 13)`
   and `tcptest: FAIL line 125: c >= 0 (errno 13)`. Better: give tcptest a mode argument (`tcptest nolisten`, `tcptest noreach`)
   that checks the `EACCES` itself and prints `tcptest: refused ok`, so that the harness never has to expect a `FAIL` line. As it
   stands, `log-triage` also reports these deliberate `FAIL` lines first whenever the step fails for another reason.

2. **warning**: `poll` says a listening socket is writable. `crates/zoraya-posix/src/socket.rs:692-698` returns `None` for
   `wait_source(true)` on a listener, and `crates/zoraya-posix/src/poll.rs:269-272` turns `None` into "ready now"
   (`now |= outs`). So `poll(l, POLLOUT)` returns `POLLOUT` at once, and a `select` with the listener in `writefds` spins. Linux
   never reports a listener writable. The new test `a_listener_is_waited_on_by_its_channel_and_is_never_writable`
   (`socket.rs:1018-1031`) asserts the exact value that makes poll report it writable, so its name claims the opposite of the
   behaviour. Fix: give a listener a "never" source for writing (as a Fresh Unix listener or the tty case does), or have `poll`
   test `listening` before calling `wait_source(true)`. Add a `tcplisten` case: `poll(l, POLLOUT, 0) == 0`.

3. **warning**: `bind` to port 0 followed by `getsockname` before `listen` gives port 0. Linux gives the assigned port.
   `crates/zoraya-posix/src/net.rs:551-566` records the port 0 and `listen` fills it in only afterwards. CPython's
   `socketserver.TCPServer.server_bind` reads `getsockname()` right after `bind` and before `server_activate` (`listen`), and
   `HTTPServer.server_bind` takes `server_port` from it. So `python3 -m http.server 0`, any `socketserver` and `xmlrpc` server
   on port 0, and `test.support.socket_helper.find_unused_port`/`bind_port` all see port 0: they print "port 0", and their
   clients connect to port 0 and are refused. ADR-0441 item 6 and the design bullet do not list this. Fix: reserve the port at
   `bind` (a `netd` request, or `listen` done at bind time with the queue opened later), or record it as a limitation in
   ADR-0441 item 7, `docs/design.md` and the report.

4. **warning**: INV-0441-1's "a refused one ... with every handle closed" is marked `checked` but nothing checks it. The closing
   is done in `user/src/bin/netd.rs:420-428` (`loopback`'s else branch) and `:569-578` (bad handle types), which no host test
   reaches. `tcptest` cannot see `netd`'s handle table: its 65th connect is refused once and only `ECONNREFUSED` is read. The
   code is right as read. But a later change that drops one of those `handle_close` calls leaks two handles in `netd` per
   refused connect. Any program with `+net 127.0.0.1` could then loop over `connect` to a port nobody listens on and exhaust
   `netd`'s handles, the machine's network service. Fix: move the refusal decision and the list of handles to close into a
   host-tested function, or add to `tcptest` a loop of several thousand refused connects followed by one connect that must
   succeed. Either way, say in the invariants row which half is checked.

5. **warning (architect's decision; recorded, not hidden)**: the loopback belongs to the machine. Exactly what an attacking
   program can do with a reach rule that names 127.0.0.1 (`+net 127.0.0.1`, an `ADDRESS/BITS` prefix over it, `any`, or no word
   at all: the shell grants `+net 127.0.0.1:PORT` itself to any line whose literal URL operand is `http://127.0.0.1:PORT/`, by
   ADR-0328):
   (a) connect to every listener of every account on the machine, including another room's server, and read and write what that
   server serves (`service.rs:460-482`: `listener_on(port)` ignores the account);
   (b) find out which ports are listened on machine-wide (`Connected` against `REFUSED`), on every port its rule covers;
   (c) deny service to any listener by filling its 64-slot queue and never closing (each connect costs only the attacker's own
   two streams, and no account is charged).
   It cannot listen without a listen rule, take a port it is not granted, reach the loopback through a name (`conn.rs:199-204`,
   `UNREACHABLE`), or get more than its own two stream ends to the listener. `web` never covers 127/8 (`NOT_PUBLIC`). ADR-0441
   item 4 and its open questions record this, and so do the architect's queue items (1) and (2). For a single-person machine
   with one shell this is acceptable. Once rooms isolate one another it is a confidentiality hole between rooms. The architect
   must decide before accepting ADR-0441. Suggested minimum until then: a listener is reached only from the account tree of the
   session that carved it, which is still the shell's for `python3` and `curl` of one line.

6. **note**: in `crates/zoraya-posix/src/net.rs:123-129` (`net_call_with`), a failed `channel_write` returns `ENETDOWN` without
   closing the `moved` handles. The kernel moves nothing on failure (as `netd.rs:406-408` itself relies on). `listen`
   (`net.rs:571-624`) therefore leaks `theirs` each time `netd` is gone or its channel is full, and `connect` leaks
   `server, in_write, out_read` the same way (that half predates this part). Fix: close `moved` on a write error in
   `net_call_with`.

7. **note**: the `Listen` message's doc comment, `crates/zoraya-proto-net/src/lib.rs:233-243`, says `netd` writes `Accepted` "for
   each connection to the port from the same account". The code and ADR-0441 item 4 say from any account. A security property
   stated wrongly in the protocol definition misleads the next reader. Separately, `netd.rs`'s `loopback` doc comment cites
   INV-0441-2 where INV-0441-1 is meant.

8. **note**: `bind` records the address but `netd` keys a listener on the port alone. A socket bound to `127.0.0.2:P` takes
   connects to `127.0.0.1:P`, a second bind of `127.0.0.1:P` by another program is `EADDRINUSE`, and the accepted socket's
   `getsockname` says 127.0.0.2. This is harmless but undocumented: add it to ADR-0441 item 6 ("the address of a bind is
   recorded and not used by `netd`").

9. **note**: the session account's listener limit is the global 8, minus what the carves took. A holder of a session-account
   connection with `LISTEN` (the shell, or a native program given the root connection or its `Duplicate`) can take all of them
   while no carve is alive. After that every carve with `listen` fails `parent_ok` with `OVER_QUOTA`, so every `+listen` line
   gets no network at all, not even its reach rules. The bound is right, and holders of the root connection are trusted. Record
   that a `+listen` carve fails whole when the session has no listener left.

10. **note**: the tests check less than they claim.
    - `tcptest.c:170` accepts `EADDRINUSE || ENOBUFS` for a port in use. The service checks `BUSY` before the charge
      (`service.rs:571-576`), so the answer is `EADDRINUSE` every time, and accepting `ENOBUFS` would hide a reorder.
    - INV-0441-3's check `a_name_that_resolves_to_loopback_never_opens_a_socket` passes with the `a[0] == 127` guard of
      `conn.rs:202` removed (the mutant list says so and calls it defence in depth: smoltcp's `tcp_open` has no source address
      for 127/8). The invariant then holds through smoltcp, not through the line the ADR names. Say so in the row, or make the
      test observe the guard: a stack fake that would open the socket.
    - `random_requests_never_panic` has no `Listen` or loopback `Connect` seed, so the new ordinal 5 and the 127/8 branch are
      reached only by chance.
    - `tests/python/unixsock.py:25-26` refers to `tests/python/serve.py`, which does not exist (the case is
      `tests/shell/python-serve.script`).

11. **note (regrtest, not run)**: `test-python-regr` runs `regr.py` with no `+net` or `+listen` word, so `listen` there is
    `EACCES` (no `netd` connection, `net.rs:585-587`). Before this part it was `EOPNOTSUPP` at `bind`. Every AF_INET server case
    of the CI subset (`test_asyncio.test_base_events`) that touches the network is already in `regr-ignore.txt`
    (`test_create_server_stream_bittype`, `test_create_server_wrong_sock`, the `sendfile` cases whose `prepare` calls
    `create_server`). The rest of the subset mocks `socket`. `test_socket` is copied for `unixpair.py` only (`regr-unix.txt`)
    and does not run under regrtest. So none of the subset's cases should start running. One kind of case could change silently:
    one that binds port 0 and reads the port back (finding 3), as `find_unused_port` does, now gets 0 instead of an exception.
    I found no such case outside the ignored ones in `test_base_events`. The reason in `regr-ignore.txt:45-46` ("bind, listen:
    plan part F7") is stale: these cases are now refused by the grant, not by a missing call.

## What the architect must decide (from this part)

- The loopback built by pairing streams instead of ADR-0440 item 5's second smoltcp interface. Consequence: no listener can be
  reached from the NIC or the QEMU host. The plan's "done when" (`http.server` serves `curl` in the same room) is met and was
  measured here: `make test-python-serve` PASS 26. The second interface was read in the source, not tried.
- A machine-wide or per-room loopback (finding 5).
- The listeners row in code: from 4, 0, 0 to 8, "1 if its rights have `listen`", 4. This is **not** a deviation from accepted
  ADR-0262. Its table (`0262-*.md:117`) fixes only "0 listeners unless `+listen`" per carve and names no global or column count
  for listeners. The change departs from the *proposed* ADR-0440 item 4 (4 and 8), and ADR-0441 and ADR-0440's appended note
  record that.
- Not built, and recorded in ADR-0224's note: the rooms' `+listen` words, the pills, the hint of a refused `listen`.

## Checked and found sound

- The grant. `listen` needs `LISTEN` and a listen rule that covers the port and TCP (`connection.rs:141-145`). Port 0 needs
  `LISTEN` and is picked only among covered ports above 49151 (`service.rs:222-243`). `BUSY` is told only to a connection
  granted the port, so an ungranted program learns nothing. `narrow` and `narrow_account` cannot add `LISTEN` or a wider listen
  rule (`connection.rs:150-161`, `ListenRule::within`). `Duplicate` keeps the account, and so its one listener (tested). A carve
  gets a listener only when its rights have `listen`, and the shell gives that right only with `+listen`
  (`netgrant.rs:291-298`).
- Bounds and release. One listener per carved account, 8 in `netd`. The charge keeps the account alive (`Account::dead` counts
  `used`), so `release` never touches a freed or reused slot. A listener ends with its channel: `PEER_CLOSED`, Level, on
  `KEY_LISTENER + id`, from a close, an exit or a crash alike. `listener_closed` is idempotent (`slot.take()`). Every refusal of
  `listen` closes the request's handle (`netd.rs` `other =>` arm; the not-a-channel arm closes it and frees the reservation;
  `attach_listener` failing closes it and frees the reservation).
- The `Accepted` transfer. The two ends move with `StreamReadObject::RIGHTS`/`StreamWriteObject::RIGHTS` under the kernel's
  rights containment, so the listener gets at most what the client handed `netd`, and the client's types are checked before.
  The listener holds the client's rings and nothing of the client's account. The client's streams stay the client's memory: a
  listener that never reads can pin them until it closes, as any peer of a pipe can. A refusal (no listener, `WouldBlock`,
  `PeerClosed`) moves nothing and closes both ends and the control end. Queued connections die with the listener channel's last
  end (measured: 64 clients read EOF).
- The library. `bind` takes the wildcard or 127/8 only (`EADDRNOTAVAIL`). AF_INET6 is `EAFNOSUPPORT`. `accept` takes only a
  decoded `Accepted` with two handles and closes anything else. `EMFILE` is checked before the read, so the connection stays
  queued. `install` failing releases the descriptor's handles. `ECONNABORTED` is reported once when `netd` goes. Non-blocking
  `accept` is `EAGAIN`, `SOCK_NONBLOCK` and `SOCK_CLOEXEC` apply to the new socket, and `SO_ACCEPTCONN` reads correctly. Any TCP
  socket, listener or accepted, is refused across `fork` and `spawn` (`jobs.rs:1274-1278`: `unix` is `None`), so nothing
  inherits a listener silently.
- The shell. `parse_listen` is total (1 to 65535, `LOW-HIGH` with low <= high, no `any`, and `+80` is read as 80 by `u32::parse`,
  which is harmless). The marker is a literal prefix token like `+net`. Its expanded argument must be one word. The rules are
  ports and protocols only, so nothing can be injected into a reach rule. `MAX_RULES` counts reach and listen rules together.
  `describe` puts listen lines after reach lines, so `pills` still pairs by index.
- No ADR status or "Architect's decision" was edited. The notes on ADR-0224, ADR-0225 and ADR-0440 are rule-6 notes.
  `cap_memory!` is in `tests/tcplisten.rs`. `make check` (host paths, the invariants table, Total 864) passes. No new `unsafe`
  without `// SAFETY:` in the diff.
- Mutants. The five `pick_port` survivors in `missed.txt` (`service.rs:232`, the arithmetic of `next_port`) are equivalent: the
  contract is "a free covered port", which every variant still finds, and only the rotation of the start (not specified, not
  tested) changes. The peer-port mutant is killed (run above).
- TCG. `tcptest` waits with 60 s polls and asserts no wall time. `python-serve.script` waits for the server's start line before
  `curl`.

## Verdict

No errors. Fix first, all small: warnings 1 to 3 (vacuous negative lines, `POLLOUT` on a listener, port 0 in `getsockname`
before `listen`), and record or test warning 4. Warning 5 is the architect's call before ADR-0441 is accepted, and it does not
block the merge of a proposed design that records it.

## Re-check (930ba9dc)

Re-check by the `opus-review` subagent of `git diff 7638e73a..930ba9dc` (the four fix commits) against the findings above, 2026-11-16.
Read: the diff, `zoraya_net::loopback`, `netd.rs`'s `Action::Loopback` arm and `LoopbackKernel`, `poll.rs`'s socket probes,
`socket.rs`'s `wait_source`, `net.rs`'s `net_call_with` and `listen`, `ffi.rs`'s `channel_write`, the fake kernel's `channel_write`
(`tests/kernel/mod.rs`), `accounts.rs`'s `carve_with`, `zosh.rs`'s carve call, CPython 3.15's `socketserver.py`,
`http/server.py` and `wsgiref/simple_server.py`.

Run by the reviewer, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("all checks passed"); the invariants table has 864 rows and says Total 864.
- `cargo test -p zoraya-net -p zoraya-posix -p zoraya-proto-net -p zoraya-shell`: exit 0; `zoraya-net` 124 passed,
  `tests/tcplisten.rs` 13, `zoraya-shell` 217, every suite 0 failed.
- `make test-net-posix`: exit 0, `net-posix-auto` PASS 66 steps: the five `tcptest: ... ok` lines, `all ok`,
  `tcptest: no listen grant ok`, `tcptest: no reach grant ok`, and no `FAIL` line (the `EADDRINUSE`-only check passes).
- `make test-python-serve`: exit 0, `python-serve-auto` PASS 26 steps.
- Hand mutants, each reverted: the three of `tools/mutants/listen-f7b.toml` on `loopback.rs` (drop `close(in_write)`, drop
  `close(control)`, swap the ends) are each killed by `cargo test -p zoraya-net --lib loopback`. Dropping `self.sys.close(mine)`
  in `listen`'s `Err` arm is killed by `a_request_to_a_gone_netd_leaks_no_handle`. Dropping only the Unix half of the new `poll`
  guard **survives** `cargo test -p zoraya-posix` (R2).

### The earlier findings

1. Fixed. `--no-listen` checks `listen` is `EACCES`; `--no-reach` checks `l >= 0` (so the line has a working `netd` connection)
   and then that the loopback `connect` is `EACCES`. The script waits for each mode's ok line and forbids `tcptest: FAIL`
   everywhere, so a `netd` that grants `listen` with no rule, or connects to the loopback with no reach rule, fails the step.
   R5 is a remaining gap in `--no-listen`.
2. Fixed for TCP: `poll` skips `POLLOUT` for a listener, and `tcplisten.rs` checks `poll(l, POLLOUT, 0) == 0` with and without a
   queued connection. Ordinary sockets are unchanged (the guard only tests `listening`; `net.rs`, `unix.rs` and `poll.rs` POLLOUT
   cases pass). The Unix half has no test (R2).
3. Recorded in ADR-0441 and `docs/backlog.md`, accepted as a limitation. Part of what the ADR says about it is wrong (R1a).
4. Fixed for the refusal decision: `netd.rs`'s `loopback` is now `zoraya_net::loopback::deliver` over `LoopbackKernel`, with the
   same accounting as before: the listener lookup is the same, a delivered connection moves the two ends and closes `control`,
   and every refusal (no listener, `WouldBlock`, `PeerClosed`) sends `Failed { REFUSED }` and closes all three. The counting fake
   fails on a close that is dropped or repeated. `EMFILE` is not a `netd` path: it is the library's `accept` and leaves the
   connection queued (INV-0441-4). The arm that rejects wrong handle types (`netd.rs:528-537`) is still not host-tested (R4).
5. Recorded plainly in ADR-0441 "Decisions for the architect" and `docs/architect-queue.md`, as a condition of ADR-0268's gate.
   R7 is one gap in how it is worded.
6. My note was wrong. `ffi.rs:107-112` closes the moved handles when the kernel write fails, as `Sys::channel_write`'s contract
   says, and the fake kernel does the same. The new test guards the library's own end (`mine`), which a hand mutant confirms.
   (The one path that keeps them is `moved.len() > 4`, `ffi.rs:103`'s early `?`; no caller passes more than 3.)
7. Fixed (the `Listen` doc comment; the wrong INV citation is gone with the old doc comment).
8. Recorded in the ADR's known limitations. 9: recorded, but wrongly (R1b). 10: `EADDRINUSE` only, the listen seeds and the
   `unixsock.py` comment are done; the INV-0441-3 bullet is not (the row still names a test that passes without the guard).
   11: the reason in `regr-ignore.txt` is updated.

### New findings

R1. **warning**: ADR-0441's "Known limitations of the build" (`docs/adr/0441-tcp-listening-as-built.md:142-148`) states two
    things that are not true.
    (a) "CPython's `socketserver` reads it after `listen`". It reads it right after `bind`: `socketserver.py:475-476`,
    `self.socket.bind(...)` then `self.server_address = self.socket.getsockname()`, inside `server_bind`, before
    `server_activate`. So every `socketserver` server on port 0 has `server_address` and `HTTPServer.server_port` set to port 0.
    `wsgiref.simple_server` (`simple_server.py:57`) puts `SERVER_PORT=0` into every request's environment, and a test that
    connects to `server.server_address` connects to port 0. `python3 -m http.server 0` prints the right port only because
    `http/server.py:1068` reads `getsockname()` again after the constructor has listened.
    (b) "a `+listen` line of a carved account has its listen refused (`ENOBUFS`) and the network of that line is useless for
    serving". When the session holds all 8 listeners, `carve_with` fails `parent_ok` (`accounts.rs:213-221`, `OverQuota`), and
    `zosh.rs:1143` turns that into no connection (`.ok()`). The program then has no `netd` connection at all: `listen` is
    `EACCES` (`net.rs:585-587`) and every `connect` fails too, reach rules included. That was the point of note 9.
    Fix: correct both sentences (and name `socketserver` and `wsgiref` in the backlog entry).

R2. **note**: the Unix half of the guard (`poll.rs:271`, `!sk.unix.as_ref().is_some_and(|u| u.listening)`) has no test. Removing
    it survives `cargo test -p zoraya-posix` (run). The mutant in `listen-f7b.toml` removes both halves at once, so the TCP test
    kills it. Fix: a `unix.rs` case (`poll(listener, POLLOUT, 0) == 0`), and a mutant for that half alone.

R3. **note**: the comment at `poll.rs:269-270` says "POLLOUT alone waits for a hang-up only". With `POLLOUT` alone no probe is
    pushed: the hang-up branch below runs only when neither `POLLIN` nor `POLLOUT` is asked. So it waits for nothing, which matches
    Linux (a listener reports neither `POLLOUT` nor `POLLHUP`). The comment is what is wrong. Likewise the two "never writable"
    comments in `wait_source` (`socket.rs:664`, `:693`) sit on a `None` that `poll` reads as "ready now".

R4. **note**: the check text of INV-0441-1 (ADR-0441 and `invariants.md:749`) is unchanged. It does not name `loopback::tests`,
    and it does not say that the wrong-handle-types arm (`netd.rs:532-537`) is checked by reading only. Finding 4 asked for that.

R5. **note**: `tcptest --no-listen` gets `EACCES` both from a `netd` that refuses and from the library when the program has no
    `netd` connection at all (`net.rs:585-587`). The mode would therefore also pass if the shell stopped giving the line a
    connection. The netd mutant that matters fails the step, as said above, so this is not vacuous. Fix: in the same mode,
    connect to `127.0.0.1:LOW` and expect `ECONNREFUSED`, which only `netd` answers.

R6. **note**: `docs/design.md`'s "Limits (ADR-0441)" paragraph does not list `bind(0)` then `getsockname`, or that the address of
    a bind is not used. They are in the ADR and the backlog only.

R7. **note**: the decision for the architect says "an untrusted line has no `+net 127.0.0.1` word to give". There are other ways
    to get a reach rule for the loopback: the shell's own grant for a literal `http://127.0.0.1:PORT/` operand (ADR-0328,
    `netgrant_tests.rs:164-166`), `any`, and a prefix over 127/8. The gate condition should name all of them, or say "no reach
    rule that covers 127.0.0.0/8, however made".

R8. **note**: `user/describe/posix/tcptest.cmd` declares `-L` and `-R`, which `tcptest.c` does not parse: `tcptest -L` runs the
    full suite. Either drop the short forms or accept them.

### Checked and found sound

No ADR status or "Architect's decision" section changed. The changes to ADR-0441 are a proposed ADR's own text. The diff adds no
`unsafe`, no host paths or addresses, and does not touch `docs/research/`, a task file or `CLAUDE.md`. `deliver` sends
`Accepted` with the read end first, as before. If `Connected` failed to encode, it would now send an empty message where the old
code sent nothing, but 64 bytes always suffice.

### Verdict

Fix first, documentation only: R1 (two false sentences in ADR-0441's limitations, and the backlog entry). The code fixes for
findings 1, 2, 4 and 6 are sound and measured. R2 to R8 are notes and can go with R1 or be recorded.
