# TASK-12 review

- **Reviewer:** independent reviewer subagent (Claude), which did not write this code.
- **Commit reviewed:** `474e8ba` on `kernel/stage-4a` (`git diff main..HEAD`).
- **Date:** 2026-11-01

## Summary

`make test` passes from a fresh clone. I found no way for a process to panic the kernel or reach a freed frame, and the unmap sequence is correct. The new object lifetimes held up, with two exceptions: a failed `dma_pin` quarantines its frames, and `reserve_many` promises more than it reserves. Most errors are invariants marked `checked` whose check does not cover them, and deviations from accepted ADRs that have no proposed ADR.

14 errors, 11 imprecisions, 4 notes. 54 mutants: 43 killed, 11 survived.

## `make test` from a fresh clone

A new `git clone` of `474e8ba` into the scratchpad, then `make test`: exit 0, `make test: all checks passed`. Host tests: 290 passed, 0 failed. Each of the seven QEMU configurations: 94 passed, 9 expected failures. `make test-boot`: `exit.script` 8 steps, `leak.script` 5 steps, `console.script` 157 steps in each of seven configurations.

Measurements in that run, HVF default and TCG default. Round trip 16 B: 7,667 ns and 222,000 ns. Round trip 61,440 B inline: 380,416 ns and 3,404,992 ns. Through a mapped object: 9,666 ns and 234,000 ns. Inline copy one way per KiB: 3,107 ns and 26,531 ns. A call with 1 channel, then with 400 more idle: 7,666 and 7,625 ns; 218,000 and 221,008 ns.

## Findings

