# TASK-26 N3: independent review

Branch `work/n3` (32a4c03) against its base on `work/q4` (merge base 5aa3b09), `git diff 5aa3b09 work/n3`. Read against
ADR-0101, ADR-0220 to ADR-0230 (mainly 0222, 0223, 0224, 0227), proposed ADR-0286, plan row N3 and the N3 section of
`docs/tasks/TASK-26-report.md`.

Run here: `cargo test -p zoraya-dns -p zoraya-http -p zoraya-net -p zoraya-proto-net` (16, 17, 59, 5 passed) and
`cargo clippy -p zoraya-dns -p zoraya-http -p zoraya-net -p zoraya-proto-net -p zoraya-test-httpd --all-targets -- -D
warnings` (clean). Finding 1 was reproduced with a scratch host test in an exported copy of the branch (not committed;
the test is given in the finding). No QEMU run.

Counts: 3 errors, 5 warnings, 8 notes.

## Errors

### 1. A reset after the peer's FIN leaves the connection and its socket in `netd` for ever (error)

**Where:** `crates/zoraya-net/src/conn.rs:307-339` (the outbound loop stops at `tcp_may_send`) and `:403-413` (the
end needs `out_eof && in_eof`); `crates/zoraya-net/src/tcp.rs:182-194` (`Finished` comes before `InvalidState`).

**What:** once the peer's FIN is seen, `in_eof` is set and the inbound loop is skipped for good. If the peer then
resets (smoltcp: CloseWait to Closed, `rx_fin_received` stays true), `recv` would still say `Finished` and is not called
anyway; `may_send` is false in Closed, so the outbound loop breaks before it reads the stream, so `out_eof` never becomes
true, even when the client closes its write end or exits. Nothing in `copy` looks at the socket's state without both
flags. The connection sends no `Reset`, closes neither stream end, keeps its `Conns` slot, and its socket counts against
`MAX_TCP` with no end. INV-0222-2 ("the peer's reset ... delivers `reset`") does not hold on this path, and the tests do
not cover it.

**Scenario:** a server answers, closes (FIN) and then resets: a server that closes with `SO_LINGER` 0, or one that
receives more bytes after its close (a client still sending a request body, or pipelining). A hostile server does it on
purpose: eight `fetch`es to it, and every TCP connection on the machine is refused with `OverLimit` until `netd`
restarts. A client that writes after the peer's FIN blocks in `stream_write` for ever once the ring is full. Reproduced:
a scripted peer that greets, closes and aborts when the late byte arrives (`GreetThenReset` added to `Script` in
`conn_tests.rs`); after the client's late write the peer's socket is `Listen`, ours `Closed`; then `fake.close_write()`
and 120 simulated seconds of turns: `conns=1 tcp=1 reset_msg=false`, and the test fails on `conns.count() == 0`.

**Fix:** in `copy`, after the two loops, when `tcp_phase(tcp) == Ended` and the connection is not done: if `!out_eof`
(the client's bytes can no longer go out), send `Reset { PEER }` (or `ABORTED`), close inbound, release, done. The
normal end (`out_eof && in_eof && Ended`) stays as it is. Add the scripted peer above as a test, with both orders (the
client closes before and after the reset).

### 2. A `url` operand from a variable or a substitution designates: ADR-0224 item 2 is broken, and ADR-0286 states the deviation wrongly (error)

**Where:** `user/src/bin/zosh.rs:238-266` (`net_for`; line 243 drops `Value::literal`);
`docs/adr/0286-n3-fetch-over-http-where-the-network-adrs-are-silent.md:79-86` and `:116`;
`crates/zoraya-describe/src/lib.rs:77-79`; `user/src/bin/fetch.rs:10-11` (they cite "ADR-0224 item 3").

**What:** designation is ADR-0224 item 2, not item 3 (item 3 is `grants` output). Item 2, accepted, makes a *literal*
`url` operand of a base-system command designate with no `+net` rule; the bound it sets is on an operand that "came from
a variable or a substitution": that one designates only inside a `+net` rule on the line or in the room, "else the line is
refused before it runs" (`zosh: http://other.example/: from $u, outside the line's network grants`). INV-0224-1 says
the same. `net_for` maps `exec.args` to their text and never looks at `literal`, so a value designates. ADR-0286 item 5
says the opposite of the ADR's shape: that every designation needs a `+net` rule and N3 skipped the condition. So the
deviation is both described wrongly and wider than it needs to be. The words N3 left out are not needed for the case
ADR-0224 allows. The claim "not weaker than today's state" is wrong for the value case.

**Exposure, checked:** rooms and the panel's shells get no `NET` slot (zosh takes slot 9 only if it is a channel; only
`init` gives the console shell its root connection), and `run-qemu.sh` refuses `--net` with `--gui`, so today only the
console shell, which holds the root connection, designates. There, a line run at home reaches any host and port that
data chooses: `fetch (cat links.txt)`, `for u in (cat list) { fetch $u }`, or a sourced script or a function that builds
the URL from a page it fetched before. Under `make run-net` (no `restrict`) that includes `http://10.0.2.2:PORT/`, which
is every service on the host's 127.0.0.1. This is the case ADR-0224 item 2 (and ADR-0153 rule 4 for paths, which
`grants.rs:199-209` does enforce) exists for.

