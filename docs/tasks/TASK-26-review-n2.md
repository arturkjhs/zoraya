# TASK-26 review of N2

An independent review by the `opus-review` subagent, which did not write the code, of `git diff 7a39a57...work/n2`
at `8d0bfcf`. It covers the code around the diff in smoltcp 0.14.0 (`socket/icmp.rs` `recv_slice` and `dispatch`,
`storage/packet_buffer.rs` `dequeue_with`, `iface/interface/mod.rs` `socket_egress`, `udp.rs`, `ipv4.rs`, `tcp.rs`),
`crates/zoraya-settings/src/lib.rs` (`read`), `user/src/stack.rs` (`start`, `end_job`) and `user/src/bin/zosh.rs` (the
`settings` builtin). It was checked against proposed ADR-0281, ADR-0220 (items 1 to 7), ADR-0221 (items 3 to 5,
INV-0221-1 to -4), ADR-0222 (the network connection's requests), ADR-0229, ADR-0230, ADR-0006 (INV-0006-4), plan row N2
and CLAUDE.md's dependency rules.

The reviewer ran these commands on a `git archive` of `work/n2`, outside the repository:
- `cargo test --offline -p zoraya-abi -p zoraya-net -p zoraya-proto-net -p zoraya-describe -p zoraya-proto-nic`: 66,
  39, 4, 23 and 12 passed.
- `cargo clippy --offline -p zoraya-net -p zoraya-proto-net -p zoraya-abi -p zoraya-describe --all-targets -- -D
  warnings`: clean.
- Throwaway tests added to that copy only (not committed). Their results are quoted below.

No QEMU run. The fuzz target was not built (no nightly). Dispositions are left to the main model.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `crates/zoraya-net/src/stack.rs:379-449`, `:286-291` | An echo request that cannot leave (no route, or no ARP answer) stays at the head of the ICMP socket's 4-slot transmit buffer forever. A run of more than four requests never ends. After one such request, every later run never ends either, even to the gateway. While the run is stuck, `netd` spins at 100 % CPU, and `ping` answers `BUSY` to every connection until `netd` restarts. |
| 2 | error | `user/src/bin/zosh.rs:3951-3954`; `docs/adr/invariants.md:448` (INV-0229-1) | The `settings` builtin does not know any `net.*` key. `settings net.gateway 10.0.2.2` is refused as "no such setting", the same as `10.0.2`. The row is `checked`, but its command never runs `settings`. ADR-0281 item 4, `docs/design.md:1549` and `net --help` describe behaviour that does not exist. |
| 3 | warning | `crates/zoraya-net/src/stack.rs:278-281` | `poll` returns `Some(0)` whenever a frame waits, including when no transmit slot is free. A full channel to `ethd` (a normal transient state), or a driver that is slow to return `Sent`, puts `netd` in a busy loop. The loop also defeats the `WRITABLE` subscription made for this case. |
| 4 | warning | `crates/zoraya-net/src/service.rs:202-207`, `user/src/bin/netd.rs:215-219` | `netd` answers `ErrorReply` to every unknown ordinal, `ErrorReply`'s own ordinal included, and it attaches any channel end handed to `narrow`. A holder of `narrow` can make `netd` hold both ends of one connection, or a channel to another server. `netd` then answers its own error replies forever. |
| 5 | warning | `user/src/bin/netd.rs:326-337`, `user/src/bin/ethd.rs:217-227`; `invariants.md:417` (INV-0221-1) | The row is `checked: make test-net`, but the check is a report by each process about itself at start-up. `ethd`'s "one channel" does not show that the peer is `netd`. The outgoing lists of `init` (INV-0053-2's method, which the row names) are not looked at. |
| 6 | warning | `invariants.md:415` (INV-0220-3) | The row cites only `cargo test -p zoraya-abi`. The half that says "never a right its caller lacked" is tested in `zoraya-net` (`connection` and `service` tests). |
| 7 | warning | `docs/adr/0281-netd-answers-ping-and-dhcp.md:77` | The claim that "the guest answers ARP and nothing else unasked" is false. smoltcp answers UDP to closed ports with ICMP port unreachable, an unknown IP protocol with ICMP protocol unreachable, and a TCP segment to a closed port with RST. |
| 8 | warning | `fuzz/fuzz_targets/net_stack.rs:166-183`, `crates/zoraya-net/src/stack.rs:182-196` | Each DHCP-mode stack leaks its packet buffer and options (`Box::leak`), about 1.6 KB, and the fuzz target makes two stacks per input. Under libFuzzer, LeakSanitizer stops the run on the first such input, and memory grows with every input. The DHCP mode never reaches lease handling, because the fuzzed reply cannot carry the client's transaction id. The ping-reply path is not fuzzed. |
| 9 | warning | `Cargo.lock`, `docs/dependencies.md:22-28`, CLAUDE.md "Environment" | Registry crates break the written rule that `make test` needs no network once the toolchain is installed. The options are listed below, and the decision is the architect's. |
| 10 | note | `crates/zoraya-net/src/stack.rs:320-331` | A lease that `netd` refuses (prefix above 30, or an address that is not unicast) is dropped without an event or a console line, while smoltcp considers itself configured until the lease expires. Prefix 0 is accepted. DNS servers from the lease are not filtered. |
| 11 | note | `crates/zoraya-net/src/stack.rs:298-343` | A lease time of 0 makes the lease flap. The throwaway test measured 5 discover/request rounds and 8 events in 10 s, and `netd` prints a line for each event. |
| 12 | note | `crates/zoraya-net/src/stack.rs:23`, `:386`, `:409` | Every run uses one fixed identifier, and its sequence numbers restart at 0, so a late reply from the previous run counts as a reply of this run, with a false round-trip time. A reply larger than 256 bytes (`Truncated`) ends that turn's drain. |
| 13 | note | `user/src/bin/netd.rs:162-174`, `:217-219` | If `port_subscribe` fails for the new end, the connection is removed, but the caller still gets `Narrowed { OK }`. |
| 14 | note | `user/src/netclient.rs:32-58` | Every call uses transaction 1, and the reply's transaction is not checked. A reply that arrives after the 5 s wait is read by the next call on the same connection (`init`'s `control`, the shell's slot 9). |
| 15 | note | `crates/zoraya-abi/src/net.rs:191-210` | `valid_name` accepts names that `inet_aton` reads as addresses: `127.0.0.0x1`, `0x7f000001`. The matcher keeps them apart (INV-0220-4 holds), but N3's resolver and `libposix` must never parse a `Name` as a number. |
| 16 | note | `crates/zoraya-net/src/config.rs:54-56` | `is_unicast` does not know the prefix, so a ping to the subnet's broadcast address (`10.0.2.255`) is allowed. |
| 17 | note | `crates/zoraya-net/src/nic.rs:76-84` | `Faults` is described as "counted for `status`", but it is never reported. |
| 18 | note | `user/src/bin/init.rs` (`config_connection`), `user/src/bin/netd.rs:381-385` | Some rights are wider than the use needs. `netd`'s `/config` connection carries `CLONE`. `ethd` gets the frames object with `MemoryObject::RIGHTS` (`READ` and `WRITE`, so it could map it), although INV-0280-1 says that the driver reads no byte of it. |
| 19 | note | `user/src/netclient.rs:179-189`, `user/src/stack.rs:33-48` | When `netd` fails to spawn, the moved channel ends, the `/config` connection and the job leak. It happens once per boot, because `network_allowed` then turns false. |
| 20 | note | `invariants.md:450` (INV-0229-3) | The row's check asks for a write counter on the `/config` connection. The test argues the same point from the structure instead (the stack holds no file). The row could say so. |