| ID | Severity | Location | Evidence | Suggested fix | Disposition |
|---|---|---|---|---|---|
| R-1 | error | INV-0053-2; `tests/shell/*.script`; `user/src/bin/init.rs` | Marked `checked: make test-boot`, but B3 (`init` gives `zosh` a `Device` at slot 5) passes both `exit.script` and `console.script`. No step enumerates `init`'s handle lists, and no step shows that a child cannot obtain a `Device`. | Have a test program report what it holds, or have `init` print its outgoing list and check it in a script. Until then the status must be `not yet applicable`. | Fixed: a `slots` built-in lists the shell's handles by type, and the script checks the whole list. |
| R-2 | error | INV-0052-3; `crates/zoraya-protocol` | Marked `checked: make test-unit`, but the check names a lint over the macro's table and no such lint exists. P7 adds `message Control { operation: u32, argument: Data<4096> }` to the block protocol and every test passes. `operation` is exactly the field the invariant forbids. | Add a test over a list of each protocol's messages and field names that refuses names such as `operation`, `op`, `command`, `ioctl`, or restate the check in a proposed ADR. | Fixed: `protocol!` refuses operation-like field names at compile time. |
| R-3 | error | INV-0052-2; `crates/zoraya-protocol/src/lib.rs`, `Seq::get` | "Any nonzero padding or trailing byte is refused" is true by reading, but its check does not cover `Seq`. P4 (`Seq` trailing bytes accepted) and P6 (padding between records unchecked) both survive. `exercise` only asks that nothing panics. | Add tests that set a padding byte between records and add a byte inside the declared size, expecting `BadPadding` and `Trailing`. | Fixed: tests of `Seq` trailing bytes and padding. |
| R-4 | error | INV-0052-1; `crates/zoraya-protocol/src/lib.rs`, `length` | P2 removes the decoder's check of a declared maximum and all tests pass. `limits_are_checked_both_ways` tests only the encoder. So a decoder that accepts a `Data`, `Text` or `Seq` over its declared size is not detected, and ADR-0052 requires "a declared maximum". | Add decoder tests with a length of N + 1 for each variable kind. | Fixed: decoder tests over each maximum. |
| R-5 | error | INV-0050-4; `user/src/bin/t_channels.rs`; `kernel/src/channel.rs`, `close` | K7 handles only the first handle of each message and leaves the rest to `Drop`. It passes in all tests, because no test queues a message with two handles where one is an end or a port. Under K7 a second end in a message would never close, so its peer would never see `PEER_CLOSED`. The code itself is right by reading. | Test a message carrying a process handle first and a channel end after it, close the reader, and expect `PEER_CLOSED` at that end's peer. | Fixed: a message with a process first and an end after it. |
| R-6 | error | INV-0054-2, second half; `user/src/bin/t_devices.rs` | K20 removes `irq::unregister` from `Interrupt::drop` and everything passes. The test only shows that another object can take the line, not that the line is masked. ADR-0054 gives the reason: a device could raise a line with no handler, which is a panic. | In the test kernel, after the close, trigger the line and expect no unexpected interrupt, or expose whether the GIC has the line enabled. | Fixed: a test call reads the GIC after the close. |
| R-7 | error | INV-0054-7, second half; `user/src/bin/t_devices.rs` | The invariant's check includes "a test with a pin handle lacking `DMA`". There is no such test, and K23 (`dma_unpin` needs no right) survives. The code does check `DMA`. | Move a pin through a channel with `Rights::TRANSFER` alone, and expect `AccessDenied` from `dma_unpin`. | Fixed: a pin moved with `TRANSFER` alone gets `AccessDenied`. |
| R-8 | error | INV-0050-6; `user/src/bin/t_channels.rs`, `a_deep_chain_closes` | The stated check ends "reporting how long interrupts stayed masked" and ADR-0050 says "INV-0050-6's test measures the stall". The test reports nothing: `masked` and `stall` do not occur in `t_channels.rs` or `chanbench.rs`. The bounded-stack half is sound (K8 and C6 killed). My probe X1b measured the 20,000-link close at 10.9 ms under HVF, so the number is cheap to get. | Print the time around `handle_close(head)` in `a_deep_chain_closes`, and record it in the task report. | Fixed: the test prints the time, about 11 ms under HVF. |
| R-9 | error | `kernel/src/syscall.rs`, `dma_pin`; `kernel/src/device.rs`, `Pin::drop` | A failed call has a lasting effect. With a full handle table, `Pin::new` succeeds, `self.insert` gives `LimitReached`, and dropping the unused pin puts the memory object in quarantine. Probe X2 ran it: `dma_pin` gave `Err(LimitReached)`, then after the memory handle was closed the frame came back only from `dma_release_quarantine` (26749, then 26750). A running driver with a mapping cannot release, so the frames stay lost until its window is unmapped. The caller also has the addresses in `out` by then. | Call `reserve` on the handle table before `Pin::new`, or mark the pin unpinned if the insert fails. Write `out` only after the point of no failure. | Fixed: everything that can fail comes before the pin, with a test. |
| R-10 | error | `crates/zoraya-handle/src/lib.rs`, `reserve_many`; `kernel/src/syscall.rs`, `channel_read`, `channel_create` | By reading: "that many inserts cannot fail" is false. `insert` appends a new slot while `slots.len() < MAX_HANDLES`, even if earlier slots are free, but `reserve_many` reserves only `max(count + n, len) - len`. With a table of 1000 slots at capacity and 10 live handles it reserves nothing, so a later insert can give `NoMemory` when the heap is out. `channel_read` has by then taken the message: it is lost, against INV-0050-1, and its remaining entries are dropped without `object::close`, so an end among them never shows `PEER_CLOSED`. It needs kernel heap exhaustion; I did not reproduce it. | Reserve `min(MAX_HANDLES, len + n) - len`, or make `insert` reuse free slots after a reservation. Add a host test on `capacity`. | Fixed: reserves appended slots, with a host test. |
| R-11 | error | ADR-0052 Handles; ADR-0052 open question added by this branch; `crates/zoraya-protocol/src/lib.rs`, `decode` | The accepted ADR says handles are "in declared order, with their object types checked". `decode` checks only the count. The branch records this as an open question and a backlog row. CLAUDE.md and ADR-0001 rule 4 require a `proposed` ADR for any deviation, and rule 6's note covers a check that cannot be made yet, not a changed rule. | Write a proposed ADR (0072) stating count only and a type error at first use, or add a call that reports a handle's type. | Proposed ADR-0072. |
| R-12 | error | ADR-0052 Body; `crates/zoraya-proto-file/src/lib.rs`, `crates/zoraya-proto-block/src/lib.rs` | "Enumerations are range-checked". `status: u32` in every reply and `kind: u32` in `Attributes` and `Entry` are enumerations with listed values, but they are plain integers and any value decodes. | Add a checked enumeration kind to `zoraya-protocol` and use it, or state in a proposed ADR that replies are open sets. | Fixed: `Below<N>` for status and kind. |
| R-13 | error | ADR-0055 Handles and rights, ADR-0013; `crates/zoraya-proto-file/src/lib.rs`, `Open` | ADR-0055 gives `open(directory, RelativePath, ...)`, and `RelativePath` refuses a leading `/`. `Open.path` is `Text<MAX_PATH>`, so `/etc/x` and an empty path decode. ADR-0052 asks that the decoder be the single check. `OpenReply.remaining` likewise. | Add a `Path` field kind that refuses a leading `/` and an empty path, and test it on the wire. | Fixed: `Path<N, EMPTY>`. |
| R-14 | error | `docs/adr/0063-first-process-until-init.md`; `docs/adr/index.md`; commit `474e8ba` | An agent changed the Status line to `superseded by ADR-0053` and added a sentence to "Architect's decision". ADR-0001 rule 2 says only the architect sets `superseded` and an agent never edits that section; CLAUDE.md repeats it. TASK-12 says "as ADR-0001 allows", and ADR-0001 does not allow this. The added sentence is written as if decided by the architect. | Revert both edits, keep the open-question note, and ask the architect to record the supersession. | Fixed: status and decision section reverted. The architect is asked to record it. |
| R-15 | imprecision | `kernel/src/memory/frames.rs`, `alloc_contig_zeroed`; `user/src/bin/t_memory.rs` | K10 (contiguous objects not zeroed) survives. ADR-0051 says "zeroed"; only scattered objects are read back. A contiguous object would show another process's old data, so this deserves a test though it is not an invariant. | Dirty a contiguous object, free it, create one again and read it. | Fixed: a test over dirtied frames. |
| R-16 | imprecision | INV-0050-1, ADR-0050 Read; `t_channels.rs` | K2 removes `reserve_many` from `channel_read` and passes. ADR-0050's "a full handle table is `LimitReached`, and the message stays" has no test. | Fill the table, read a message with a handle, and check that it stays. | Fixed: a test with a full table. |
| R-17 | imprecision | INV-0070-2; `t_devices.rs` | K27 (reset does not wait for Status 0) survives. QEMU resets at once, so the test cannot see the wait. The check holds for the write and the first read, not for a slow device. | Accept and say so in ADR-0070, or test the loop on a fake register in a host crate. | Accepted, and said in ADR-0070. |
| R-18 | imprecision | INV-0053-1; `tests/shell/*.script` | The check is indirect. B1 is caught only by `exit.script` (`console.script` passes), and B2 only because `hello` happens to print a line the script forbids. A second process that prints nothing would pass. The output does not show "`init` as the only process". | Have the kernel print each process it starts and match that line. | Fixed: the kernel prints the first process, and the script forbids a second. |
| R-19 | imprecision | `kernel/src/memory/user.rs`, `UserMemory::write`, `peek` | Two SAFETY comments are stale. `write` says "this space is only changed while it is being built", but `map_object` and `unmap_object` change a live space. `peek` says "a frame of this space", but a mapping may now be a memory object's page, or a device page, which `phys_to_virt` does not map. | Reword `write` to rely on the `space` lock and the one thread. Make `peek` refuse a device mapping. | Fixed. |
| R-20 | imprecision | `kernel/src/device.rs`, `read`, `write`, `init` | The SAFETY comment says `offset` is aligned and inside the page. That rests on the device tree: `transport.base` is not checked for alignment, and `offset + 0x70` can pass the page end. `VirtioTransport.size` is parsed and unused, so a range larger than its page is silently cut. A ninth transport in a page is silently dropped. | At `init`, assert alignment and that the range fits the window, and report a dropped range. | Fixed: a transport that does not fit is left out and reported. |
| R-21 | imprecision | `kernel/src/gic.rs`, `enable`; `kernel/src/device.rs`, `Interrupt::ack` | `interrupt_ack` calls `gic::enable`, which rewrites `GICD_ICFGR`. If the holder acknowledges a line that is enabled (a second ack), that register changes on an enabled interrupt, which the GIC specification does not allow as I recall it. QEMU does not mind. | Set the type once in `irq::register`, and have `ack` only write `ISENABLER`. | Fixed: `gic::unmask`. |
| R-22 | imprecision | `crates/zoraya-protocol/src/testing.rs`; TASK-12 step 3 | The doc comment says "random bytes", and the task asks for that. The loop changes one byte of a valid message at a time, always restoring it, so it never tries a wrong count with a wrong size together. INV-0052-2 asks for a "generated" round-trip test per message, and the tests are a hand list, so a new message is not covered. It does list all 24 file and 7 block messages today. `ErrorReply` gets no `exercise`. | Add a loop of wholly random bodies under a valid header, and several mutated bytes at a time. Generate the test in the macro, or fix the wording. | Fixed: random bodies and several bytes. The list stays by hand. |
| R-23 | imprecision | INV-0062-1; `user/src/bin/init.rs` | "Only the first process starts with a `Power` handle" is now false: `zosh` starts with one at slot 4, from `init`. ADR-0062 allows handing on, but the invariant wording does not, and it is still `checked`. | Propose a rewording: only the kernel makes one, and only the first process receives one from the kernel. | In the backlog, for the architect. |
| R-24 | imprecision | INV-0063-2 row in `docs/adr/invariants.md` | The status says `not yet applicable (stage 4)` during stage 4, while ADR-0063's new note says the path has no test and is checked by review only. | Use a true status, once the status question in the finding on ADR-0063 is settled. | Left, with ADR-0063's status, for the architect. |
| R-25 | imprecision | Decisions made without an ADR: `kernel/src/syscall.rs`, `kernel/src/device.rs` | (a) `dma_unpin` closes its handle (backlog W7 open, built anyway). (b) `memory_unmap` ends only one exact mapping; ADR-0051 does not say partial unmapping is refused. (c) One interrupt object per line, `BadState` for a second. (d) `memory_create` refuses a size that is not whole pages. (e) ADR-0070's 100 ms is per transport, up to 800 ms in a page. Each is reasonable. CLAUDE.md asks for a short proposed ADR when unsure. | One proposed ADR (0073) covering these, with W7 closed by it. | Proposed ADR-0072. |
| R-26 | note | Interrupts masked for long operations (unbounded by a fixed number) | System calls run masked. Measured under HVF, on 128 MiB: `memory_create` of 26,469 frames 9.3 ms; closing that object 3.6 ms; a 20,000-link chain 10.9 ms. All scale with memory or chain length, and TCG is slower (not measured). By reading also: `memory_map` and `dma_pin` (per page), address-space teardown, a 64 KiB copy (26 µs per KiB under TCG, from the measurements above), and the reset, up to 100 ms a transport. Acceptable while every process is trusted. ADR-0056 must bound them. | Record in Known limitations of `docs/design.md`. | Recorded in Known limitations. |
| R-27 | note | ADR-0050 No cycles; `kernel/src/syscall.rs`, `spawn` | "An endpoint that has handles queued for reading cannot be moved at all" holds for `channel_write`. `spawn` can move such an end. That cannot make a cycle, since a process is not a queue, so the rule's reason holds and its words do not. | Say "through a channel" in a later ADR. | In the backlog. |
| R-28 | note | `crates/zoraya-protocol/src/lib.rs` | ADR-0052's follow-up, a wire-format document next to the macro, does not exist, and `Seq`'s layout (count, padding, 64-bit size) is stated nowhere but the code, against driver 5. A record is always `VARIABLE`, so a fixed record cannot precede a fixed field. `Seq::iter` decodes again, against "once", safely: it runs the same `get` over bytes `get` accepted. | Add the document in TASK-13. | Fixed: `crates/zoraya-protocol/WIRE.md`. |
| R-29 | note | Small | `MemoryObject::runs` has a doc comment for another function ("True if the frames are one physical run"). `user/src/bin/t_loop.rs` has `let _ = Rights::NONE;` to use an import. `testing` in `zoraya-protocol` is compiled into every user. A device window can pass a user pointer into system calls, and a load pair on MMIO under HVF may stop QEMU; ADR-0054 already places the driver in the trusted base. | Tidy when next there. | Fixed the first two. The rest stays. |

