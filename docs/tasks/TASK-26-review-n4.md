# TASK-26 review of N4

An independent review by the `opus-review` subagent, which did not write the code, of
`git diff origin/main...origin/work/n4` (da771a0...96a24a3). Read around the diff: `crates/zoraya-posix/src/{net,socket,
process,jobs}.rs`, `ffi.rs` (`bootstrap`, `RtSys::channel_write`, `c_str`, `put`, `user`), `ffi/sockets.rs`, `ffi/procs.rs`
(`fork`, `exec`, `spawn_into`), `crates/zoraya-handle/src/lib.rs` (`insert`, `free_index`), `crates/zoraya-net/src/service.rs`
(`connect`, `resolve`, `resolve_done`, `remove`), `user/src/{posix_child,netclient}.rs`, `user/src/bin/{zosh,netd}.rs`,
`crates/zoraya-shell/src/{grants,netgrant}.rs`, `tests/posix-net/zfetch.c`, `tools/posix-surface-check/src/surface.rs`,
`fuzz/fuzz_targets/net_stack.rs`. Checked against ADR-0005 (INV-0005-2), ADR-0050 (INV-0050-1), ADR-0102 item 3, ADR-0225,
ADR-0262, proposed ADR-0287 and the plan rows N4 and Q3. No command was built or run; nothing here is a test result.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `crates/zoraya-posix/src/jobs.rs:853` (`after_fork_in_child`), `:771-848` (`fork_moves_into`) | The report's premise "a fork child holds the connection" is false. A channel end is not duplicable (ADR-0102 item 3) and `net` is not in the move list, so the child's `Posix::net` names an **empty slot at the parent's generation**. `Table::insert` reuses an empty slot without raising its generation (`zoraya-handle/src/lib.rs:121-135`) once the 1024-slot table is full, and the `ZPBN` handle sits above 1005, so after a few allocations in the child the slot holds an unrelated channel with the very value `net` holds. Scenario: a program forks, the child opens files, then calls `getaddrinfo`/`connect`: the library sends `Resolve`/`Connect`, with two stream ends moved, to a file service's connection and discards that connection's replies while it waits for its own transaction (`net.rs:103-117`). This is the failure ADR-0287 rejects Option C for. Before reuse, the child's `connect` fails with `ENETDOWN`, not ADR-0287 item 3's `EACCES`. There is no authority gain: exec sends `ZPBS` (`procs.rs:1220`), and the scope is the URL the person typed for the same image. So it is not a hole for the gate, but it is a stale handle in the library. Fix: `self.net = None` and clear `addrs` in `after_fork_in_child`, with a host test (fork, then `connect` gives `EACCES`). Correct the report's question 1 and ADR-0287 item 3 ("a child, spawned or forked, has no network"). |
| 2 | warning | `crates/zoraya-net/src/service.rs:305`, `:423` | A lookup token has only 16 bits (`LOOKUP_BASE + (next_token & 0xffff)`). Lookups answered at once from the cache also use up tokens, and `resolve_done` takes the first lookup with that token. Scenario: connection A keeps a slow lookup pending and sends 65,535 resolves that hit the cache. Connection B's next lookup then gets A's token, and B's answer (name and addresses) goes to A, under A's scope, across accounts. Fix: skip any token still in `lookups`, or use 32 bits above `LOOKUP_BASE` with that check. Add a host test for the wrap. |
| 3 | warning | `user/src/netclient.rs:90` (only definition), `user/src/bin/zosh.rs:902`, `:1054` | Q3's `netd` account is built but no launcher uses it. `narrow_account` has no caller, and `zosh` uses `narrow` for both POSIX and native programs, so every program's connections count against the shell's account. The account bound and `OVER_QUOTA` are only host-tested, and Q3's done-when for its `netd` half is not shown in the system. The report's N4 "Built" does not say this, and "Not done" does not list it. Fix: list it in the report's "Not done", or make `posix_net` use `narrow_account` with a budget and add a script line that goes past it. |
| 4 | warning | `docs/tasks/TASK-26-report.md` N4, item 4; `docs/research/network/plan.md` row N4 | The N4 done-when is not met. It needs `+net test.zoraya curl -O ...` and the wget form under HVF and TCG. There is no curl or wget, no `+net`, and the TCG run (`FULL=1`) was not done. Only the counting fake's half (one `resolve` per `getaddrinfo`) is met. `zfetch` is a fair stand-in for the library path (same checksum as `fetch`, INV-0287-3), and the gap is recorded honestly. Keeping W11/W13 open, and deciding whether N4 counts as done, is the architect's call. The TCG run must come on `candidate-full/`. |
| 5 | note | `crates/zoraya-posix/src/jobs.rs:583-604`, `:692` | `exec`, `posix_spawn` and `system` map every non-`CLOEXEC` fd. An open socket fd without `SOCK_CLOEXEC` therefore makes the whole spawn fail with `EBADF`, where Linux would let the child inherit it. Record this in ADR-0287 item 3 as stated behaviour. |
| 6 | note | `user/src/posix_child.rs:468` | A failed `ZPBS`/`ZPBN` write is ignored. Under INV-0050-1 the caller keeps `me` and `net`, so each such launch leaks two handles in `zosh`. Fix: close both on `Err`. |
| 7 | note | `crates/zoraya-posix/src/ffi/sockets.rs:236-243` | `recvfrom` takes the data first and then returns `EFAULT` when `addr` is set and `addrlen` is NULL, so the bytes are lost. Fix: check the pair before calling `recv_flags`. |
| 8 | note | `crates/zoraya-posix/src/ffi/sockets.rs:290-293` | `getsockopt` sets `*len = 4` even when it copied fewer bytes. Linux stores the length it copied. |
| 9 | note | `crates/zoraya-posix/src/net.rs:160-197` | A `connect` to an address that `resolve` returned is sent by name, so `netd` picks its own address. A program that loops over `ai_next` reaches the same peer every time, and `getpeername` may differ from the address it passed. This follows from ADR-0225 item 3; ADR-0287 should state the consequence. |
| 10 | note | `tests/posix-net/zfetch.c:248` | `atoi(head + 9)` reads past the NUL when the head is shorter than 9 bytes (`HTTP/1.` alone), into stack bytes that were never written. Fix: require `strlen(head) >= 12`. |
| 11 | note | `user/src/bin/zosh.rs:882-903`; `crates/zoraya-shell/src/netgrant.rs:23-27` | A literal `url` designates the network with no check of the room's trust, for native and POSIX programs alike, and trust is by path: any binary at `/bin/zfetch` gets the designated network. This is the posture of ADR-0153 rule 10, and the gate refuses untrusted rooms for now. It belongs to Q7's gate tests before the gate is lifted. |