## Findings

**1. error: an echo run that cannot send never ends, and blocks every later run.** `ping_step` queues a request only
when `socket.can_send()` holds (`stack.rs:406`). It sets the deadline only after the last request has been queued
(`:426-428`). smoltcp's ICMP `dispatch` uses `dequeue_with`, which leaves the packet in the buffer when `emit` fails.
`socket_egress` turns `NoRoute` or `NeighborPending` into `neighbor_missing`, and the packet is retried every second,
forever. The socket has 4 metadata slots (`stack.rs:156-157`).
- **Scenario A.** A connection with `reach` pings `10.0.2.99`, an on-link address that answers no ARP, with
  `count` 8. Four requests are queued, `can_send` turns false, the deadline is never set, and the run never ends.
- **Scenario B.** Under `make test`, the restricted lease has no router (ADR-0281 item 8). One `ping 8.8.8.8` with
  `count` 1 ends at its timeout, but its packet stays queued. Every later run, also to `10.0.2.2`, queues behind it and
  never finishes.

In both cases `Service::ping` stays `Some`, so every connection gets `BUSY` until `netd` restarts. The client's
`exchange` gives up after its wait, and the connection is left with a deferred reply that never comes.

The run is also a busy loop. Once the run is stuck, `next_at` lies in the past, so `poll` returns `Some(0)`
(`stack.rs:286-291`), and `netd`'s `port_wait` deadline is always "now".