## Mutation table

Each mutant was applied alone in the working tree, run, and reverted with `git checkout`. "Survived" means the check still passed. `make test-qemu-one` is the `auto-default` configuration. "boot" is `exit.script` and `console.script` on the normal kernel under `auto`. A baseline with a comment changed passed first (94 passed, 9 expected failures). Every invariant in the brief has at least one mutant.

| Mutant | Invariant | File | Check | Result |
|---|---|---|---|---|
| C1: queue push_front instead of push_back | INV-0050-1 | `crates/zoraya-channel/src/lib.rs` | `cargo test -p zoraya-channel` | killed |
| C2: may_travel ignores queued handles | INV-0050-2 | `crates/zoraya-channel/src/lib.rs` | `cargo test -p zoraya-channel` | killed |
| C3: no byte bound in prepare | INV-0050-3 | `crates/zoraya-channel/src/lib.rs` | `cargo test -p zoraya-channel` | killed |
| C4: no message bound in prepare | INV-0050-3 | `crates/zoraya-channel/src/lib.rs` | `cargo test -p zoraya-channel` | killed |
| C5: channel rights gain DUPLICATE | INV-0050-5 | `crates/zoraya-abi/src/lib.rs` | `cargo test -p zoraya-abi` | killed |
| C6: Pile drop recursive (default drop) | INV-0050-6 | `crates/zoraya-channel/src/lib.rs` | `cargo test -p zoraya-channel` | killed |
| A1: dma_unpin takes a Size | INV-0051-5 | `crates/zoraya-abi/src/lib.rs` | `cargo test -p zoraya-abi` | killed |
| A2: Handle encode truncates to 32 bits | INV-0057-3 | `crates/zoraya-abi/src/lib.rs` | `cargo test -p zoraya-abi` | killed |
| A3: spawn set_wide truncates | INV-0057-3 | `crates/zoraya-abi/src/spawn.rs` | `cargo test -p zoraya-abi` | killed |
| H1: generation wraps at 16 bits | INV-0057-1 | `crates/zoraya-handle/src/lib.rs` | `cargo test -p zoraya-handle` | killed |
| H2: count lost at the wrap | INV-0057-2 | `crates/zoraya-handle/src/lib.rs` | `cargo test -p zoraya-handle` | killed |
| H3: a slot retires at the last generation | INV-0057-2 | `crates/zoraya-handle/src/lib.rs` | `cargo test -p zoraya-handle` | killed |
| P1: Reader::take indexes without a check | INV-0052-1 | `crates/zoraya-protocol/src/lib.rs` | `cargo test -p zoraya-protocol -p zoraya-proto-block -p zoraya-proto-file` | killed |
| P2: decoder does not enforce declared maximum | INV-0052-1 | `crates/zoraya-protocol/src/lib.rs` | `cargo test -p zoraya-protocol -p zoraya-proto-block -p zoraya-proto-file` | survived |
| P3: padding not checked | INV-0052-2 | `crates/zoraya-protocol/src/lib.rs` | `cargo test -p zoraya-protocol -p zoraya-proto-block -p zoraya-proto-file` | killed |
| P4: Seq trailing bytes accepted | INV-0052-2 | `crates/zoraya-protocol/src/lib.rs` | `cargo test -p zoraya-protocol -p zoraya-proto-block -p zoraya-proto-file` | survived |
| P5: message trailing bytes accepted | INV-0052-2 | `crates/zoraya-protocol/src/lib.rs` | `cargo test -p zoraya-protocol -p zoraya-proto-block -p zoraya-proto-file` | killed |
| P6: Seq padding between records not checked | INV-0052-2 | `crates/zoraya-protocol/src/lib.rs` | `cargo test -p zoraya-protocol -p zoraya-proto-block -p zoraya-proto-file` | survived |
| P7: a free-form operation message added | INV-0052-3 | `crates/zoraya-proto-block/src/lib.rs` | `cargo test -p zoraya-proto-block` | survived |
| T1: Total not compared | INV-0071-1 | `tools/adr-check/src/main.rs` | `cargo test -p zoraya-adr-check` | killed |
| T2: missing Counts row not an error | INV-0071-1 | `tools/adr-check/src/main.rs` | `cargo test -p zoraya-adr-check` | killed |
| K1: prepare after handles leave the table | INV-0050-1 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K2: channel_read skips reserve_many (LimitReached no longer leaves the message) | INV-0050-1 | `kernel/src/syscall.rs` | `make test-qemu-one` | survived |
| K3: kernel ignores holds_handles | INV-0050-2 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K4: kernel ignores same channel | INV-0050-2 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K5: MAX_QUEUED 65 | INV-0050-3 | `crates/zoraya-channel/src/lib.rs` | `make test-qemu-one` | killed |
| K6: closing forgets queued messages | INV-0050-4 | `kernel/src/channel.rs` | `make test-qemu-one` | killed |
| K7: closing handles only the first handle of a message | INV-0050-4 | `kernel/src/channel.rs` | `make test-qemu-one` | survived |
| K8: closing recurses | INV-0050-6 | `kernel/src/channel.rs` | `make test-qemu-one` | killed |
| K9: failed create keeps frames | INV-0051-1 | `kernel/src/memory/object.rs` | `make test-qemu-one` | killed |
| K10: contiguous object not zeroed | INV-0051-1 (zeroing) | `kernel/src/memory/frames.rs` | `make test-qemu-one` | survived |
| K11: read-only mapping executable | INV-0051-2 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K12: every mapping writable | INV-0051-2 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K13: write mapping needs only READ | INV-0051-2 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K14: unmap leaves the entries | INV-0051-3 | `kernel/src/memory/user.rs` | `make test-qemu-one` | killed |
| K15: no TLB invalidation on unmap | INV-0051-3 | `kernel/src/memory/user.rs` | `make test-qemu-one` | killed |
| K16: unmap leaks the holder | INV-0051-4 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K17: exit leaks mappings | INV-0051-4 | `kernel/src/memory/user.rs` | `make test-qemu-one` | killed |
| K18: device mapping Normal | INV-0054-1 | `kernel/src/syscall.rs` | `make test-qemu-one` | killed |
| K19: line stays enabled when it fires | INV-0054-2 | `kernel/src/device.rs` | `make test-qemu-one` | killed |
| K20: closing does not mask the line | INV-0054-2 | `kernel/src/device.rs` | `make test-qemu-one` | survived |
| K21: a dead holder counts as unpinned | INV-0054-3 | `kernel/src/process.rs` | `make test-qemu-one` | killed |
| K22: closing a pin counts as unpin | INV-0054-7 | `kernel/src/object.rs` | `make test-qemu-one` | killed |
| K23: dma_unpin needs no right | INV-0054-7 | `kernel/src/syscall.rs` | `make test-qemu-one` | survived |
| K24: release ignores mappings | INV-0070-1 | `kernel/src/device.rs` | `make test-qemu-one` | killed |
| K25: release ignores live pins | INV-0070-1 | `kernel/src/device.rs` | `make test-qemu-one` | killed |
| K26: release does not reset | INV-0070-2 | `kernel/src/device.rs` | `make test-qemu-one` | killed |
| K27: reset does not wait for Status 0 | INV-0070-2 | `kernel/src/device.rs` | `make test-qemu-one` | survived |
| K28: reset writes nothing | INV-0070-2 | `kernel/src/device.rs` | `make test-qemu-one` | killed |
| K29: no transport counts as reset | INV-0070-3 | `kernel/src/device.rs` | `make test-qemu-one` | killed |
| K30: AddressSpace drop does not invalidate its ASID | TLB (not an invariant) | `kernel/src/memory/user.rs` | `make test-qemu-one` | killed |
| B1: kernel starts zosh, not init | INV-0053-1 | `kernel/src/main.rs` | boot | killed |
| B2: kernel starts a second process | INV-0053-1 | `kernel/src/main.rs` | boot | killed |
| B3: init gives zosh a Device | INV-0053-2 | `user/src/bin/init.rs` | boot | survived |