**Fix:** in `net_for`, find the `Value` the parsed `url` operand came from. If it is not literal, refuse the line before it
runs, with the ADR's message (no `+net` words exist, so a value is always outside the line's grants). Do not just run
it without a connection. Move `net_for` into `zoraya-shell` (next to `grants::plan`) so it is host-tested: literal,
`$u`, substitution, function parameter, non-base-system description, unparsable URL. Correct ADR-0286 item 5, its open
question and the citations to "ADR-0224 item 2": a literal operand designates as accepted; what is not built is
`+net`/`+listen`, the value-inside-a-rule case (refused until the words exist), `grants` output, REPORT and the hint.

### 3. `fetch` exits 0 with a cut page when a close-delimited body ends in a reset (error)

**Where:** `user/src/bin/fetch.rs:207-224`.

**What:** `parser.finish()` returns `Ok` in `State::UntilClose` (a body without a length ends with the connection).
`reset` (from `was_reset`) is only read inside `if let Err(e) = result`. So when the connection is reset in the middle
of a body framed by the close, `fetch` prints or writes the part that arrived, says nothing, and exits 0. Here the reset
is exactly what tells a cut body from a complete one, and `netd` provides it (`Reset` before end of file, ADR-0286 item
3). The same applies to `Err(_) => break` on a `stream_read` error (line ~180).

**Scenario:** `fetch -o big.bin http://host/file` from an HTTP/1.0-style server, cut by a middlebox or an off-path RST:
exit status 0 and a truncated `big.bin`. The test `/reset` page uses `Content-Length`, so the test suite does not see this.

**Fix:** if `reset` (or a read error), the result is `Err` whatever the framing: "the connection was reset". Add a
`/reset-close/N` page to `test-httpd` (no length, some body, then a reset) and a script step. Note that the existing
`/reset` step expects "ended before the page did", not "was reset". Through SLIRP the guest may never see the RST, which
says that path is not tested in QEMU (note 16).

## Warnings

### 4. The rows of ADR-0224's invariants are not true and its open questions do not say why (warning)

**Where:** `docs/adr/invariants.md` rows INV-0224-1, INV-0224-2 (unchanged: `not yet applicable (stage 8)`).

**What:** N3 is the first part that works in ADR-0224's area (a `url` source of grants, in stage 8). Under ADR-0001 rule 6
and CLAUDE.md, it implements the checks or records in ADR-0224's open questions why it cannot. INV-0224-1 is partly
implementable now (literal vs value, base-system vs not: finding 2) and is broken today. INV-0224-2 is false today:
`grants fetch http://x/` prints no network rule while the command gets one. The report says "rows unchanged", but no
open-question note was added to ADR-0224.

**Fix:** after finding 2, make INV-0224-1's host test for the cases that exist, and add the rule-6 note to ADR-0224's
open questions for the rest (the words, `grants`, REPORT).

### 5. ADR-0286 says the DNS step is skipped when SLIRP's forwarder is unreachable, and it is not (warning)

**Where:** `docs/adr/0286-*.md:119`; `tests/shell/net-fetch.script` (the `localhost` and `nothing.invalid` steps); the
`test-net-fetch` recipe in `Makefile`.

**What:** nothing skips those steps. `localhost` gives "points at this machine's own network" only if the host's
resolver, as libslirp forwards to it, answers `localhost`. On a CI host whose `resolv.conf` server does not, the step
fails ("the name has no address"). With `restrict` off, `nothing.invalid` is forwarded to the host's upstream resolver,
so `make test` now sends a query off the machine when it can (CLAUDE.md: `make test` must not *need* the network; it
still passes offline only by the 8 s give-up).

**Fix:** correct the ADR text. Either put the two names in a separate step that is skipped when a probe fails, or answer
them from a scripted DNS (a `--dns` in test-httpd behind `guestfwd`, or `net.dns` pointing at a host-side responder).

