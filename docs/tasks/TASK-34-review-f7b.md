# TASK-34 review: part F7, second independent look (socket files in the line's `/tmp`; TCP listening as a design only)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f7` at 1aec00e5 (five commits), 2026-11-16,
read against `CLAUDE.md`, `docs/tasks/TASK-34-foundations.md`, ADR-0412 item 5, ADR-0225 item 5, ADR-0430, ADR-0433, ADR-0310,
the proposed ADR-0439 and ADR-0440, and the earlier reviews of TASK-34 (f2 to f6, and `TASK-34-review-f7.md`, which this review
re-derives instead of trusting). Code read around the diff: `Service::open_as`, `send`, `flush_held`, `close_connection`,
`socket_listen`, `socket_connect`, `rename`, `publish`, `remove` (`crates/zoraya-fs-service/src/lib.rs`); the kernel's
`channel_write` (`kernel/src/syscall.rs`: rights containment, `may_travel`, the writer's payment) and `Pair::prepare`
(`kernel/src/channel.rs`: a closed peer is `PeerClosed`); `fsd`'s transport; `unix_file.rs`, `unix.rs`, `jobs.rs`'s fork guard,
`process.rs`'s release; the broker's `start_tmpd`; `ports/apps/zshim.c` and the link lines of `ports/Makefile`;
`zoraya-proto-net` and `zoraya-net/src/config.rs`; `tests/python/regr-*.txt`.

Run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `cargo test -p zoraya-fs-service -p zoraya-posix -p zoraya-proto-file`: exit 0, 20 suites, 450 passed, 0 failed
  (`tests/socket.rs` 17, `tests/socketfile.rs` 16).
- `make check`: exit 0 ("all checks passed").
- `make test-libposix`: exit 0 (`libposix-auto` PASS 167 steps with `t_unix: file passed` in its log, `libposix-maps-auto` PASS 17,
  `libposix-top-auto` PASS 66).
- A link test of finding 1 (scratch files outside the repository): `zshim.c` compiled with the ports compiler into a `libzshim.a`,
  and a program that calls `socket`, `bind` and `listen` linked with `-lzshim` and without it.

Not run: the hand mutants of `tools/mutants/sockets-f7.toml`, `make mutants-host`, any Python, curl or wget step. Nothing here claims
they pass.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

1. **error**: the ported programs never reach the new `bind`, `listen` and `accept`: `libzshim.a` still defines them as `ENOSYS`
   stubs and is linked ahead of `libc.a`. `ports/apps/zshim.c:17-36` defines `bind`, `listen`, `accept`; `ports/Makefile:1131`
   links CPython with `LIBS="-lpyshim -lzshim"`, and curl (`:834`), wget (`:867`), git (`:909-910`), openssh (`:538`) and the libcurl
   `.pc` (`:1258`) the same way; the compiler driver puts `-lc` after every input (`tools/zcc:154`). An archive member is extracted
   for the first undefined symbol it resolves, so `zshim.o` is taken for `bind` and `libc.a`'s `zw-bind.o` never is (the default
   `POSIX_STATS=1` gives each export its own wrapper member, which is why no duplicate symbol is reported). Measured: a program
   calling `socket`, `bind`, `listen`, linked with `-lzshim`, has `bind` at `0x43235c`, next to `main` and the shim's `link` and
   `setsid`; linked without it, `bind` is at `0x4f5524`, next to libc's `socket`. Consequences: (a) `python3`'s
   `socket.bind`/`listen` on an `AF_UNIX` socket fail with `ENOSYS`, so ADR-0439's **Positive** consequence ("a Python
   `socketserver` on a Unix socket, a `multiprocessing` listener ... can run", `0439-socket-files-as-built.md:116`) and the design
   bullet's claim are false for the shipped interpreter; (b) the report's risk note that `test-python-regr` "must show whether an
   `AF_UNIX` server now passes or hangs a case" is moot: nothing in Python changed for `bind`; (c) one configure result does flip:
   `accept4` was absent from every library and is now in `libc.a` (not in the shim), so CPython's `AC_CHECK_FUNCS` finds it and
   `sock.accept` calls libposix's `accept4` while `bind` and `listen` stay the shim's; curl's and gnulib's checks of `accept4` flip
   the same way (harmless for clients, but a half-real socket API); (d) ADR-0310 (accepted) says at line 175 "a program that needs
   `bind`, `listen` or `accept` (a server): the shim's refusals become real calls in `libc.a`", and the shim's header still says it
   holds "names ... Zoraya's libc.a does not export". *Fix:* delete `bind`, `listen` and `accept` from `zshim.c` (and its comment),
   then run a candidate with `test-apps`, `test-python` and `test-python-regr` (`tools/test-map.txt` maps `ports/apps/` to them);
   the regression risk is small (below), but it must be run, not inferred. If the architect prefers to leave the ports as they
   are, correct ADR-0439's Consequences, the design bullet and the report instead, and say that no ported program can listen.
   *Regression risk once the shim goes:* AF_INET `bind`/`listen` turn from `ENOSYS` to `EOPNOTSUPP` (both `OSError`); the
   `test_asyncio.test_base_events` cases that bind are already in `regr-ignore.txt:45-56`; `test_socket` is not in
   `regr-modules.txt` (only `BasicSocketPairTest` through `unixpair.py`), and neither `test_unix_events` nor `multiprocessing` is
   run; curl uses `bind`/`listen` only for `--interface` and FTP active mode, wget for FTP `PORT`: none is in a test.

