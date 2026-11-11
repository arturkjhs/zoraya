# TASK-26 review of N1

An independent review by the `opus-review` subagent, which did not write the code, of
`git diff 9a74448...work/n1` at `8e60dce`, with the code around it in `crates/zoraya-virtio/src/queue.rs`
(`begin`, `Queue::offer`, `notify`, `take`), `kernel/src/syscall.rs` (`dma_pin`, `dma_unpin`), `kernel/src/device.rs`
(`release_quarantine`), `user/src/gui.rs` (`start_driver`, `Running::end`) and `user/src/stack.rs` (`end_job`). It was
checked against proposed ADR-0280, ADR-0221 (items 1, 2, 4, INV-0221-1 and -2), ADR-0230 (INV-0230-1 to -4), ADR-0054
and ADR-0070, plan row N1 (`docs/research/network/plan.md`) and the report's N1 section. The reviewer ran
`cargo test -p zoraya-virtio-net -p zoraya-proto-nic` (24 and 12 passed) and
`cargo clippy -p zoraya-virtio-net -p zoraya-proto-nic --all-targets -- -D warnings` (clean). An offline
`cargo check` of the fuzz workspace failed because `libfuzzer-sys` is not in the local registry, so the fuzz target
was not built here either (`fuzz/Cargo.lock` was restored unchanged). No QEMU run. Dispositions are left to the main
model.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `user/src/bin/ethd.rs:131-140`, `:224-233`, `:280-290` | A push the channel cannot take is dropped for good: a lost `Sent` leaks a transmit slot forever, a lost `LinkChanged` or reply is never resent. |
| 2 | error | `docs/adr/invariants.md:511` (INV-0280-1) | The row says `checked: make check`, but nothing in `make check` checks that `ethd` maps only its queue memory. |
| 3 | warning | `crates/zoraya-virtio-net/src/lib.rs:268-270`, `user/src/bin/ethd.rs:216-233`, ADR-0280 recommendation 4 | A transmit refused because the device still holds the slot is reported as `Sent`, which looks exactly like a completion: the client frees a slot that is in flight, and later gets a second `Sent` for it. |
| 4 | warning | `user/src/bin/t_net.rs:362-365` | The test closes the channel and then kills `ethd` at once. Which ending runs (the clean reset and unpin, or the quarantine) depends on timing, and the test checks neither. |
| 5 | warning | `user/src/bin/ethd.rs:178-292` | The driver's dispatch logic lives in the binary, not in a host-tested crate. INV-0221-2's "every message from netd" half rests on it, and no host test or fuzz input reaches it. |
| 6 | warning | `Makefile:234-235` | Any failure of the probe, not only a QEMU without user networking, turns `test-net` into a silent pass, also in CI. `make test`'s summary does not list the skip (INV-0230-3). |
| 7 | warning | `user/src/bin/t_net.rs:322-325`, plan row N1 | The plan's acceptance ("answers an ARP request for `10.0.2.15`") is never exercised in QEMU. `t_net` passes with `answered == 0`. |
| 8 | note | `crates/zoraya-virtio-net/src/lib.rs:334-354` | The receive loop of `poll` has no bound: a device that keeps completing runts keeps `ethd` in one `poll`. |
| 9 | note | `crates/zoraya-virtio-net/src/lib.rs:131-133`, `:184-186` | The 16-bit `status` field is read with two byte accesses, and the 6-byte MAC is read without `ConfigGeneration`. |
| 10 | note | `user/src/bin/ethd.rs:11-15`, report "Buffers under DMA" | The lifetime story says that the next `ethd` resets the device through `Net::new`. In fact the kernel's `dma_release_quarantine` does the reset (ADR-0070). |
| 11 | note | `user/src/bin/ethd.rs:274-278`, ADR-0280 recommendation 4 | A `Received` that is undone is not counted in `dropped`, although the ADR says it is. |
| 12 | note | `user/src/bin/ethd.rs:199-213`, ADR-0280 recommendation 5 | An `Attach` that does not decode is answered `BAD_OBJECT`, but every other message that does not decode ends the driver. |
| 13 | note | `crates/zoraya-virtio-net/src/lib.rs:211-212` | `post` marks a slot `Posted` before `offer`: if `offer` failed, the slot would be lost. It cannot fail today. |
| 14 | note | `user/src/bin/t_net.rs:284-287` | The test assumes that the next push after the hostile transmits is `Sent`. An unsolicited frame from SLIRP makes it panic. |
| 15 | note | `fuzz/fuzz_targets/net_rx.rs`, `fuzz/Cargo.lock` | The target has never been built in the repository, its lock file is stale, and it does not cover the configuration and link path, truthful transmit completions or `ethd`'s dispatch. |
| 16 | note | `tools/test-map.txt:23-25`, `user/src/bin/init.rs:331-341` | Changes to `kernel/` (DMA, devices) and `crates/zoraya-rt/` do not select `test-net`. No step boots the normal kernel with `--net`, so the `ethd` that `init` starts is never run. |
| 17 | note | `crates/zoraya-virtio-net/src/lib.rs:252`, `:294` | One `QUEUE_NOTIFY` per released or transmitted slot, so a batch of 64 costs up to 64 exits. |