Evidence from throwaway tests over the crate's own scripted peer:
- Count 8 to `10.0.2.99`: the run had not ended after 60 simulated seconds.
- Count 1 to `10.0.2.99`, then count 4 to `10.0.2.2`: the second run had not ended after 30 s (`PingDone` was
  `None`).
- A stuck run after 20 s: `poll` returned `Some(0)`.

The existing test `ping_to_nobody_ends_after_its_timeout_with_nothing_received` uses `count` 2, so it fits in the
buffer and passes.

**Fix:**
- Fix the deadline when the run starts: `start + (count - 1) * interval + timeout`.
- Count a request that cannot be queued when it is due as sent and lost, instead of waiting for `can_send`.
- When a run ends, empty the ICMP transmit buffer: remove the socket and add a freshly bound one.
- Make `poll` never return 0 for a run that has nothing due.
- Add tests for the three scenarios above. They must also assert that `poll`'s delay is never `Some(0)` while nothing
  can move.

**2. error: `settings` cannot set any `net.*` key, and INV-0229-1 is marked checked without a test of `settings`.**
`zosh` builds `declared` from its own description and `compd.cmd` only (`zosh.rs:3951-3954`). `netd.cmd` is included
nowhere but `zoraya-net` (`config.rs:13`). So `zoraya_settings::set` returns `Refusal::Unknown` for every `net.*`
name, and `settings` prints "no such setting".
- **Scenario.** A person follows `net --help` ("use settings net.NAME VALUE") or ADR-0229's own example
  (`settings net.dns +1.1.1.1`) and is told the setting does not exist. The only way to change the address is to edit
  `/config/settings` by hand.

`settings net.gateway 10.0.2` is indeed refused, as ADR-0281 item 4 and `design.md:1549` say, but for the wrong reason.
The row's command (`cargo test -p zoraya-describe -p zoraya-net`) tests the new setting types and `netd`'s reading,
never the `settings` command. Plan row N2 lists "the `net.*` declarations".

**Fix:**
- Extend `declared` with `netd.cmd`'s settings, as for `compd.cmd`.
- Add a shell-script or host test:
  - `settings net.gateway 10.0.2` is refused by type, and the file is unchanged.
  - `settings net.gateway 10.0.2.2` and `settings net.dns +1.1.1.1` are written.
  - `settings --check` reports a bad `net.*` line.
- Until that test exists, set the row back to `not yet applicable`.

**3. warning: `netd` spins when frames wait and nothing can be sent.** `LinkDevice::receive` refuses a frame without a
free transmit slot (`link.rs:109-111`). `Nic::can_send` is false when the channel to `ethd` is full and 64 transmits
wait, or when all 64 transmit slots are in flight. `poll` still returns `Some(0)` because `has_input()` is true.

