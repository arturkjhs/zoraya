# ADR-0221: Where the stack lives: one network service over a separate driver

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0002, ADR-0006, ADR-0050, ADR-0051, ADR-0052, ADR-0053, ADR-0054, ADR-0056, ADR-0080, ADR-0091, ADR-0092, ADR-0097, ADR-0101, ADR-0168, ADR-0220
- **Evidence:** CP-12, CP-14, CP-15, CP-17, DM-047, DM-048, DM-078, DM-161, X-5, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#genode, alternatives-postmortem.md#minix-3, network/survey.md#fuchsias-socket-provider, network/survey.md#genodes-nic-router, network/survey.md#one-stack-as-a-service-or-a-stack-per-process, network/survey.md#other-stacks, network/survey.md#smoltcp, network/survey.md#the-device-virtio-net-under-qemu

## Context

ADR-0220 gives a program a scoped connection to "the network service". Something must own the NIC, run TCP/IP, and
enforce the scope. ADR-0054 puts drivers in user processes with device, interrupt and pin objects, trusted for DMA
without an IOMMU, and `blkd` plus `fsd` is the pattern: a dumb driver and a service that holds the state and parses
the hostile data. The survey weighs one stack as a service against a stack per process behind a router
(`network/survey.md#one-stack-as-a-service-or-a-stack-per-process`); the in-kernel question is *contested* in the
catalog (DM-078). The virtio-net device needs one receive and one transmit queue and no offloads under QEMU's
user-mode networking (`network/survey.md#the-device-virtio-net-under-qemu`). smoltcp 0.14.0 builds for
`aarch64-unknown-none` without `std` (`network/survey.md#smoltcp`, measured).

**What is measured.** The smoltcp build (`network/survey.md#smoltcp`). Nothing on Zoraya: a frame's path through two
processes is unmeasured, as the block path was when ADR-0054 was written.

**What is assumed.** That one TCP/IP stack serving every program is enough for QEMU user-mode networking's throughput,
which is itself modest (the survey found no figure); that hostile packets from the network are the main attack on the
stack, so where they are parsed decides the blast radius.

## Decision drivers

1. **Hostile data is parsed outside the kernel and outside the driver** (ADR-0015 rule 8 as ADR-0054 applies it to
   the block path; DM-047, DM-048, CP-15): every frame from the network is untrusted input.
2. **The scope of ADR-0220 is enforced where every packet passes** (INV-0220-2): resolution and connection by name
   need one place that sees names and packets.
3. **Failure is a value and a restart, never a kernel fault** (CP-12, ADR-0080's restart of the storage stack; MINIX 3's
   restartable drivers, `alternatives-postmortem.md#minix-3`, CP-17).
4. **Bounded memory per client, charged later** (CP-14, ADR-0056): a stack's buffers are what one client can make
   another pay for.
5. **Small driver, replaceable transport** (ADR-0054 driver 5): the driver knows virtio-net, not TCP.
6. **Copies have a known bound** (X-5): a frame crosses at most two process boundaries.

## Considered options

### Option A: TCP/IP in the kernel (Linux, BSD)
- **Description:** The stack runs in the kernel behind socket system calls; the driver too.
- **Evidence for:** Fewest copies and crossings; shares the NIC among processes with the kernel's own protection; the
  standard tools work (DM-078, the pro-kernel position).
- **Evidence against:** Against ADR-0054 and ADR-0002: a packet parser in the trusted base (DM-048), a driver bug a
  kernel fault (DM-047), and no place for a scope by host name without a kernel resolver. Zoraya's kernel has no
  network code and no plan for one.
- **Cost:** *(estimate)* The whole stack in the TCB, for good.
- **Precedent:** Linux, BSD, Windows; the `[contested]` split of DM-078.

