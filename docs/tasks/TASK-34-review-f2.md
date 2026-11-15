# TASK-34 review: part F2 (`AF_UNIX`, `socketpair`, `SCM_RIGHTS` as handle transfer)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f2` at 16fce2d4 (the merge e2572b5b
included), 2026-11-15, read against `docs/tasks/TASK-34-foundations.md` row F2, `docs/research/browser-engine/plan.md`
row F2, ADR-0412 (accepted) items 1 to 4, 6, 7 and the proposed ADR-0430. The kernel's `channel_write`
(`kernel/src/syscall.rs:880`) and ADR-0102 item 3 (what a snapshot duplicates) were read for the transfer and `fork`
claims.

Tests run by the reviewer on the merged tree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("all checks passed").
- `make test-posix-threads`: exit 0, `PASS: 104 steps`, `t_unix: block passed`.
- `make test-python`: exit 0, `PASS: 112 steps`; the plan's "done when" line
  `python3 -c "import asyncio; print(asyncio.run(asyncio.sleep(0, 'ok')))"` printed `ok`, and `smoke.py` printed
  `ok socketpair`.
- A throw-away host test (removed again, not committed) for finding 1.

Not run by the reviewer: `make test-libposix` (the `t_unix` cases other than `block`), `make test-python-regr`
(`unixpair.py`), the host suite as a whole.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

1. **error**: a stream socket sent from a `fork` parent is lost, and so is everything else in its message, while
   `sendmsg` reports success. Where: `crates/zoraya-posix/src/unix_io.rs:293` (`r.shared = u.shared`) and `:296-305`
   (`has_anc` is set whenever `s.ctl` is `Some`), against the decoder's rule `r.has_anc != r.shared`
   (`crates/zoraya-posix/src/unix.rs:169`). After `fork`, `after_fork_in_parent` (`unix_io.rs:1095`) sets `shared` on
   every Unix stream socket, and the parent keeps the ancillary channel (`ctl`), as ADR-0412 item 6 says it should. So
   the parent encodes a record with `shared` and `has_anc` both set. The send succeeds and `finish_moves` marks the
   sender's description dead and closes the fd. The receiver's `decode_packet`/`decode_anc` then refuses the record,
   and the whole message is dropped with all its handles closed: the moved socket, and every pipe end and file sent
   beside it.
   *Scenario, reproduced:* `socketpair(STREAM) -> (a, b)`, `socketpair(SEQPACKET) -> (x, y)`, `pipe -> (pr, pw)`,
   `after_fork_in_parent()`, `sendmsg(x, "s", [a, pr])`. The send returned `Ok(1)` and `a` is gone. `recvmsg(y)` gets
   no message (the fake kernel's wait reported `EBUSY`, because the queue is empty). INV-0412-1 is broken: a
   descriptor sent does not arrive, and the sender has lost it. No test sends a socket that is shared.
   *Fix:* in `prepare_one`, a shared stream socket sends only its byte streams: no ancillary handle, `has_anc =
   false`. `finish_moves` then closes the parent's ancillary channel along with the rest (or refuse the send with
   `EOPNOTSUPP` before anything is made, if the move should not be offered). Add a test in `tests/unix.rs`: send a
   socket after `after_fork_in_parent`, and send one from the child (`after_fork_in_child`). Both must arrive and
   work, with the other descriptors of the message.