The throwaway test (`room = 0`, one frame injected) got `Some(0)`. In `netd` this is a `port_wait` with a deadline of
"now", repeated until `ethd` drains its channel or returns slots. The `WRITABLE` subscription that `turn` makes for
exactly this case never gets to wait. The existing test `no_free_transmit_slot_leaves_received_frames_queued` does not
look at the delay.

**Fix:** return `Some(0)` only when `has_input() && can_send()`. Otherwise fall through to the timers, and let
`READABLE` and `WRITABLE` on the driver's channel wake the loop. Add the delay assertion to that test.

**4. warning: `netd` can be made to answer itself forever.** Two facts combine:
- `Service::handle` answers any ordinal other than 1, 2 and 6 with `ErrorReply` (`service.rs:202-207`). That includes
  `ERROR_ORDINAL`, so an `ErrorReply` is answered with an `ErrorReply`.
- `serve` attaches any channel end that comes with a valid `narrow` (`netd.rs:215-219`). Nothing stops it from being
  one of `netd`'s own connections' client ends, or a channel whose peer is another server.

Scenario, with a holder of `narrow` and two connections X (client end `cx`) and Y:
1. Write a message with an unknown ordinal on `cx`.
2. Let `netd`'s `ErrorReply` arrive in `cx`'s queue, and do not read it.
3. Send `Narrow` on Y with `cx` as the handle. A channel end moves with its queue.

`netd` now serves `cx` and reads its own `ErrorReply` as a request. It answers on `cx`, the answer arrives at X's
server end, and the cycle repeats: 16 messages per turn per side, forever, with no client left to stop it. A channel
to `fsd` that answers unknown ordinals the same way gives a loop between two services.

In N2 only `init` and the shell hold `narrow`. ADR-0220 item 5 lets programs pass narrowed connections on, so N3 widens
who can do this.

**Fix:** treat a message with `ERROR_ORDINAL` (any reply-shaped message) as malformed, and close the connection.
Never answer it. Add a test of an `ErrorReply` sent as a request.

**5. warning: INV-0221-1 is a self-report, and the row says `checked`.** `check_table` and `ethd`'s channel count run
inside the processes whose tables they describe. That does catch `init` handing over a device. But the row's check is
"enumerates both processes' handle tables … (INV-0053-2's method)", and INV-0053-2 looks at `init`'s outgoing lists.
`ethd`'s count also passes with one channel to any program. ADR-0221's new open-question note says this honestly; the
row does not.

**Fix:** check `start_network`'s entry lists (types and peers) in `t_netd`, or in a host test of the list builder, or
write `checked in part: make test-net (self-report at start-up)`.

**6. warning: INV-0220-3's row cites half its check.** The subset relation is tested in `zoraya-abi`. The rights half,
and `narrow` through the request, are tested in `zoraya-net`
(`narrow_never_gives_a_right_or_a_scope_the_caller_lacked`, `narrow_makes_a_connection_inside_its_parent_and_nothing_wider`).

**Fix:** `checked: cargo test -p zoraya-abi -p zoraya-net`.

**7. warning: what the guest answers unasked is misstated in a proposed ADR.** With `echo-reply` off, smoltcp still
answers three kinds of frame:
- UDP to an unbound port, with ICMP port unreachable (`iface/interface/udp.rs:60`).
- An unknown IP protocol, with ICMP protocol unreachable (`ipv4.rs:245`).
- TCP to a closed port, with RST (`tcp.rs:54`).

Each is one reply per received frame, and `BURST` bounds them, so there is no amplification. But ADR-0281 item 3
bases a decision on the claim.

**Fix:** correct item 3 and the design text: ARP, ICMP errors for UDP and IP, and RST for TCP; no echo replies.