### Option B: One network service, `netd`, over a separate driver process, `ethd`
- **Description:** `ethd` is a virtio-net driver on ADR-0054's objects: one receive and one transmit queue, no offloads,
  frames in a memory object it pins (ADR-0051, ADR-0054); it speaks a frame protocol to one client. `netd` holds the
  smoltcp stack, every socket's state, the scopes of ADR-0220, the resolver (ADR-0223) and the configuration
  (ADR-0229), serves programs over ADR-0222's protocol, and is `ethd`'s one client. Both run in jobs `init` creates,
  with `init` as supervisor (ADR-0053). It is Fuchsia's shape: one netstack component that every program reaches
  through a routed protocol (`network/survey.md#fuchsias-socket-provider`, `network/survey.md#other-stacks`), in a
  system whose drivers are user-space components as well (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`:
  Zircon "includes a small set of userspace services and drivers").
- **Evidence for:** Drivers 1 to 6. The stack parses hostile frames in a process that holds no DMA authority and no
  device; the driver holds the device and parses only virtio headers (ADR-0054's split). One place sees every packet
  and every name, so the scope is enforceable (driver 2). Death of either is `PEER_CLOSED` to the other and a restart
  by `init` (ADR-0080's pattern). smoltcp is one process's dependency.
- **Evidence against:** One more hop per frame than Option A, two than kernel bypass (DM-078's cost of a user-space
  service). One stack is one failure domain: a bug in it ends every connection. One single-threaded loop serves every
  client, so a busy client delays others until ADR-0056's accounts and a measured need for threads.
- **Cost:** *(estimate)* `ethd` 600 lines on `zoraya-virtio`'s generic queue; `netd` 3,000 lines over smoltcp plus the
  resolver; a frame protocol crate; one ring of frames per direction in a shared memory object.
- **Precedent:** Fuchsia (netstack3, shipping), Google's Snap and gVisor's netstack (DM-078); MINIX 3 runs its drivers
  and network code as restartable user processes (`alternatives-postmortem.md#minix-3`).

### Option C: A stack in every process, behind a router (Genode's NIC router)
- **Description:** Each program that holds the network links smoltcp and drives it over a NIC session to a router,
  which demultiplexes frames by address and port, applies the session's domain rules and does NAT
  (`network/survey.md#genodes-nic-router`, `alternatives-postmortem.md#genode`).
- **Evidence for:** A hostile peer corrupts only its own stack; a slow program blocks nobody; the router is simpler
  than a stack. Genode ships it.
- **Evidence against:** The router still parses every frame to route it, so the hostile-input surface moves rather
  than shrinks; every program carries a stack and its buffers; each program needs an address or a NAT port range; and
  the router cannot enforce a scope by *name*, since it never sees a name (driver 2 fails: Genode's rules are prefixes
  and ports). Resolution would still need a service.
- **Cost:** *(estimate)* A router of the same size as `netd`'s core, plus smoltcp in every networked program, plus
  per-program configuration.
- **Precedent:** Genode (production); kernel-bypass stacks per application (DM-078).

### Option D: The stack inside the driver process
- **Description:** One process holds the device and the stack.
- **Evidence for:** One hop fewer; one process to start.
- **Evidence against:** The parser of hostile frames then holds DMA authority (DM-161, ADR-0054's trust boundary):
  a bug reached from the network becomes a total compromise. A stack restart is a device reset.
- **Cost:** *(estimate)* Smaller than B by one protocol.
- **Precedent:** None in the files that keeps the split ADR-0054 chose.

## Recommendation

### Decision

Accepted as recommended (architect, 2026-11-04), with item 8 deferred: no way out of the four pages is chosen yet, `--net` and `--gui` stay refused together (ADR-0230 item 2), and the choice is an open decision with the revisit trigger "before the network on the desktop".

The original recommendation follows.

**Recommendation (not a decision): Option B.**

1. **`ethd`.** A process holding one `Device` (the virtio-net transport, item 8), its `Interrupt`, pins for
   its queue memory and for one frame buffer object, and a channel to `netd`. It negotiates `VIRTIO_NET_F_MAC` and
   `STATUS` only (`network/survey.md#the-device-virtio-net-under-qemu`): no checksum or segmentation offload, no
   merged buffers, no control queue; receive buffers of 1526 bytes; one queue pair. It validates every length and
   index the device returns (INV-0054-4's rule) and the frame protocol's requests from `netd`. It parses nothing above
   the virtio header.
2. **The frame protocol** (`zoraya-proto-nic`, ADR-0052). `netd` creates one memory object of `N` frame slots of 1536
   bytes each direction (`N` 64 `[assumption]`, 192 KiB in all), sends it with `attach`, and the two sides exchange
   slot indices in messages: `received { slots[] }` from the driver, `transmit { slots[] }` and `release { slots[] }`
   from the stack. `info` gives the MAC and the link state; `link_changed` is pushed. Copies: device to slot (DMA), slot
   to smoltcp's socket buffer (`netd`), buffer to the stream ring (ADR-0222): two copies in `netd`, one in the kernel
   to the program.
3. **`netd`.** One process, one thread, one port (ADR-0006): smoltcp's `Interface` over a `Device` whose tokens are
   slots of the shared object; the sockets of every program; the scope records; the resolver (ADR-0223); the
   configuration (ADR-0229); the time of smoltcp's `poll` from the monotonic clock. It is a client of `ethd` and of
   nothing else but `/config` and, for SNTP later, its own stack.
4. **`init`'s table.** `init` starts `ethd` in a job with its device handles (the page whose device ID is 1, found as
   ADR-0091 finds the GPU's), then `netd` in a job with the driver channel and a read-only `/config` connection, then
   gives each launcher (the shell, the panel, the POSIX broker) a *root* network connection, scope `any`, rights
   `reach | listen | status | narrow`, as it gives them the volume root (ADR-0053). A machine without a virtio-net
   device starts neither, and the launchers get no connection: every `+net` then fails in the shell with
   `zosh: the network is not available`, and a program never sees a `NET` slot (ADR-0225 item 2).
5. **Failure.** `ethd` dying is `PEER_CLOSED` at `netd`, which marks every connection failed (ADR-0222's `Reset`) and
   exits. `netd` dying closes every stream end and channel it held: by ADR-0101's handle counting each program sees
   `PEER_CLOSED` on its stream ends and on its control channels, which is what an unplugged cable looks like, and
   `libposix` reports a control channel closed without a `reset` message as `ECONNRESET`, and a scope channel closed
   as `ENETDOWN` on the next request (ADR-0225 items 4 and 11). Whichever died, `init` ends both jobs, resets the device through a
   new `ethd` (ADR-0054's restart order), starts `netd`, and sends every launcher a new root connection, at most three
   times per boot (ADR-0080's bound). Running programs are not recovered: their connections are gone, their scope
   channel is dead, and only their launcher can give them a new one (ADR-0053's rule).
6. **How a launcher learns of the restart.** `init` keeps a *services channel* to each launcher it starts, on which it
   sends `NetworkRestarted { root }` after a restart; a launcher whose root connection shows `PEER_CLOSED` refuses
   grants with `Unavailable` until the message comes. This channel is new: ADR-0097 and ADR-0168 give the panel and the
   shell no channel to `init`. It costs one channel per launcher and one message kind, and it is the first time a
   running client is told of a restart rather than restarted itself (ADR-0080 restarts the shell with the storage
   stack); whether ADR-0080 should move to the same shape is an open question.
7. **Relation to ADR-0056.** What the network adds to its accounts, in `netd`: the connections per scope (the stream
   rings are 64 KiB per direction each, kernel memory ADR-0056 charges to their maker, which is `netd`, so `netd` must
   bound them per client); smoltcp's socket buffers in `netd`'s own memory; listening ports; pending resolutions and
   cache entries; frames queued for a slow reader. ADR-0056's `clone(rights, server_end, budget, accounts)` shape fits
   `narrow`: a scope carries a budget of connections. Bandwidth and CPU time are not covered, as ADR-0056 says of CPU.
   An alternative that ADR-0056's rule "A channel is paid by the job that created it" suggests: the program creates the two
   streams and passes `netd` their far ends in `connect`, so its own job pays the 128 KiB and `netd` needs no bound
   for them; ADR-0222 keeps it as an open question.
8. **Transports and pages.** QEMU's `virt` has 32 virtio-mmio transports, eight per page, four pages; ADR-0091 gives
   each driver a page and has `init` refuse a page with two devices, and the stage 5 placement takes all four pages
   with the GUI on (disk on transport 31, GPU 23, keyboard 15, tablet 7). Without the GUI, the NIC goes on transport
   16 (page 2) and the entropy device (ADR-0227) on transport 8 (page 1), within ADR-0091 as accepted; that covers the
   plan's first five tasks. With the GUI there is no page for either, so a desktop with a network needs one of:
   (a) register access by kernel call (ADR-0091's Option D: a `Device` per transport whose `device_read` and
   `device_write` the kernel bounds to the transport's 0x200 bytes, one system call per register access, which
   for virtio is a notify per transmit batch and an acknowledgement per interrupt; DMA and pins unchanged);
   (b) virtio over PCIe (every device its own pages; a PCIe host bridge and ECAM in the kernel or `init`, ADR-0054's
   Option D territory); (c) one driver process for two devices on one page (merging `ethd` and `rngd`, or keyboard
   and tablet in one process, against ADR-0092's one `inputd` process per device). This ADR recommends (a) as the change to ADR-0091 when the
   GUI and the network meet, since it keeps every other ADR as written and also serves the RTC page (ADR-0226); it
   is a superseding note to ADR-0091 and a kernel task of its own in the plan (N6), not part of this ADR's cost.

**Examples.** Nothing is typed here. What a person sees of a dead `netd`:

```
~/proj> fetch https://example.org/ -o page.html
fetch: example.org: connection reset
~/proj> +net example.org fetch https://example.org/ -o page.html
zosh: the network is not available (restarting)
```

**Tensions with accepted ADRs.**
- *ADR-0091* makes one `Device` per page of `virtio,mmio` transports and has `init` identify devices by their
  registers; the network transport is one more device ID (1) in the same scan, no change without the GUI. With the GUI
  on there is no free page (item 8), and the recommended way out, register access by call, is ADR-0091's own Option D
  and a superseding note to it; INV-0091-1 (one device per page) would then become one device per `Device` object.
  ADR-0226 asks for the RTC's page as well, which the same change serves.
- *ADR-0053* "a process has what it was handed": item 6 hands a launcher a new root connection at run time, through a
  channel it was handed at start; no registry appears.
- *ADR-0080* restarts the whole storage stack and the shell; item 5 restarts the network stack without the shell.
  Both rules stand; a superseding ADR-0080 would be needed only to unify them.
- *ADR-0054*'s "the driver is in the trusted computing base" holds for `ethd`; `netd` is not, by this split.

### Effect of the vision
For target 2 the split is what makes a scope enforceable and keeps the frame parser out of the DMA-holding process: a
compromised `netd` can read and forge every program's traffic, which TLS in the program (ADR-0228) is the answer to,
but it cannot touch memory. Threads: `netd`'s one loop is the first service that a measured desktop may want
threaded; smoltcp is single-threaded by design, so the split would be by connection group. Target 3: several
interfaces are several `ethd`s and one `netd`. Changing from B to C later keeps `ethd` and the frame protocol and
replaces `netd`'s core with a router.

## Consequences
- **Positive.** Hostile frames are parsed in a process with no device and no DMA. One place enforces scopes, resolves
  names, and reads the configuration. A dead stack looks to programs like a reset, which they already handle.
- **Negative.** Two copies in `netd` and one in the kernel per transfer. One loop for every client until ADR-0056.
  A new channel from `init` to each launcher.
- **Follow-up.** `zoraya-proto-nic`; `ethd` on `zoraya-virtio`; `netd` and its host-tested logic crate `zoraya-net`
  over a fake device; `init`'s table and the services channel.

## Invariants
- **INV-0221-1.** `netd` holds no `Device`, `Interrupt` or `Pin` handle, and `ethd` holds no connection to any
  program but `netd`. *Check:* an in-QEMU test that enumerates both processes' handle tables by type after start-up
  (INV-0053-2's method).
- **INV-0221-2.** `ethd` treats every device-supplied length and index and every message from `netd` as untrusted,
  and `netd` treats every frame as untrusted: no input panics either. *Check:* host tests of the driver against a fake
  device returning out-of-range values (INV-0054-4's), and deterministic random frame tests of `netd`'s logic crate
  through smoltcp with the panic-as-failure rule.
- **INV-0221-3.** When `netd` ends, every program's stream ends and control channels to it assert `PEER_CLOSED`, no
  program is ended, and a POSIX program's next socket call returns `ECONNRESET`. *Check:* an in-QEMU test with two
  connected programs, one native and one through `libposix`, killing `netd`'s job and reading both programs' signals,
  the C program's `errno`, and both exit records.
- **INV-0221-4.** After `init` restarts the network stack, a launcher's next grant works, and the restart happens at
  most three times per boot. *Check:* an in-QEMU test killing `netd` three times and then a fourth, with a fetch
  between each.

## Open questions
- The frame ring's size (64 slots) and whether `netd` should poll the device or wait on the interrupt only: measure
  the round trip of a ping through both processes under HVF and TCG.
- Whether ADR-0080's restart of the shell should become item 6's notification.
- Whether `netd` needs a second thread before ADR-0056, if one client's slow reads are measured to stall another.
- Which of item 8's three ways the architect prefers for a desktop with a network: deferred by the Decision; the
  plan's N6 waits on it.

- INV-0221-2 (ADR-0001 rule 6, N1 and N2): both halves are checked by host tests (`cargo test -p zoraya-virtio-net -p
  zoraya-proto-nic -p zoraya-net`, with the fuzz target `fuzz/fuzz_targets/net_rx.rs` for the driver and
  `fuzz/fuzz_targets/net_stack.rs` for `netd`'s frames, DHCP replies, driver messages and requests). N2 made the row
  `checked`.
- INV-0221-1 (N2): `netd` and `ethd` each look at their own start-up handle table (`zoraya_user::start_table`) and print
  the result, which `make test-net` expects. This is a report by the process about itself, not a view from outside: the
  kernel has no call that lists another process's handles. A handle made later has a generation and cannot be a
  device, interrupt or pin that `init` did not give.
- INV-0221-3 and INV-0221-4 (N2, partial): `t_netd` kills `netd` and `ethd` with frames in flight, checks that the
  client's channel shows `PEER_CLOSED`, that `ethd` resets the device and exits 0, that `netd` exits when the driver
  goes, and that a new start gets a lease and echo replies (`make test-net`). Not yet: a `libposix` client's
  `ECONNRESET` (the sockets come with N3), the three-restart bound as a test (the code is in `init`; the test needs a
  fetch between kills), and ADR-0221 item 6's services channel. Both rows stay `not yet applicable (stage 8)`.

## Revisit triggers
- Before the network on the desktop: item 8's decision, so that `--net` and `--gui` can combine.
- A measured transfer rate dominated by the two process boundaries: a shared ring between `netd` and programs for
  bulk data (ADR-0051's deferred ring), never the stack in the kernel.
- A second NIC or wireless: one `ethd` per device.
- The architect commits to target 2: ADR-0054's Option D (an IOMMU) applies to `ethd` as to `blkd`.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request. Item 8 (the virtio pages with the GUI) is deferred: `--net` and `--gui` stay refused together, recorded as an open decision with the revisit trigger "before the network on the desktop".