2. **warning**: ADR-0439 still overstates what a program with `WRITE` on the file can do. "Effect of the vision" ends: it "can fill
   the 64-message queue ... and nothing else" (`0439-socket-files-as-built.md:110-111`). From the code it can also: take every one of
   the service's 64 listener slots (`MAX_LISTENERS` counts the whole service, `lib.rs:1718-1726`; the per-account `room` check
   bounds handles, not listeners), after which every other program of the line gets `ENFILE`; fill the 256 nodes (`ENOMEM` for the
   line until something is unlinked); and make `fsd` pay for queued connections: each `socket_connect` message is written by the
   service's process, so `channel_write` charges `message_cost` to `tmp_job` (ADR-0261 point 7), up to 64 listeners x 64 messages,
   while the connector pays only for its pair's objects. `tmp_job` is made with no limit of its own (`user/src/bin/broker.rs:581`),
   so the cost lands on the broker's job. The earlier review raised this (its finding 4); the report says "the rest in the ADR and
   backlog", but the sentence is unchanged and the backlog records only the 256 nodes. *Fix:* replace the sentence with the list,
   record the shared 64 listeners and who pays the queue in the ADR's Negative consequences and `docs/backlog.md`, or bound listeners
   per account as ADR-0440 does for `netd`.

3. **warning**: the name of a live server can be replaced by any program of the line, and ADR-0439 reads as if it could not. Item 3
   says "another program cannot win the race between `bind` and `listen` or take an inert node over". That holds for the node. It
   does not hold for the name: program Q with `WRITE` on `/tmp` does `unlink("/tmp/s")` (or `rename` of its own file over it, which
   `socket_unnamed` handles, `lib.rs:2835-2849`) and then `bind("/tmp/s")`; server P keeps listening on an unnamed node, and every
   client that connects by path from then on reaches Q, which can relay to P and read everything. Same thing between P's `bind` and
   `listen`: Q unlinks and rebinds, P's `listen` then fails with `EPROTOTYPE` (`socket_errno` of `INVALID_ARGUMENT`,
   `unix_file.rs:37`). A client has no way to tell (no `SO_PEERCRED`, ADR-0430). On Linux the sticky bit of `/tmp` stops this between
   users; here the line's programs are one user and one `/tmp`. *Fix:* say in item 3 and in "Effect of the vision" that the name is
   only as trustworthy as every program with `WRITE` on the line's `/tmp`, so a socket file authenticates nothing; if that is not
   wanted, the architect's question is whether `unlink`/`rename` of a node with a live listener should be refused to every
   connection but its owner's account.

4. **warning**: the fix for the earlier review's finding 2 (`OverQuota` no longer kills a live listener) lives only in
   `user/src/bin/fsd.rs:150-157` (`Status::NoMemory | Status::OverQuota => Outgoing::NoMemory`, `AccessDenied | BadState |
   BadHandle => Refused`), which no host test runs and no hand mutant covers; the service tests drive the fake transport's
   `full` and `refuse` sets directly. A regression that maps `OverQuota` back to `Closed` passes every test listed in
   INV-0439-1/-2. I checked the mapping by reading the kernel: a closed peer is `PeerClosed` (`kernel/src/channel.rs:84`), so it
   stays `Closed`; `InvalidArgument` (the remaining `Err(_)` arm) cannot be caused by a connector (64-byte record, at most three
   distinct handles); so the mapping is right today. *Fix:* move the status-to-`Outgoing` function out of `fsd.rs` into
   `zoraya-fs-service` (a pure `fn outgoing(Result<(), Status>) -> Outgoing`) with a table test, or add hand mutants for the two arms
   and say which step catches them.