Checked and sound:
- `struct addrinfo` matches musl LP64: offsets 0, 4, 8, 12, 16, 24, 32 and 40, size 48, with `sockaddr_in` and the
  name in the same `malloc` block. `freeaddrinfo` walks `ai_next`.
- The `unsafe` blocks of `ffi/sockets.rs`:
  - `copy_bytes` is bounded to 128 bytes, and `put_sockaddr` copies `min(room, 16)`.
  - `inet_ntop` checks `room` before `put`, and `inet_ntoa` fits in 16 bytes.
  - `c_str` stops at `PATH_MAX`, and the `recv`/`send` buffers go through `user()` to the kernel's checked copy.
  - Hints are read as four `int`s, and every `// SAFETY:` argument holds.
- The `ZPBN` bootstrap:
  - Only `posix_child` sends it, and only when `l.net` is `Some`. The broker, `posixrun` and libposix's own spawn and
    exec send `ZPBS`.
  - `libposix` takes the second of exactly two handles, and only if it is a channel, so a program without the grant
    has `net == None` and gets `EACCES`.
  - Every failure path of `posix_child::launch` closes `l.net`.
  - `RtSys::channel_write` closes moved handles on failure (`ffi.rs:96-100`), so `net_call` leaks nothing.
- Replies from `netd`:
  - Each is matched by transaction and decoded by the protocol crate, and an unknown status or reason gives `EIO`.
  - `Resolved` holds at most `MAX_ADDRESSES` addresses.
- The parsers `parse_sockaddr_in`, `inet_pton4`, `inet_aton` and `plan` are total.
- The deny-list change is consistent with ADR-0225 item 1 (accepted) and with INV-0005-2's "unless an accepted ADR adds
  it". All 22 names in ADR-0287's block are in ADR-0225's TN list, so the proposed ADR is not their authority.
  ADR-0005's superseding note and the text of INV-0005-2's row in `invariants.md` are left to the architect.

Fuzzing:
- No new target is needed for the libposix parsers. They run inside the program, so a bad input only hurts the program
  itself, and their host tests cover the boundary cases.
- The boundary is `netd`. `net_stack` target 4 drives `Service::handle` on the root connection only. It should be
  extended to a connection narrowed with an account (ordinal 7, then 3 and 4 on the child) and to `resolve_done` with
  fuzzed tokens, which would reach finding 2.