Survivors (11): P2, P4, P6, P7, K2, K7, K10, K20, K23, K27, B3. K27 cannot be killed on QEMU (R-17). The rest are test gaps: the code under them is right by reading.

K15 and K30 show that the TLB steps matter: without them `memory_objects_from_a_process`, and many process tests through ASID reuse, fail under HVF.

### Probes

Temporary test code, run on the unmodified kernel and reverted.

| Probe | Result |
|---|---|
| X1: `spawn` moves one channel end to two slots | Refused. `SpawnRequest::decode` refuses a moved handle in a second entry, so an end keeps one holder. My suspicion was wrong: no finding. |
| X1b: time to close a 20,000-link chain | 10.9 ms under HVF, no panic. |
| X2: `dma_pin` at a full table | `LimitReached`, and the frame is quarantined: R-9. |
| X2: `device_map` and `device_interrupt` with index `u32::MAX`; `dma_pin` with a huge `out`; an index of 2^32 | `InvalidArgument`, `InvalidArgument`, `BadAddress`, `InvalidArgument`. No panic. |
| X3: `memory_create` of 2^52, 2^62, 2^63 and 2^64 - 4096, plain and contiguous; `memory_unmap` and `memory_map` with extreme values | `NoMemory` or `InvalidArgument`. No panic. |
| X3: every frame taken (0 free), then `channel_create`, `port_create`, `memory_map`, `directory_lookup`, `spawn` | Channel, port and lookup worked from free heap; map gave `NoMemory`; spawn failed cleanly. No panic, and all counts returned. |