**8. warning: the new fuzz target leaks per input, and it misses the lease and ping paths.** `Stack::new` leaks the
DHCP packet buffer, the hostname and the options (`stack.rs:182-196`), which is fine once per `netd` process. The fuzz
target makes two stacks per mode 1 or 2 input. Under libFuzzer on Linux, the leak check stops the run. Without it,
memory grows by about 1.6 KB per input, which is the failure CLAUDE.md's memory section is about.

Coverage gaps:
- `dhcp_frame` cannot carry the client's random transaction id, so smoltcp drops every reply after parsing it.
  `dhcp_events`, the lease checks and `DhcpRepr::parse` of the kept packet are never reached.
- Mode 4 never polls with a run active, so the echo-reply parsing in `ping_step` is not fuzzed.
- `fuzz/Cargo.lock` is stale.

**Fix:**
- Let the stack own its buffers: a lifetime on `Stack`, with `netd` passing static ones. Or reuse one stack per fuzz
  process.
- Answer the client's own DISCOVER and REQUEST with fuzzed options (take the transaction id from its frames, as the
  host tests' server does).
- Add a mode that runs a ping and feeds fuzzed ICMP.

**9. warning: the registry dependency (the deviation).** Facts:
- `Cargo.lock` has 17 registry packages with checksums. Six are not compiled: `defmt`, `defmt-macros`, `defmt-parser`,
  `proc-macro2`, `quote` and `syn`, plus `thiserror` and `unicode-ident`.
- `unicode-ident` is `(MIT OR Apache-2.0) AND Unicode-3.0`, which is outside CLAUDE.md's licence list. It is never
  built, but `cargo fetch` and `cargo vendor` take it.
- `byteorder` is `Unlicense OR MIT`, acceptable through MIT.
- The CI workflow is unchanged, so each job downloads the crates.
- `make test`'s cargo calls do not pass `--locked`, so a stale lock would be updated over the network silently.

Options:
- **(a) Accept, pinned by the lock's checksums** (ADR-0281 option A). The checksums already pin the content, and "pin
  by hash" adds nothing to it. Network is needed once per checkout. Cheapest, and it breaks the written rule.
- **(b) Accept with a fetch step.** Add `make fetch-crates` (`cargo fetch --locked`), parallel to `make fetch-ports`,
  so the one network step is explicit. Add `--locked` (or `CARGO_NET_OFFLINE=true` after the fetch) to `make test`'s
  cargo calls. Cache `~/.cargo/registry` in CI. Reword CLAUDE.md's rule to "after `make fetch-crates`" (the architect's
  edit).
- **(c) Vendor** (option B), with `cargo vendor --locked` and a source replacement. Fully offline, but the tree takes
  the uncompiled `defmt`, `syn` and `unicode-ident` sources (the Unicode-3.0 licence among them), and every check that
  walks the tree must skip `vendor/`.
- **(d) Vendor only smoltcp's real graph** (smoltcp, managed, byteorder, heapless, bitflags, cfg-if, hash32,
  stable_deref_trait) as path dependencies under a third-party directory, with smoltcp's `defmt` feature removed from
  its manifest. It is the smallest offline tree, but it is a local fork to maintain.

**Recommendation (not a decision): (b) now, (c) or (d) when the TLS provider brings `ring` and rustls.** That is
ADR-0281's own timing, plus `--locked` and an explicit fetch target, so that "needs network" is a named step and never
a silent side effect.

**10. note: a refused lease is silent.** When a `Configured` event fails `is_unicast` or `prefix <= 30`
(`stack.rs:322`), nothing is recorded or printed. smoltcp then considers itself configured and does not ask again
until the lease ends. Other gaps:
- Prefix 0 (everything on-link) is accepted.
- A router outside the subnet is accepted.
- DNS servers from the lease (0.0.0.0, a broadcast address) are not filtered. N3 will send queries to them.

**Fix:** print the refusal once, and filter the DNS list with `is_unicast` when the lease is taken.