5. **note**: a listener whose service end closed never recovers. `unix_listen` returns `Ok` when `u.listening` is set
   (`unix_file.rs:149`) without asking the service; `accept` answers `ECONNABORTED` on every call (`unix_file.rs:324`); poll reports
   it readable for good (`socket.rs`, `READABLE | PEER_CLOSED`). With finding 2 of the earlier review fixed, this is reached only
   when the `/tmp` service itself goes (its process ends) while the program lives; a server that retries `ECONNABORTED`, as most do
   on Linux where it is transient, then spins. *Fix:* on `PeerClosed`, clear `listening` and close `ctl` so that the next `accept` is
   `EINVAL` and a `listen` asks again (and fails), or record the behaviour in ADR-0439 item 5.

6. **note**: a refused `bind` can leave a named, inert node. `open_as` inserts the `SocketNode` whenever the create made a new
   object, also when `add_connection` failed (`lib.rs:2225-2247`: `owner: added` is then `None`, the reply `LIMIT_REACHED`): the
   library's `bind` fails, the file stays in `/tmp`, its name is `EADDRINUSE` until someone unlinks it, and it holds one of the 256
   entries. Bounded and visible, so a note. *Fix:* remove the new name (or skip the create) when the connection cannot be added.

7. **note**: in `socket_listen` the listener bound and `room` check (`lib.rs:1722-1726`) comes before the owner check
   (`lib.rs:1731`): a non-owner gets `TOO_MANY_OPEN` (`ENFILE`) instead of `EACCES` when the table is full, and `refused_by`
   counts a refusal against its account (ADR-0262 item 2) for a request it could never make. Move the owner check up.

8. **note**: errno differences the earlier review listed (its 6a to 6c) are neither fixed nor in the backlog: `listen` after the
   name was unlinked is `EPROTOTYPE`; `listen` of an unbound `SOCK_DGRAM` socket is `EINVAL` where Linux says `EOPNOTSUPP`; a far end
   the kernel refuses (`Refused`) is `EPROTOTYPE` to the connector. Record them in ADR-0439 or the backlog.