## TLB and break-before-make analysis

**`memory_unmap`.** `AddressSpace::unmap_object` finds the exact mapping, then calls `PageTable::unmap`. That checks coverage and calls `split_at`, which does nothing here because `map_object` maps 4 KiB pages only. `remove` writes 0 to each leaf, then calls `invalidate` once over the range, which is `mmu::invalidate_asid`: `dsb ishst; tlbi aside1is; dsb ish; isb`. Only after that does `unmap_object` return the `Holder`, and only then does the object's count fall. This is the Arm ARM's sequence for removing a translation: the store is visible to walkers before the invalidation, and the invalidation is complete before the frame can be freed. Break-before-make proper applies to changing a live entry into another live one. Nothing here does that: entries go valid to invalid, and later invalid to valid. `aside1is` covers cached walk entries of the ASID too. Empty tables stay allocated, so no table is freed under a walk cache.

**New mappings.** `map_pages` writes a leaf into an invalid entry and ends with `dsb ishst; isb`. An invalid entry is not held in a TLB, so no invalidation is needed. A new table is made visible before its link. A reused address follows an earlier invalidation.

**Exit.** `exit_current` sets `EPD0`, then `TTBR0_EL1` to 0, with `isb`, so the thread can no longer walk or speculate through the user table. `AddressSpace::drop` then invalidates the ASID, drops the mappings (frames of objects, and the device hold), frees the table and user frames, and last frees the ASID. So a frame goes back after no TLB entry can name it, and an ASID is reused clean. K30 shows the last step is needed.