**11. note: lease time 0.** smoltcp renews and expires at once. With the scripted server at lease 0, 10 s gave 5
discovers, 5 requests and 8 `Configured` or `Deconfigured` events. Each event prints a console line in `netd`. smoltcp's
own timers bound the rate (about one cycle per 2 s), so this is noise, not a loop. A hostile DHCP server on the
segment can keep the address flapping.

**Fix:** set a lower bound on the lease time `netd` accepts, or rate-limit the lines.

**12. note: echo replies of an earlier run count in this one.** `PING_IDENT` is fixed, and `seq_no` restarts at 0 in
each run (`stack.rs:23`, `:409`). A late reply for sequence k of run N, arriving after run N+1 has sent k, is counted
with a short round-trip time. Separately, `recv_slice` returns `Truncated` after it has dequeued the packet, and the
`while let Ok` then stops the turn's drain (`:386`).

**Fix:** a per-run identifier, or a sequence offset; and keep draining on `Truncated`.

**13. note: `narrow` says OK for a connection that failed to attach.** `attach_connection` closes the end and removes
the connection when `port_subscribe` fails (`netd.rs:168-171`), but `serve` writes the `OK` reply anyway (`:218-219`).

**Fix:** let `attach_connection` return whether it succeeded, and answer `UNAVAILABLE` when it did not.

**14. note: stale replies.** `exchange` reads the first message on the connection, and every request carries
transaction 1 (`netclient.rs:62`). After a timeout, the late reply stays queued and answers the next call. For
`narrow`, a stale `Narrowed { OK }` returns a client end whose server end `netd` refused or has not attached yet.

**Fix:** use rising transactions, and skip replies whose transaction differs.

**15. note: names that read as numbers.** `valid_name` refuses a name whose last label is all digits, but it accepts
`127.0.0.0x1` and `0x7f000001`, which `inet_aton` and WHATWG URL parsing read as `127.0.0.1`. The scope keeps them
typed, so the matcher is right. The danger is a later layer (the resolver of N3, `libposix`, a URL parser) that treats
a `Name` as a number.

**Fix:** refuse a last label that "ends in a number" in the WHATWG sense (digits, or `0x` followed by hex digits), or
record the rule for N3.

**16. note: directed broadcast.** `Stack::ping` refuses `255.255.255.255`, but it accepts the subnet's broadcast
address (`10.0.2.255/24`). Every host on the segment answers, and the replies are dropped (`from != target`).

**Fix:** refuse the network's broadcast and network addresses in `ping`.

**17. note: `Faults` is never reported.** `Nic::faults` counts bad entries and bad messages "for `status`"
(`nic.rs:76`). `status` carries only the link device's counters.

**Fix:** add the counters to `status`, or drop the claim.

**18. note: rights wider than needed.**
- `config_connection` asks for `READ | CLONE`. `netd` opens one file and closes the connection, so `READ` is enough.
- `netd` gives `ethd` the frames object with `MemoryObject::RIGHTS`, which include `READ` and `WRITE`. INV-0280-1
  ("the driver reads no byte of any frame slot") then rests only on the source check. The rights `dma_pin` needs would
  make it a kernel guarantee.

**19. note: leaks on a failed start.** `start` (`stack.rs:33-48`) leaks the job when the image lookup or the spawn
fails, and does not close moved entries. `start_network`'s failure branch (`netclient.rs:185-189`) therefore leaks
`to_netd`, `theirs`, the `/config` connection and a job. The pattern was there before N2. It happens once per boot,
because `network_allowed` then turns false.