## Findings

**1. error: pushes lost on a full channel are never recovered.** `Ethd::send` returns `false` on any write error
other than `PeerClosed`. That includes `WouldBlock`, when the client's queue holds `MAX_QUEUED` (64) messages. The
callers handle the failure as follows:
- **`Received`.** The slots are undone (`ethd.rs:274-278`).
- **`Sent` for completions (`ethd.rs:280-287`).** The result is ignored, and `Net::poll` has already set those
  slots to `Tx::Free`.
- **`Sent` for refusals (`:224-233`).** The result is ignored.
- **`LinkChanged` (`:288-290`).** The result is ignored, and `Net::link_changed` has already stored the new state,
  so it is never sent again.
- **`InfoReply` and `Done`.** The result is ignored.

*Scenario:* `netd` (N2) is busy for a while, for example in a TLS handshake. Every device interrupt pushes up to two
messages, so about 32 interrupts fill the queue. From then on every `Sent` is dropped. The driver considers those
transmit slots free, but `netd` waits for their `Sent`, which never comes. Once all 64 are lost, `netd` can never
transmit again, and no error is raised anywhere. This breaks ADR-0280 driver 4 ("the client's accounting of slots
never stalls") and DM-047 ("never a hang"). The protocol gives the client no way to recover: it cannot ask which
slots are free. Likewise, a client that sends `Info` while the queue is full waits for its reply forever.

*Fix:* keep what has not been delivered in the driver and retry it:
- a bitmap of transmit slots that are finished but not yet reported, a "link changed" flag, and the pending reply;
- `WRITABLE` (or the next wake) triggers a resend, and slots are folded into the next `Sent`.

Alternatively, make `Info` also return a snapshot of the free transmit slots, so that the client can resynchronise.
Add a host test (with the dispatch moved into a crate, finding 5) in which the channel refuses writes and then
accepts them again, and assert that every slot is reported exactly once.

**2. error: INV-0280-1's status is not true.** CLAUDE.md requires the Status column to name a command that checks
the invariant. The row's check has three parts, and only one is mechanical:
- **"`#![forbid(unsafe_code)]`".** Clippy does verify this.
- **"Takes no `Shared` view of the frames object".** This holds by construction: `Net` holds one `S`, and the
  frames are a `u64`. Nothing checks it, and a later `Net<R, S, F: Shared>` would pass `make check`.
- **"`ethd` maps only its queue memory".** Nothing checks this. `ethd.rs` could `memory_map` the `Attach` handle
  before closing it, and `make check` and every test would still pass.

*Fix:* either add a check, or change the row to `not yet applicable` or to a partial `checked` that says what is not
covered. Two possible checks:
- a `check-*` script that allows exactly one `memory_map` call in `user/src/bin/ethd.rs`, the one on the queue
  object, and fails on any `memory_map` or `Shared` implementation that takes the attached handle;
- an in-QEMU assertion in `t_net` that `ethd`'s address space has no mapping of the frames object, if the kernel can
  enumerate mappings.

**3. warning: a refused transmit is indistinguishable from a finished one.** `Net::transmit` refuses a slot in
`Tx::Queued` with `WrongState`, and `ethd` reports every refusal as `Sent`. *Scenario:*
1. `netd` transmits slot 70.
2. Because of a retransmit bug or a race, it transmits 70 again before the first `Sent`. The second request is
   refused, and `Sent{70}` arrives at once.
3. `netd` marks 70 free and writes the next frame into it while the device is still reading the first, so the wire
   carries mixed bytes.
4. The device then completes, and a second `Sent{70}` arrives. `netd` frees the slot again, possibly while a third
   frame is queued in it.

The client's accounting doubles, which is the opposite of what recommendation 4 wants. *Fix:* report refusals in a
separate field (`Sent { slots, refused }`), or as a separate message. Do not report a `WrongState` refusal as
released at all, because the slot is still with the device and will be reported when it completes. ADR-0280's open
question ("whether a refused transmit should carry a reason") should become part of the recommendation, because the
reason changes what the client may do with the slot.

**4. warning: the end of the buffer lifetime is not tested, and the test races itself.** At `t_net.rs:362-365` the
test closes `end` and the memory handle and calls `running.end()`, which is `end_job`, which is `job_kill` followed by
`wait_exit`. If `ethd` has not yet read `PEER_CLOSED`, it is killed with its two pins live and RX buffers posted. The
device stays at `DRIVER_OK` and keeps writing frames into quarantined memory for the rest of the boot. If `ethd` reads
`PEER_CLOSED` first, it resets the device and unpins. The test does not say which ending ran, and the comment ("the
driver resets the device and ends") claims something that is not checked. The report lists the kill test as open
for N2, but the clean path can be checked now.

*Fix:*
1. After closing `end`, wait for `ethd`'s exit with a deadline before any kill, and assert its exit status.
2. `t_net` holds the device handle from `scan`. Call `dma_release_quarantine(device)` and assert `Ok` (nothing
   mapped, no live pin, nothing quarantined).
3. Assert the free-frame count before and after.

A second round could kill `ethd` with frames in flight, check that frames stay quarantined (INV-0054-3), then release
and compare.

**5. warning: the dispatch is outside a host-tested crate.** `Ethd::handle`, `serve` and `device_event` decide the
following:
- which messages end the driver;
- which handles are closed (only `Attach` keeps one);
- how refusals are reported;
- that a lost `Received` is undone;
- the order of acknowledgement, poll and `interrupt_ack`.

This is logic that does not need hardware. It is also the client-facing half of INV-0221-2. CLAUDE.md requires such
logic to live in a `no_std` crate with host tests. As things stand, it is reached only by `t_net`'s few hostile
messages. `net_rx.rs` calls `Net` directly, and `zoraya-proto-nic`'s `exercise` proves only that the decoders are
total. *Fix:* move the dispatch into `zoraya-virtio-net`, or into a small `zoraya-ethd` crate, generic over a channel
trait (write that can fail, read, close a handle) and a pin trait. Then host-test the hostile client and the
full-channel cases (finding 1), and make the fuzz target feed it arbitrary message bytes.

**6. warning: the skip can hide a broken step.** `make test-net` skips, and exits 0, whenever
`run-qemu.sh --accel tcg --net --dumpdtb` fails, for any reason. Examples are a typo in the new `-device` line, a
QEMU whose `virtio-mmio-bus.16` is named differently, or a missing `$(BUILD)`. CI then shows the network step as
green without running it. INV-0230-3 also requires `make test`'s summary to list the skip, and an `echo` inside a
passing recipe does not. *Fix:* probe precisely, for example `qemu-system-aarch64 -netdev help | grep -qx user`
through a `run-qemu.sh --probe-net`, and let any other probe failure fail the step. In CI (`CI=true`), never skip.

**7. warning: N1's acceptance is not met as the plan states it.** Plan row N1 asks for `t_net` to answer an ARP
request for `10.0.2.15` by hand, and for the pcap to show the reply. In the branch, `t_net` sends a who-has and reads
SLIRP's reply. That proves both DMA paths, but the answer path (`arp::answer` → a transmit through slot 65) runs only
in host tests. `answered` is printed but not asserted. The report gives the reason (SLIRP asks only when the host
connects) and leaves it for N2, but this changes the stage's acceptance and needs the architect's agreement. It can
be provoked in N1 without a guest TCP stack:
1. Add `hostfwd=tcp:127.0.0.1:PORT-10.0.2.15:PORT` to the `--test` netdev.
2. Before `t_net`'s first transmit, have the runner open a connection to `127.0.0.1:PORT`. SLIRP must then ARP for
   the guest in order to send the SYN. `restrict=on` does not block `hostfwd`.
3. Assert `answered >= 1` and grep the pcap for an ARP reply from `52:54:00:12:34:56`.

**8. note: `poll` is unbounded.** Each runt is reposted at once, and the loop reads the used index again, so a device
that completes every reposted buffer with a runt keeps `ethd` in a single `poll`. In that time `ethd` serves no
client, and `ethd` cannot tell such a device from a broken one. QEMU never sends a frame under 14 bytes. *Fix:*
take at most `SLOTS` entries per call, and let the level-triggered interrupt bring the rest.

**9. note: configuration access widths.** Virtio 1.2, section 4.2.2.2, requires 16-bit aligned accesses for 16-bit
fields of the device configuration. `read_link` reads `status` as two bytes. `mac` spans several accesses and is read
without checking `ConfigGeneration` (section 2.5.1). QEMU tolerates both. *Fix:* add `read16` to `Registers`, and
read `mac` in a generation loop.

**10. note: who resets the device.** `ethd.rs:11-15` and the report say that the quarantine lasts "until the next
driver resets the device and calls `dma_release_quarantine`", and that `Net::new` does the reset. Under ADR-0070 the
order is the reverse, and the code is right: the kernel resets every transport inside `release_quarantine`
(`kernel/src/device.rs:230-241`). That call is refused while the device is mapped or pinned, and `ethd` makes it
before `device_map`. Only the text needs correcting.

**11. note: undone receives are not counted.** Recommendation 4 says that frames of a `Received` that could not be
sent are counted as dropped. `ethd` calls `net.release` without incrementing anything. *Fix:* add a
`Net::undo(slot)` that reposts the slot and counts the frame.

**12. note: inconsistent rule for undecodable messages.** `Attach` with a handle whose body does not decode gets
`BAD_OBJECT`. `Info`, `Transmit`, `Release` and unknown ordinals end the driver. Either is defensible, but the ADR
says "any message the driver cannot decode … ends the driver". Make the code and the ADR say the same thing.

**13. note: state before offer.** `post` sets `Rx::Posted` and then calls `offer`. In `release`, an `offer` failure
returns `WrongState` and leaves the slot `Posted` but not offered, so it is lost to both sides. `offer` cannot fail
for a `Lent` slot today. *Fix:* set the state after a successful offer.

**14. note: a possible flake in `t_net`.** The match at `t_net.rs:284` panics on anything but `Sent`. After the attach,
SLIRP may push a frame, for example an unsolicited IPv6 router advertisement, before the refusals arrive. *Fix:*
loop, release any `Received` slots, and then expect the `Sent`.

**15. note: the fuzz target.** It is honestly recorded as not run. I could not build it offline either. Its lock file
is stale, and nothing in `make check` builds the fuzz workspace (the test map sends `fuzz/` to `check`), so a type
error would go unnoticed. Its coverage matches the report's claim (the ARP parser, any head, length and used index,
hostile release and transmit). It does not cover link and configuration reads, truthful transmit completions (action
3 picks any `a`), or message decoding and dispatch (finding 5).

**16. note: the test map and the normal kernel.** `tools/test-map.txt` maps `kernel/` and `crates/zoraya-rt/` to
other steps but not to `test-net`. So `make test-changed` skips the only DMA receive path after a change to
`dma_pin`, the device code or the runtime. The `ethd` that `init` starts (`init.rs:331-341`) runs only under
`make run-net`, which no step boots, as the report says.

**17. note: notifications per slot.** `release` and `transmit` each write `QUEUE_NOTIFY`, so a `Release` of 64 slots
costs 64 MMIO exits. *Fix:* offer the whole batch and notify once. This matters for N3's throughput rows, not for
correctness.

## Checked and found sound

- **Device-supplied values in `Queue::take` and `Net::poll`.**
  - The used index is read once per call, after a fence. A jump, including a backwards one through wrapping, beyond
    `outstanding` is `Device`.
  - `id` and `len` are read once into locals, and each entry is read after the index.
  - A head that is out of range, not offered, or a repeat is `Device`. An odd head, `slot >= 64`, or a state mismatch
    is `Device`.
  - The receive length is computed with `checked_sub(HEADER)` and bounded by `MAX_FRAME`. Runts are reposted. The
    transmit length is ignored.
  - u16 wraparound is consistent, because the queue size divides 65536 and at most 64 chains are outstanding against
    128 ring entries.
  - No arithmetic on device input can overflow or panic.
- **Client-supplied values.**
  - In `release` and `transmit`, the slot range, state and length are checked before any index.
  - In `attach`, `checked_add` is used, one attach per life, and both a non-contiguous object and one of more than
    two runs are refused (the kernel returns `InvalidArgument` when `runs` is too small).
  - `dma_pin` requires `WRITE` on the memory and `DMA` on the device, so a read-only object cannot be made the
    device's target.
  - Handles other than `Attach`'s are closed, and sequences are bounded at 64.
- **Lifetimes.**
  - Both pins are made before the device sees an address.
  - `stop` unpins only after `reset` reads status 0, and exits without unpinning otherwise, which quarantines.
  - The pin keeps the client's object alive after `ethd` closes its handle.
  - `dma_release_quarantine` comes before `device_map`, as ADR-0070 requires.
  - The receive header cells and the all-zero, read-only transmit header are in `ethd`'s own pinned memory, and the
    driver never reads them.
- **Ring order.** `notify` fences, writes `avail.idx`, fences again, and then writes `QUEUE_NOTIFY`, the same
  pattern as `blkd` and `gpud`. `device_event` acknowledges `InterruptStatus` before it polls, and calls
  `interrupt_ack` after, so a completion during the poll raises the level-triggered line again.
- **`zoraya-proto-nic::arp`.** Checked slices only. Truncations, sizes and operations are covered by tests, and
  `answer` refuses a multicast sender.
- **`unsafe`.** Every block in `ethd.rs`, `t_net.rs` and `isar0` has a `// SAFETY:` comment whose argument holds.
  `t_net` copies a received slot once before parsing it.
- **`run-qemu.sh`.** `restrict=on` is set in `--test` mode. `--net` with `--gui` is refused. The pcap is cut to 256
  bytes per frame.
- **ADR-0221 rule 6 note.** The note added to ADR-0221's open questions is within ADR-0001 rule 6, and the INV-0221-2
  row correctly stays `not yet applicable`.

## Dispositions

Fixes are in the commits of `work/n1` after this review (`git log 7bef8cf..`); the subjects name the finding.

| # | Disposition |
|---|---|
| 1 | Fixed in "N1 review error 1 and warnings 3, 5": `zoraya_virtio_net::serve::Server` keeps what a full channel refused as bounded state (finished transmit slots as bits, one reply of each kind, one reason per transmit slot, a link flag) and writes it again on every event and on `WRITABLE`; a slot is `Tx::Done` until its `Sent` is written. Host tests: a full channel for 20 rounds then drained, no loss or duplicate, a slot not reusable before its `Sent`. |
| 2 | Fixed in "N1 review error 2": `make check-ethd-maps` (`tools/check-ethd-maps.sh`, in `make check`, CI and the test map); INV-0280-1 says `checked: make check-ethd-maps` and its text says it is a source check, not a run in QEMU. |
| 3 | Fixed in the first commit: a refusal is a `Rejected { slot, reason }` message (new in `zoraya-proto-nic`, ADR-0280 recommendation 4), never a `Sent`; `STILL_QUEUED` keeps the slot with the driver until its real `Sent`. Host and round-trip tests; `t_net` checks the reasons. |
| 4 | Fixed in the first commit: `t_net` closes the channel and waits (10 s) for `ethd` to end by itself with exit code 0, prints which ending ran, then `dma_release_quarantine` must find nothing. The kill ending with frames in flight stays untested (open for N2, with INV-0221-3). |
| 5 | Fixed: the dispatch is `serve.rs` in `zoraya-virtio-net` with 8 host tests (hostile client, handles closed, bounded reads) and in the fuzz target. |
| 6 | Fixed in "N1 review warning 6": `run-qemu.sh --probe-net` (0 / 1 / 2); the step skips only on 1, listed by `make test`'s summary; fails on any other probe failure; fails on 1 too when `CI` is set. |
| 7 | Fixed in "N1 review warning 7": a `hostfwd` and `tools/net-poke.sh`; SLIRP's who-has for `10.0.2.15` is answered in QEMU and the step greps the pcap for the reply (HVF run: 1 answered). Try 2 of 2 (try 1: SLIRP already knew the guest's MAC). TCG: see the report. |
| 8 | Fixed: `Net::poll` takes at most `2 * SLOTS` receive entries per call. |
| 9 | Open: `read_link` still reads `status` as two byte accesses and the MAC without `ConfigGeneration`. QEMU tolerates both; a `Registers::read16` and a generation loop are for the first real-device or N3 work. Not recorded elsewhere than here and the report. |
| 10 | Fixed in the commit of warning 6 (report text) and the `ethd.rs` header comment. |
| 11 | Fixed: `Net::undo` reposts and counts the frame; the Server uses it. |
| 12 | Fixed in code and ADR: an `Attach` whose body does not decode ends the driver like every other undecodable message (recommendation 6 now says so, and that an `Attach` without a handle gets `BAD_OBJECT`). |
| 13 | Fixed: `post` sets `Posted` after a successful `offer`. |
| 14 | Fixed for the steps before the rounds (`next_not_received` gives unsolicited frames back and skips link messages); in the rounds a refused transmit panics on purpose and an unsolicited frame is released. |
| 15 | Partly fixed: the target now drives the client half (messages, a full channel, link flips) and was compiled and run on 20000 random inputs in a scratch crate; not run under libFuzzer. Open: `fuzz/Cargo.lock` is stale (cargo regenerates it with network), and nothing in `make check` builds the fuzz workspace. |
| 16 | Fixed: `kernel/` and `crates/zoraya-rt/` select `test-net`. Open: no step boots the normal kernel with `--net`, so the `ethd` that `init` starts is never run (N2). |
| 17 | Open, accepted: one `QUEUE_NOTIFY` per slot; it matters for N3's throughput rows, not for correctness. |