2. **warning**: a `fork` child leaks the stream handles the parent's library held. This breaks INV-0430-2 ("closed
   exactly once") in the child. Where: `unix_io.rs:1108-1119` (`u.book.after_fork()`, `u.held = None`) and
   `unix.rs:363-366` (`pending.clear()`). ADR-0102 item 3 duplicates every stream end into the child at the same slot.
   So the handles of a pipe end, and the byte streams of a stream socket, that sit in `book.pending` (a tag received
   before its byte), or in `held` (a packet socket shut both ways still passes the fork check at `jobs.rs:911-913`),
   exist in the child too. The child forgets them without closing them. Channel ends (files, directories, packet
   sockets, ancillary channels) are absent in the child, so forgetting is right for those and wrong for stream ends.
   *Scenario:* a parent receives "AAAA" and stops before the tag of "BBBB", which carries a pipe's write end, then
   forks. The child holds a copy of that write end that nothing in it names. The pipe's reader sees no end of file
   until the child exits or execs.
   *Fix:* in the child, close the stream-end handles of what was held (records of kind `PipeRead`/`PipeWrite`, and a
   `Stream` record's read and write handles), and drop the channel handles unclosed. Test it with the fake: after
   `after_fork_in_child`, the handle count drops by exactly the stream ends held.

3. **warning**: the architect's decision on ADR-0412 is not carried out. It reads: "The changes to ADR-0225 (items 1
   and 10) and ADR-0342 item 11 ... are accepted; each is recorded by the foundation part that implements it (plan F2,
   F7)". F2 changes ADR-0225 item 10 (a stream pair gains an ancillary channel) and ADR-0342 item 11 (`sendmsg` and
   `recvmsg` carry `SCM_RIGHTS`). Neither ADR has a note: `grep 0412 docs/adr/0225-*.md docs/adr/0342-*.md` finds
   nothing. A reader of ADR-0342 still finds "without ancillary data". *Fix:* add the amendment notes the decision asks
   for, in the form the architect used for ADR-0156 item 2 (commit f969cf41).

4. **warning**: ADR-0412 item 6 is half built, while the task assigns all of item 6 to F2, and INV-0412-1's Status
   reads as done. A Unix socket does not cross `spawn` or `exec` (`EBADF`, `jobs.rs:786`, ADR-0430 item 7). Item 6
   decides that the ancillary channel moves to a spawned child that is given the socket, and that a packet socket
   listed in a spawn's file actions moves to the child. Recording the deferral in a proposed ADR is the right
   procedure. But it is a deviation from an accepted item that the task's row names, so it needs the architect's
   decision on ADR-0430 before merge, not after. The `invariants.md` row for INV-0412-1 says
   `checked: ... ; the in-QEMU half between two processes waits for plan part F4`. The ADR's stated check (an in-QEMU
   C test between two processes) does not run. *Fix:* the architect accepts ADR-0430 item 7 (or F2 builds the move),
   and the Status names the half that is not applicable yet in the form `adr-check` expects, so that the row is not
   read as fully checked.

5. **note**: a tag that lands at the read offset after the first look skips the `EMFILE` rule. Where:
   `unix_io.rs:846-851` checks `due_at_read` before the read. `:902` then files tags that came during the read. A tag
   whose offset equals the old `read_off` (the sender queued it and wrote its byte between the reader's first
   `drain_tags` and its `stream_read`) is consumed by `finish_stream` with no room check. With a full table,
   `install_received` closes its descriptors and sets `MSG_CTRUNC`. ADR-0412 item 3 says the message stays queued and
   `recvmsg` is `EMFILE`. It is narrow (a full table plus the race), but the descriptors are lost. *Fix:* after the
   second `drain_tags`, if the tag at the old `read_off` needs more fds than are free, put the bytes back into `rest`
   and return `EMFILE`.

6. **note**: ADR-0430 item 6 says a shared holder closes a peer's descriptors "as they arrive, so that its ancillary
   queue never fills". This holds only while the parent reads: `drain_tags` runs only inside `recv_stream`
   (`unix_io.rs:841`). If the child is the reader and the parent never reads that socket, the parent's ancillary
   channel fills at 64 messages. The peer's next `sendmsg` with rights then blocks (or is `EAGAIN`) for as long as the
   parent lives. *Fix:* say so in ADR-0430 item 6, or drain the shared socket's channel in `poll`/`close` as well.

7. **note**: the stream window of ADR-0430 item 4 is wider than recorded. After the ancillary message is queued and
   the sockets are moved (`unix_io.rs:503-508`), `write_pieces` can return any error with `done == 0` (`:546`), not
   only `EPIPE`. The caller sees a failed send whose descriptors were delivered (pipes and files are copies) or lost
   (moved sockets). A program that retries sends them twice. The open question lists only a peer that closes. *Fix:*
   record that every error after the commit has this shape, or return the error only when the kernel reports that
   nothing was queued.