### 6. `fetch -o FILE` truncates the file before it connects (warning)

**Where:** `user/src/bin/fetch.rs:116-134` (`CREATE | TRUNCATE` before `connect`).

**What:** an existing `FILE` is emptied at once. A refused connection, a 404 (whose body is not written) or a cut transfer
leaves it empty or partial, with exit status 1. `fetch -o notes.txt http://host/notes.txt` with the host down destroys
`notes.txt`. The comment ("a file that cannot be made costs no connection") is a reason to *check* early, not to truncate
early.

**Fix:** open without `TRUNCATE` (or check the grant) early, and truncate on the first body byte of a 2xx. Better: write a
temporary name in the same directory and rename at the end. Say in `fetch.cmd` what happens to the file on failure.

### 7. A fixed port for the test server collides between worktrees (warning)

**Where:** `Makefile:278` (`NET_HTTP_PORT := 47080`); `tools/test-httpd/src/main.rs:247-255` (`--ready-file` writes the
port it was given, so port 0 cannot be used).

**What:** this host runs `make test-par`/`test-changed` in several worktrees at once. A second `test-net-fetch` fails with
"port taken", or its guest is served by the other run's server, which can disappear in the middle when that run ends.

**Fix:** bind port 0, have `--ready-file` write `listener.local_addr()?.port()`, and have the recipe read it (and wait
on the file instead of polling with perl).

### 8. `net_for` is shell logic in the binary, with no host test (warning)

**Where:** `user/src/bin/zosh.rs:238-266`.

**What:** this decides a security boundary (what a command may reach), from parsing to the scope's encoding, and only the
QEMU script exercises it, for literal addresses and one `net.hosts` name. CLAUDE.md puts logic that needs no hardware
in a host-tested crate. Folded into the fix of finding 2.

## Notes

### 9. The global bounds belong to whoever comes first (note)

`MAX_TCP` 8 and `MAX_CONNS` 16 are global, open connections have no idle limit (`set_timeout` only covers unacknowledged
data), and a client that holds a connection open and idle holds a socket for as long as it lives. Any program with any
`reach` rule can therefore deny TCP to every other room. The report records this as Q3's account. Say it in
`docs/design.md`'s known limitations too, next to finding 1, which makes it reachable by a remote server.

### 10. "One query in flight per name" is not what the resolver does (note)

`crates/zoraya-net/src/dns.rs:144-186`: two connections to the same name send two queries with two identifiers (which
also doubles an off-path guesser's chances). ADR-0286 item 4 says one per name. Either join the second connection to
the first query, or correct the text.

### 11. A DNS datagram larger than the buffer stops reading for the turn (note)

`crates/zoraya-net/src/dns.rs:208-210`: smoltcp's `recv_slice` dequeues an oversized datagram and returns `Truncated`.
The `let Ok .. else break` then leaves later datagrams (possibly the real answer) until the next turn (up to the next
attempt timer, 1 to 4 s), and does not count it in `rejected`. Use `continue` and count it. A related point: the receive
queue is 8 datagrams on a guessable port, so a sender on the link can fill it with junk.

### 12. A label that contains a dot compares equal to two labels (note)

`crates/zoraya-dns/src/lib.rs:157-170`: `push_label` copies label bytes as they are, so the one-label name
`test.zoraya` (a byte 0x2e inside the label) matches the asked name `test.zoraya`, as owner or as question. It is only the
server's own word about the name asked, so it is not an escalation. Refusing `.` (and bytes outside letters, digits, `-`
and `_`) inside a label in `read_name` makes the equality mean what it says.

### 13. Page bytes go to the terminal as they are (note)

`user/src/bin/fetch.rs:197` (`write_output(bytes)`): a hostile page can send escape sequences to `term`, as `cat` of a
downloaded file can. Not new, but `fetch` is the first command whose output comes straight from the network. Worth a
line in `fetch.cmd`, or a later decision on control bytes from untrusted output.

### 14. `--net-open` is safe as built, but wider than its comment (note)

`tools/run-qemu.sh:36-42, 226-228, 278`: only `--net-open` removes `restrict=on` in test mode, and only the
`test-net-fetch` recipe passes it, so no default path loses `restrict`. `--net-open` without `--test` is accepted and
does nothing (the interactive mode was already open), contrary to its "with --net --test". With `restrict` off, the
test guest can reach every service on the host's 127.0.0.1 through 10.0.2.2, the host's LAN, and the Internet: "nothing
else is reached" is a property of the script, not a bound. That is acceptable for a guest that runs only our image. The
server listens on 127.0.0.1 only, has 8 connections, timeouts, and a 300 s lifetime, and is killed by the recipe. Any
local user can read generated bytes from it for those 300 s, which is harmless.