**Is a freed frame reachable through a stale entry?** No, by reading, on one CPU. A partial-failure path in `map_object` unmaps through the same sequence before dropping its holder. `DeviceMapping` clears `mapped` only after the same steps, so `dma_release_quarantine` cannot run with a stale user mapping of the device.

**Four CPUs.** QEMU reports four, and only the boot CPU runs. The others stay powered off, since nothing calls `CPU_ON` (`boot.rs`), so they hold no TLB entries. The inner-shareable forms are what SMP will need, so this code does not depend on one CPU. Other things will need work at SMP: `SpinLock`, `try_arc`. Migration of a vCPU between host cores under HVF is the host's concern: I did not examine it.

**Device attributes.** `Attributes` with `MemoryType::Device`, `write`, `user`, not `execute` encodes attribute index 0 (`MAIR` 0x00, Device-nGnRnE), `AF`, `AP_USER`, `nG`, `PXN` and `UXN`. `is_forbidden` refuses executable device memory. Shareability bits are 0, which Device memory ignores. The page is mapped the same way in the kernel, so no attribute mismatch arises. `nG` is what makes the ASID invalidation enough. The kernel's writes in `reset` go to the same peripheral as any earlier user access, and Device-nGnRnE keeps those in order.

## Kernel object lifetime: checked with no finding except R-9 and R-10

- A pin moved through a channel: if the reader closes, `close` drops it, and `Pin::drop` quarantines it. `Pin::new` reserves room for every live pin, so `push` does not allocate. `live` stops a pin that failed in `try_arc` from counting.
- `Interrupt::drop`: `end_subscriptions` runs before it, the line is masked and its handler removed, then `LINES` and the source go. `line_fired` finds nothing after that.
- Exit order: mappings before handles, as ADR-0051 says. Each entry goes through `object::close`.
- A channel `Pair`: each end's closing empties that end's queue, and a closed end accepts nothing, so `Pair` never drops with messages and `Drop` has no recursion. Moved entries end their subscriptions, so `free_source` finds none.
- A message left at closing: messages an end sent stay for the peer, and its unread ones go through the work list. The no-cycle proof holds in the code: any edge closing a cycle moves an end that already holds a handle, so it is refused, including for an end in flight.
- `spawn`: a failed `create` drops clones only, so no end is lost.
- Lock order: handles, then channel, then shown, then the event lock; `LINES` then the event lock; `space` then frames or device state. I found no reverse order.

## `unsafe` and SAFETY comments

Read every `unsafe` and `SAFETY` line the diff adds, in the kernel and in `user/`. Findings R-19 and R-20. `user/src/bin/child.rs` has "SAFETY: none: the fault is the point", which is accurate. All others hold.

## Decoders and hostile input