8. **note**: the INV-0412-1 tests check "rights or fewer" only through the library's `access` field
   (`tests/unix.rs:326`, `t_unix.c:209`: `EBADF` comes from the library, before any request). No test shows that the
   `share`d connection itself lacks `WRITE` at the service, which is what the invariant protects against a receiver
   that does not use this library. *Fix:* in the fake, send a raw `Write` on the received read-only connection and
   expect the service's refusal.

9. **note**: INV-0430-3's in-QEMU check (`t_unix block`, `t_unix.c:439-463`) covers only plain `send` and `recv`. The
   paths that do work before a wait (`prepare_rights` after `room_in_stream`, the `held` message, `drain_tags`) are
   not run from a thread with `SCM_RIGHTS`. Add one blocking `sendmsg` with rights on a full packet queue and one
   blocking `recvmsg` with rights on a stream.

10. **note**: the host property test of INV-0412-2 (`tests/unix.rs:568`) sends and receives one `iovec` each time.
    The multi-piece paths (`write_pieces`, and the cut and `gather_range` of `recv_stream` across pieces) are not
    exercised "for any split". Draw the iovec count at random as well.

11. **note**: the wire record drops facts of the description. `O_SYNC` (`Desc::Object::sync`) is not carried, so the
    receiver's writes are not durable. A `Desc::Stream` with both ends (`unix_io.rs:231`) sends its read end alone,
    with no error. Record both in ADR-0430 item 3, or refuse the second with `EINVAL`.

12. **note**: `tests/shell/python-regr.script` says "the sendmsg/recvmsg cases ... unixpair.py loads the two classes".
    `tests/python/unixpair.py` loads one class (`BasicSocketPairTest`), which uses neither `sendmsg` nor `recvmsg`. The
    plan's acceptance ("CPython's `test_socket` Unix-pair cases") is met only in that narrow sense. CPython's own
    `SCM_RIGHTS` classes need `bind` (F7), and the descriptor path is checked only by `smoke.py`'s
    `send_fds`/`recv_fds`. Correct the comment, and say this in the task report.

## Checked and sound

- `Sys::channel_send` over the real kernel: `channel_write` does every check and allocation before it removes a handle
  (`syscall.rs:900-937`), refuses an end with queued handles or one of its own pair with `BadState` (`may_travel`), and
  `prepare`/`write` cannot be split by a peer's close, so a refused packet send changes nothing. The fake
  (`tests/kernel/mod.rs`) mirrors those refusals.
- `prepare_rights`/`prepare_one` close every connection `share` made on each error path, and `drop_prepared` closes
  them on a failed send. Moved fds are closed only after a successful send. A `dup` of a moved description is left
  dead (EOF, `EPIPE`). A description sent twice in one message, the socket the message travels on, and its peer are
  refused.
- `release` closes the book's tags and a `held` message (INV-0430-2 in one process). The decoders are total and strict
  over random bytes (INV-0430-1). A record with the wrong kind of handle gives the receiver nothing it did not already
  hold.
- Receiving: `MSG_CTRUNC` and closing beyond `max_fds`, `EMFILE` keeps a packet message, `MSG_CMSG_CLOEXEC`, and the
  cut at the next tag with bytes kept in `rest`.
- `fork` refuses a packet socket that has a channel (`ENOTSUP`) and keeps the stream's ancillary channel in the
  parent. `after_fork_in_parent` runs only on success.
- FFI: the `struct msghdr`/`cmsghdr` offsets match musl's LP64 layout. `scm_rights` stays inside `controllen`. The
  control write in `recvmsg` fits, because `max_fds` is derived from `controllen`. Each `unsafe` block has a
  `// SAFETY:` argument that holds; `put_unix_addr` has its `# Safety`.
- `invariants.md` Total 796 = 793 + ADR-0430's three; the `posix-surface-added` block names `t_unix`, and `t_unix.c`
  calls all seven names; `tests/unix.rs` installs `cap_memory!()`; `make check` (host paths, adr-check, test caps)
  passes; the workflow's cache key gained `regr-unix.txt` and ADR-0430; the `pyshim.c` stubs and asyncio patch 0003
  are gone, and `PYTHON_ABSENT` no longer lists `socketpair`.

## Verdict

Fix first: finding 1 (an error that loses descriptors silently) and finding 2. Findings 3 and 4 need the ADR notes and
the architect's decision on ADR-0430 item 7 before merge.