**20. note: INV-0229-3's write counter.** The row asks for a file-write counter on the `/config` connection. The test
argues the same point from the structure instead: the stack holds no file, and `netd` closes its read-only connection
after reading. The argument is sound. The row should name it, so that a later reload (ADR-0229's open question) brings
the counter back.

## Checked and found sound
- **`Scope::decode` is total.** Every read goes through `Reader::take` with `checked_add`. The rule counts are at most
  64 and the length at most `MAX_SCOPE_BYTES`. The whole input must be consumed. `mask` is reached only after `len <=
  32` (`valid`'s short-circuit). Names are lower case, within DNS's limits, and do not end in an all-digit label.
  Prefixes with host bits set are refused. The fuzz invariant "encode of decode is identity" holds by construction,
  because the scope borrows its bytes.
- **`Ports::within` is exact.** A throwaway test compared it with a point model over 200,000 random pairs of 1 to 8
  overlapping, unordered ranges and found no difference.
- **`Target::within` follows ADR-0220 item 5 and the architect's wildcard fix.** A wildcard is not under its own
  suffix's name. `Any` is under nothing but `Any`. A name and an address are never under each other.
- **`Connection::decide`.** Unknown right bits are refused, the rights must be a subset, and the scope must be within
  one parent rule. A status-only connection can neither `narrow` nor `ping`. A root child can widen nothing.
- **The connections the shell holds and gives.**
  - `zosh` gives `STATUS` with the empty scope only to base-system commands that declare `needs net-status`.
  - Slot 9 goes only to the shell. No other start in `init` passes it, and `zosh` does not put it in any table.
  - The dead connection after a restart is handled: `narrow` fails at once on `PEER_CLOSED`, and `net` says the
    network is not available.
- **`Frames`' `unsafe`.** `OBJECT_SIZE` is `2 * SLOTS * SLOT_SIZE`, the slot and the length are checked twice, and the
  object is mapped for `OBJECT_SIZE` bytes. Both SAFETY comments are true. Volatile byte reads of a slot that `ethd`
  might write are the right form.
- **`Nic`.**
  - Slot ranges and states are checked, and a slot offered twice is taken once.
  - `arrived` is bounded by `SLOTS`, `transmit` by `MAX_BATCH`, `pump` by 64 messages, and message lengths by `BUFFER`.
  - A driver that breaks the protocol ends the service.
  - Frame bytes are copied out before smoltcp sees them.
- **`netd`'s request loop.**
  - Requests are at most 32 KiB, and 16 are served per connection per turn.
  - Every handle that comes with a request is closed unless it is a `narrow` channel.
  - A message that does not decode closes the connection.
  - A connection that leaves during a run loses only its reply: the run's owner is cleared in `remove`, so a reused id
    gets no stale reply.
- **`init`'s supervision.**
  - Ending both processes closes their process handles, and closing a handle removes its port subscriptions
    (INV-0006-4). The kill of the second process therefore does not come back as a second `KEY_NETD` or `KEY_ETHD`
    packet that would end the new network.
  - The count allows restarts 1 to 3 and stops at the fourth ending.
  - `control` is closed in `Network::end`, and the `/config` connection moves to `netd`, which drops it.
- **The settings and the lease time.** `config::read` handles bad lines once each and checks the condition between
  keys. `parse_address` and `parse_prefix` refuse signs, leading zeros and lengths above 32. A huge lease time is
  clamped below `NO_LEASE`.
- **The dependency rows.** `docs/dependencies.md` has a row for every compiled registry crate, with its version and
  licence. All of them are permissive.

## Dispositions

(Main model, after the review. Tests named were run in this session.)

1. **Fixed.** The run's deadline is fixed when it starts (`start + (count - 1) * 200 ms + timeout`); a request that cannot
   be queued takes its turn and is lost; when a run ends its ICMP socket is removed and a fresh one added, so nothing
   stuck survives it; each run has its own identifier (this also answers finding 12's late replies; `Truncated` no longer
   ends the drain); `poll` asks for at least 1 ms on account of a run, never 0. Host tests: count 8 to an unanswered
   address ends within its deadline with 8 sent and 0 received, a later run to the peer gets 4 of 4, and `poll` never
   returns a delay below 1 ms or `Some(0)` meanwhile. `t_netd` does the same (ping 10.0.2.99 x8, then 10.0.2.2).
2. **Fixed.** `zosh` declares `netd.cmd`'s settings beside its own and the compositor's. `tests/shell/net-settings.script`
   (a step of `make test-net`, with a disk) runs `settings`: `net.gateway 10.0.2`, `net.ip 10.0.2.15` and `net.address
   auto` are refused for their types, `net.gateway 10.0.2.2` and `net.dns +1.1.1.1` are written and read back. INV-0229-1's
   row names both checks.
3. **Fixed.** `poll` returns `Some(0)` only when a frame waits and a transmit slot is free; otherwise `netd` waits on the
   driver channel's `READABLE`/`WRITABLE` and the timers. Host test added (`room = 0`).
4. **Fixed.** A message with the error ordinal is a malformed request: the connection ends, and nothing answers it (test
   added). `netd` already refuses a `narrow` handle that is not a channel; a channel end that leads back to `netd`
   itself cannot loop any more because no reply-shaped message is ever answered. (`netd` cannot tell whose peer a channel
   end is; N3 should keep `narrow`ed ends with launchers.)
5. **Row made honest.** INV-0221-1 says `a self-report: each process looks at its own start-up table; no call lists another
   process's handles`. Not done: a check of `start_network`'s entry lists in a host test.
6. **Fixed.** INV-0220-3 cites `cargo test -p zoraya-abi -p zoraya-net`.
7. **Fixed (text); not turned off.** ADR-0281 item 3 lists what smoltcp answers unasked. smoltcp 0.14 has no switch for
   the ICMP unreachables or the TCP reset (a raw socket would suppress only the port-unreachable); turning them off would
   be a fork. One reply per frame, bounded by `BURST`.
8. **Fixed.** The fuzz target keeps one static and one DHCP stack for the process (no leak per input); mode 2 answers the
   client's own DISCOVER/REQUEST with the transaction id and hardware address and the input as options (it reaches the
   lease handling); mode 5 runs a ping and feeds fuzzed echo replies with the run's identifier. In the scratch driver
   (200,000 inputs, debug) a lease of 10.0.2.15 is reached and echo replies reach the stack. Still not run under
   libFuzzer; `fuzz/Cargo.lock` still not updated.
9. **Not decided (the architect's).** The options: (a) accept as is (the lock's checksums pin the content; network once
   per checkout); (b) accept with an explicit `make fetch-crates` (`cargo fetch --locked`), `--locked` in the build, a
   crates.io cache in CI, and the architect rewording CLAUDE.md's rule to "after `make fetch-crates`"; (c) `cargo vendor`
   with a source replacement (offline; takes the uncompiled `defmt`, `syn` and `unicode-ident` sources, the last
   `(MIT OR Apache-2.0) AND Unicode-3.0`, outside CLAUDE.md's licence list, and every check that walks the tree must skip
   `vendor/`); (d) vendor only smoltcp's real graph as path dependencies with its `defmt` feature removed (smallest, a
   local fork). The reviewer's recommendation: (b) now, (c) or (d) when the TLS provider arrives. Not done: `--locked` in
   the Makefile (`$(CARGO)` is the program, the flag goes after each subcommand in many places) and `make fetch-crates`.
10, 11, 13 (in part), 14, 15, 16, 17, 18, 19, 20. **Open notes, not changed**, except 13: `narrow` now answers
   `UNAVAILABLE` when the new end cannot be subscribed. The rest are recorded for N3: refused-lease message and DNS
   filtering (10), a lower bound on the lease time (11), rising transactions in `netclient` (14), names that read as
   numbers (15: N3's resolver and `libposix` must not parse a `Name` as a number), directed broadcast in `ping` (16),
   `Faults` not in `status` (17), `READ` only for `/config` and no `READ` on the frames object (18), leaks on a failed
   start (19), and INV-0229-3's row naming the structural argument instead of a write counter (20).