`Reader`, `Writer`, every `Field`, `Seq::get` and `Seq::iter`, `Header::decode`, `ChannelEntry::from_bytes`, `ChannelSizes`, and the system calls reading user lists. All indexing goes through `get`, `checked_add` or `split_at_checked`; `from_bytes` takes a fixed array. Every loop is bounded by a declared `N` or the input length. `next_multiple_of` has a power-of-two argument and an offset within the buffer, so it does not overflow. `Seq::iter` uses the same start and the same `get` as `Seq::get`, so it agrees with it, and that holds when nested, since a part starts at an offset of 8. No panic is reachable from a hostile message. `#![forbid(unsafe_code)]` is on the new crates. System calls: list and message lengths are checked before use, `UserSlice` checks overflow, `count` is at most 64 from a kernel-held message, and `frames` arithmetic is bounded by `count <= free`.

## Consistency with accepted ADRs

Read against ADR-0040, 0041, 0044, 0050 to 0055, 0057, 0060 to 0063, with 0070 and 0071 as proposed. Deviations are R-11, R-12, R-13, R-14, R-23, R-24, R-25 and R-27. Rights and object types in `object_types!` match ADR-0050, ADR-0051 and ADR-0054. ADR-0050's limits, statuses, signals and `WRITABLE` rule match. ADR-0051's 256-mapping limit includes device mappings, which it does not exclude. ADR-0053's slots match. ADR-0057's width and ADR-0070's release order match. Rows INV-0040-1 and INV-0061-1 are correctly replaced. `make adr-check` passed. No external crate was added: the four `Cargo.lock` additions are this workspace's.

## What was and was not checked

Checked as stated above. Not checked:

- Mutants ran under `auto-default` or `auto` only, not under TCG or the other configurations.
- Real hardware, and an external abort from a driver.
- A driver at work: no queue was run, so DMA itself and a slow reset were not exercised (TASK-13).
- R-10 was found by reading and not reproduced, and neither was R-21's GIC point against the specification text.
- `INV-0051-5`'s `RelativePath` half was not mutated.
- All mutants and probes were reverted. `git status --short` shows no source change: only the two untracked task files and this review.

## Check of the fixes

- **Reviewer:** independent reviewer subagent (Claude), which took no part in this code or its first review.
- **Range:** `git diff 474e8ba 4b32f97`.
- **Date:** 2026-11-01

Errors in changed lines: 3.

Also 6 lesser points below, which do not count as errors. `make test` passed in the working tree at `4b32f97`, not from a fresh clone: `make test: all checks passed`, `console.script` 162 steps in the boot runs seen.

### Problems

| ID | Severity | Location | Evidence | Suggested fix |
|---|---|---|---|---|
| C1 | error | `docs/adr/0072-handle-types-and-small-kernel-choices.md`, Option C | The ADR says each method's first act with a handle "is a call that needs that type, so a wrong one gives `BadHandle` before an effect", and lists "a `server_end` is subscribed". That is false. `port_subscribe` takes any object with signals, and a process knows bits 0 and 2, the same bits as `READABLE` and `PEER_CLOSED`. A probe subscribed a process handle with those two signals, and the call succeeded. So `open` with `CREATE` and a wrong end passes the first use and can create the file. "A token is compared" does not fit either: a comparison gives "not the same", not `BadHandle`, and `object_same` does not exist yet. | Correct the claim before the architect decides. State what each first use really refuses, require a first call that does check the type for `server_end`, or recommend Option B. |
| C2 | error | `user/src/bin/t_channels.rs`, `closing_closes_what_is_queued` (R-5) | The new comment says "each is closed, not only the first". `close` takes handles with `Vec::pop`, so the last in the list goes first. The test puts the end last. A mutant that turns `while let` into `if let`, so only one handle per message is handled, passes `make test-qemu-one` (94 passed, 9 expected failures). Only the mutant that handles index 0 is killed. INV-0050-4 is therefore still not covered for a message with several handles. | Also send a message with the end first and the process after it, or two ends, and expect `PEER_CLOSED` at each peer. |
| C3 | error | `tests/shell/console.script`, `x first process:`; disposition of R-18; INV-0053-1 | The disposition says the script forbids a second process. An `x` step covers only the text between the last two matches (`Step::Absent`), here the `first process: init` and `zosh: type help` lines. A mutant that calls `spawn_first("spin")` before `init` prints `first process: spin` and passes all 162 steps of `console.script`. This is the silent second process R-18 was about. `exit.script` was not run on this mutant. | Use `! first process: ` with a check of exactly one such line, for example a kernel line printed after the first process starts, or match the first such line and refuse any other name. |
| C4 | imprecision | R-1 disposition; `tests/shell/console.script`; INV-0053-2 | The disposition says `slots: 1 2 3 4`; the script expects `slots: 1 2 3 4 5`. The step sees slot numbers up to 32, not object types, so a `Device` in place of `Power` at slot 4, or at a slot past 32, would not show. It does kill B3 as described. The invariant's check still says `init`'s lists are enumerated, which no step does. | Correct the disposition, and word the invariant's check as what is run. |
| C5 | imprecision | `docs/design.md`, shell section | The new built-in `slots` is not in the list of built-ins, which CLAUDE.md asks to keep true. | Add it. |
| C6 | imprecision | `docs/backlog.md`, row W7 | The row is removed as closed by ADR-0072, but it had two halves. ADR-0072 covers `dma_unpin` closing its handle, not "a pin can also end when a queue is torn down". ADR-0072 is also only proposed. | Keep that half in the backlog until the architect decides. |
| C7 | imprecision | `crates/zoraya-protocol/src/lib.rs`, `Path` (R-13) | The fix does what its disposition says. But the type cites ADR-0013, and a probe shows `a/../b`, `..`, `.`, `a//b` and a control byte all decode. INV-0013-5 says no `.` or `..` component is accepted by any service request. This gap is older than `Path`, and `Text` had it too. | Check each component with `Name::is_valid`, and allow an empty path only where `EMPTY` is set. |
| C8 | imprecision | `docs/adr/0063-first-process-until-init.md`, open questions | R-14 said to revert the edits and keep the note. The note now also says that ADR-0053 replaces this ADR in the code. That goes past the open-question note of ADR-0001 rule 6, the only edit CLAUDE.md allows here besides rule 3's. It decides nothing new, since ADR-0053 says the same. | Keep only the sentence on INV-0063-2, and leave the rest to the architect's request. |
| C9 | note | `kernel/src/device.rs`, `init` | `transport.base + transport.size.max(...)` is an unchecked addition on device-tree values, by reading. `virtio_transports` filters a zero size but not, in the lines I read, an overflow. It needs a broken device tree, and QEMU gives none. | `checked_add`, and leave the transport out on overflow. |