9. **note**: what the tests and documents claim beyond what they check. (a) INV-0439-3's Check still cites "the property over every
   length and byte pattern in `tests/socketfile.rs`" (`invariants.md`, ADR-0439:133); the loop is 64 lengths x 256 arithmetic
   sequences, of which almost none decode; it shows totality, not the property. (b) No test connects through a read-only `/tmp`
   entry: the library's `connect` asks `Access::Write` of the resolver, the service refuses a `WRITE` open from a read-only
   directory connection (`open_as`, `has(i, needed)`, `lib.rs:2129`) and a `WRITE`-less node connection (tested), and `t_unix file`
   connects to `/bin/t_unix` (read-only, but not a socket). I found the refusal sound by reading; a `socketfile.rs` case with a
   `Tmp` entry opened read-only would pin it. (c) Of the 12 `make mutants-host` misses in `zoraya-posix`
   (`build/mutants/cargo/mutants.out.old/missed.txt`): the four `|` to `^` (`socket.rs:638`, `unix_file.rs:111,115,116`) are disjoint
   bits, equivalent; `unix_file.rs:96` (`node || name` to `&&`) is equivalent, both are set together; `unix_file.rs:121` (`||` to
   `&&`) and `:189` (the error-reply guard) are defence-in-depth paths no current service reaches (a non-socket service refuses the
   type bits first; the library's own `S_IFSOCK` check precedes an `UNSUPPORTED` error reply); `net.rs:526` (`SO_PROTOCOL` arm)
   falls through to the same 0; the three `fork_moves_into` mutants are holes of the host tests, caught only by `t_unix file`'s fork
   checks in QEMU. None is a security hole. The three equivalents of `sockets-f7.toml` are as stated.

10. **note**: ADR-0439 item 7 says a listening socket is "not inherited by `spawn` ... (`EBADF`)". What happens is that the whole
    `posix_spawn` fails with `EBADF` when a Unix socket without `FD_CLOEXEC` is open (ADR-0433 item 6; the report found it in QEMU).
    A C server that binds without `SOCK_CLOEXEC` and spawns a helper (a CGI, a pager, a worker) fails to spawn at all; the ADR should
    say "`posix_spawn` fails", not "is not inherited".

11. **note**: report bookkeeping: `TASK-34-report.md:8` still says F7 is in progress after F6, and `:28` lists "no
    `bind`/`listen`/`accept` (F7)" as a known limitation.

12. **note**: `accept` installs whatever objects arrive with a well-formed record (`desc_from` after `decode_connect`), as the
    earlier review's note 9 said. I followed the worst case: a connector hands the listener, as the "channel" of a packet socket, the
    client end of its own connection to a file service. The listener's sends then reach that service as requests with the
    connector's rights, never the listener's, and replies arrive as packets the listener drops. No authority moves to the
    connector; it is a confusion, not an escalation. `transport.kind` in `socket_connect` would refuse it cheaply.

## What was checked and found sound

- **Who binds where.** `bind_place` allows only an entry of kind `Tmp` with a non-empty, non-directory rest (`EACCES` elsewhere,
  `EINVAL` for `/tmp` and a trailing slash); a path no entry covers is the resolver's `ENOENT` with its report; an intermediate link
  out of `/tmp` lands in a service with no socket nodes, which refuses the type bits (`EINVAL`, tested), and the entry is checked
  again after the open. `S_IFSOCK` is kept only by a service that called `enable_sockets()`, which only `fsd --memory` does.
  INV-0412-3 holds with the narrower place, and its check (unit test, `socketfile.rs`, `t_unix file` with `/bin` `EACCES`) tests it.
- **Who listens.** Only the connection whose open created the node (`owner`), cleared in `close_connection`; a reused connection
  index is not the owner (tested); one listener per node and per connection.
- **Who connects.** The path resolves in the caller's own table; `WRITE` on the node connection is required by the service and
  cannot be had from a read-only directory connection (`open_as`). Another line's `/tmp` is another service, so its nodes are not
  reachable (tested). `WRITE` is the right Linux asks for (`connect` needs write permission on the socket file).
- **Rights.** The far end is the connector's own objects; `fsd` forwards each handle with its type's full rights and the kernel
  refuses an entry not contained in the held handle's (`AccessDenied`) and a channel end that cannot travel (`BadState`), both
  `Refused`, which keeps the listener (tested). Nothing can arrive wider than the connector held.
- **Handles on refusal.** Every early return of `socket_listen` and `socket_connect` leaves the request's handles to `ready`; a failed
  `channel_write` changes nothing (INV-0050-1); a full queue is `LIMIT_REACHED` and `EAGAIN` with the far end closed and the
  connector's near ends closed by the library (tested both sides); the listener end is charged once and released once.
- **Lifecycle.** A dead listener's queued connections end with its channel (the connector reads EOF, tested); a connect to an inert
  node is `ECONNREFUSED` at once, never a wait; a new `bind` of the name needs `unlink` and makes a new id (ids are never reused).
- **What is not carried.** `fork` with a bound or listening socket is `ENOTSUP` (QEMU); `SCM_RIGHTS` of a listener `EINVAL`
  (tested); `posix_spawn` refuses a non-`Connected` Unix socket (`EBADF`); nothing inherits a listener.
- **TCP.** `bind`, `listen`, `accept` of an `AF_INET` socket are `EOPNOTSUPP` in libposix (tested). `netd` has no `listen` request
  (`zoraya-proto-net` ordinals 1 to 8, 16 to 18, 32, 33, 40) and `zoraya-net/src/config.rs:52` rejects loopback as a host address,
  as ADR-0440 says. The report, ADR-0440, the backlog and the architect queue state plainly that the plan's "done when"
  (`python3 -m http.server` served to `curl`) is not met; the queue now asks the architect whether the TCP half becomes its own
  plan part and for the word of a listen rule, and the own-directory question.
- **Bookkeeping.** `invariants.md` Total 859, INV-0440-1/-2 `not yet applicable` (true); every new `unsafe` block in
  `ffi/sockets.rs` has a `// SAFETY:` comment stating C's contract; both new test files install `cap_memory!()`; no host paths.

## Verdict

**Fix first: finding 1.** As built, no ported program (Python, curl, wget) can use the socket files: the shim shadows the calls,
and the documents claim Python servers work. Remove the three stubs and run a candidate with the Python and apps steps, or correct
the claims. Findings 2 and 3 are documentation of the security model and should land with it; 4 is a test gap of the one fix that
matters for liveness. Notes 5 to 12 may go to the backlog. The decision on the TCP half (ADR-0440) is the architect's.