### 15. A connect that finds no slot is still told "Connecting OK" first (note)

`user/src/bin/netd.rs` (`Action::Connect` arm): `Connecting { OK }` is written before `free_slot`, and `Failed {
OverLimit }` follows on the connection channel. That is consistent with ADR-0286 item 1 (the scope channel's reply means
"accepted for work"), but it costs the client a round trip and a wait. Fine as built; noted so that nobody "fixes" one
side only.

### 16. The reset path is not exercised in QEMU (note)

The `/reset` script step expects "the connection ended before the page did", which suggests SLIRP turns the host's
RST into a FIN (or `was_reset` is false). Either way, `Reset` reaching `fetch` is covered only by the host tests. Say so
in the report's test section.

## Checked and found sound

- `zoraya-dns`: total. Pointers only backwards and at most 8 per name, so `read_name` ends in at most nine passes over
  the message. Reserved label types are refused. Names are capped at 253 in a fixed buffer. `rdlength` is checked against
  the message before use. An A record must have 4 bytes. The answer count is capped at 32 and a count beyond the data
  is `Malformed`. A CNAME's name must end inside its rdata, and chains are bounded by the 32 records. QR, opcode and
  exactly our question are required. TTL is the minimum of the kept records and at most 3,600 s; TTL 0 is not served
  from the cache (`retain` with `>`). The resolver takes answers only from a configured server's port 53 with an
  identifier in flight, and keeps waiting after a forged or malformed one. The identifier and port are guessable, as
  stated (ADR-0227 / N5).
- `zoraya-http`: URL (no userinfo, ports 1 to 65,535 with at most 5 digits, IPv6 literals and numeric non-quad hosts
  refused, the path limited to 0x21..0x7e, so no CR, LF or space reaches the request; 2,048 bytes). The head is capped at
  8 KiB and 64 fields. Bare LF, CR, folding and whitespace before the colon are refused. Both framings, differing
  lengths and non-digit lengths are refused, and so are codings other than `chunked` or `chunked` twice. Chunk sizes
  have at most 15 hex digits and 256-byte lines, extensions are ignored, and trailers are bounded by `max_head`. `deliver`
  checks the cap before it hands bytes on, so INV-0286-2 holds for every framing. 1xx responses are bounded (4); 101,
  204 and 304 have no body. Redirects are not followed, so no other host and no downgrade are possible.
- `Connection::decide` for `Connect`: `reach`, port not 0, and a name rule for a name and an address rule for an address
  (INV-0223-2). The service checks UTF-8 and `valid_name`, and refuses address 0. The decision comes before any query
  (INV-0223-1's decision part).
- Local-address rule: applied to every resolved candidate, cache hits included, and `net.hosts` is exempt as ADR-0223
  item 4 says. `tcp_open` refuses non-unicast.
- The pump: backpressure through smoltcp's closures with no staging. Subscriptions exist only while waited for, and are
  unsubscribed on change, on `close_inbound` and on drop, so a ready but unwanted stream does not wake the loop. Every
  open connection is stepped every turn, so a missed subscription only delays (except finding 1). The rounds per
  direction are bounded, and an exhausted round always leaves an ACK or a window update to come.
- Handle ownership: wrong handle types close all of them. Refusals close the inbound end and drop the rest. `Failed` and
  `Reset` are written before the ends are closed (INV-0286-1), and the slot's subscriptions are removed before the slot
  can be reused. A dead client is noticed through `Gone` (an empty `write_inbound` every turn) or a failed `notify`.
- `KEY_OUT`/`KEY_IN` do not overlap `KEY_CONNECTION` (at most 64 service connections).
- No new `unsafe` (`zoraya-dns` and `zoraya-http` are `forbid(unsafe_code)`). `Frames::read_slot`'s volatile copy is
  from N2 and costs only time.
- `--net-open` cannot leave `restrict` off on any path other than `test-net-fetch` (note 14).

## Recommendation

Hold the merge for findings 1 to 3. Each is a small change with a test. With finding 2 fixed, N3's designation is within
ADR-0224 item 2 as accepted (literal operands of base-system commands). What remains missing (the `+net`/`+listen`
words, `grants` output, REPORT and the hint) can then ship as a recorded limitation with N3b, rather than as a deviation
to decide. ADR-0286 item 5 and its first open question need rewriting either way, because they put the condition on the
wrong case.