### Mutants

Each was applied alone, run and reverted. The `make test-qemu-one` runs are the `auto-default` configuration. B3 and the boot probes ran `console.script` on the normal kernel under `auto`.

| Mutant | Check | Result | Caught by |
|---|---|---|---|
| P2 | `cargo test -p zoraya-protocol -p zoraya-proto-block -p zoraya-proto-file` | killed | `a_decoder_enforces_declared_maxima` |
| P4 | same | killed | `a_sequence_has_no_loose_bytes` |
| P6 | same | killed | `padding_between_records_is_refused` |
| P7 | `cargo test -p zoraya-proto-block` | killed | Does not compile: "an operation field" |
| K2 | `make test-qemu-one` | killed | `channels_from_a_process` |
| K7, `if let` on `pop`, one handle a message | `make test-qemu-one` | **survived** | C2 |
| K7, only index 0 handled | `make test-qemu-one` | killed | `channels_from_a_process` |
| K10 | `make test-qemu-one` | killed | `memory_objects_from_a_process` |
| K20 | `make test-qemu-one` | killed | `devices_from_a_process` |
| K23 | `make test-qemu-one` | killed | `devices_from_a_process` |
| B3 | `console.script` | killed | `slots: 1 2 3 4 5 6` where the script expects 5 |

The first review's table does not give K7's exact edit, so I ran both forms. Its wording, "the first handle", fits the second; its likely edit, given the `pop` loop, is the first. Nine of the ten mutants are killed outright, and K7 is killed in one form and survives in the other. P7's kill rests on a list of names, so `selector: u32` would pass; INV-0052-3 leaves disguised operations to review.

Probes, all temporary and reverted:

| Probe | Result |
|---|---|
| `spawn_first("hello")` before `init` | Caught only because `hello` prints a line the script refuses (`x hello from EL0`). |
| `spawn_first("spin")` before `init` | Passes `console.script`: C3. |
| `port_subscribe` on a process with `READABLE` and `PEER_CLOSED` | Succeeds: C1. |
| `Paths` with `a/../b`, `..`, `.`, `a//b` and a control byte | All decode: C7. |

### Dispositions

All of R-1 to R-29 were read against the diff. Apart from C1 to C8, each fix does what its disposition says. In particular, by reading: `reserve_many` now covers appended slots. In `dma_pin` nothing can fail after the pin but the second write to bytes just written, and a process has one thread; no reverse order of the handle and device locks appeared. `gic::unmask`, `is_enabled`, the `peek` and `write` SAFETY comments, the `init` fit check and `WIRE.md` agree with the code, and `WIRE.md` agrees with ADR-0052. The status values fit `Below<16>` and `Below<5>`. ADR-0072's five small choices match the code, and 0072 is inside TASK-12's reserved range. The chain close printed 11.0 to 11.5 ms under `auto`, as R-8 says.

### Backlog, outside the changed lines

- `x` steps in the scripts cover only the text between the last two matches, which the wording of older steps such as `x hello from EL0` does not suggest.

### What was and was not checked

Checked as above. Not checked:

- A fresh clone. `make test` ran in the working tree.
- Mutants under TCG or any other configuration.
- `make test-boot`'s other scripts on B3 or the probes.
- K27: the first review accepted it, and it was not in this brief.
- The GIC specification text for R-21, and the kernel's behaviour on a broken device tree (C9).
- Old problems outside the changed lines, except where noted.
- All mutants and probes were reverted. `git status --short` shows only this review and the two untracked task files.

### Disposition of the check

- **C1:** ADR-0072 now recommends `handle_type`, which is built and tested. **C2:** a message with an end on each side of the process. **C3:** the runner gains `o TEXT`, exactly once. **C4:** `slots` gives types. **C5, C6, C8, C9:** done. **C7:** every component of a `Path` is a `Name`.
- The mutants of C2 and C3 were rerun by the author, one configuration each. No further independent check was made.
