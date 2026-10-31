# Architecture decision records: index

All ADRs follow [`0000-template.md`](0000-template.md). Statuses and the rules for using ADRs are in [ADR-0001](0001-decision-process.md). Every invariant from every ADR is collected in [`invariants.md`](invariants.md). The independent review of this batch is in [`review-notes.md`](review-notes.md).

Evidence IDs (`DM-`, `GR-`, `CP-`, `X-`) resolve in `docs/research/`; `task-02/…` and `task-02b/…` citations resolve in `docs/research/task-02/` and `docs/research/task-02b/`.

| ADR | Title | Status | One-line summary (recommendation, not decision) | Depends on |
|---|---|---|---|---|
| [ADR-0001](0001-decision-process.md) | How decisions are made and used | accepted | Lightweight ADRs; only the architect sets `accepted`/`rejected`/`superseded`; accepted ADRs bind implementation agents, any deviation needs a new or superseding ADR; invariants get IDs and become CI checks after acceptance. | none |
| [ADR-0002](0002-native-posix-boundary.md) | Boundary between the native ABI and the POSIX personality | accepted | POSIX lives in a per-process library for the data path, a per-session broker for pids, groups and the terminal, and a per-session temporary directory service for `/tmp` and FIFOs; paths resolve through a launcher-built namespace table; the only POSIX-specific kernel facility is a copy-on-write snapshot for `fork`, gated by a job policy bit. | ADR-0001 |
| [ADR-0003](0003-process-creation.md) | Process creation | accepted | Native `spawn` from parts with an explicit handle list, argv and environment vectors, process-group handle and `start_suspended`; `fork` emulated for `posix` jobs by `snapshot_process`; `exec` is spawn plus exit; make, tcc, toybox and clang map onto `spawn` directly, dash's subshells use the snapshot. | ADR-0002 |
| [ADR-0004](0004-async-notification.md) | Asynchronous notification without Unix signals (native side) | accepted | No asynchronous handlers: exit records with a reason field on process objects; request signals (`Interrupt`, `Terminate`, `Hangup`, `Quit`, `Custom`) with kernel-held dispositions; `job_signal` fans out to a whole job atomically; faults go to exception channels with the thread suspended; the personality delivers POSIX handlers at wait points and maps "killed by signal N" through the exit record. | ADR-0002, ADR-0003 |
| [ADR-0005](0005-posix-scope.md) | Scope of POSIX compatibility | accepted | Tier T1 then T2 (autoconf `configure`), each defined by the measured CSV rows (248 / 291 libc symbols, 49 / 59 syscalls) and acceptance workloads; T2 accepted only after fork-emulation cost is measured on W4; threads, `ptrace`, System V IPC, sockets, general `poll`/`epoll`, fault handlers, record locks, `dlopen`, set-uid and T3 are out of scope with written reasons. | ADR-0002, ADR-0003, ADR-0004 |
| [ADR-0006](0006-event-mechanism.md) | Event and readiness mechanism (X-4) | accepted | Handle signals plus a port: persistent, handle-keyed, level-triggered subscriptions; `port_wait` with a monotonic deadline; user packets and port close as the only cancellation; readiness plus non-blocking transfer; completions reserved as a later packet kind on the same port; `pselect`+SIGCHLD and both jobserver styles walked through for every TASK-02b scenario. | ADR-0002, ADR-0003, ADR-0004 |
| [ADR-0010](0010-filesystem-client-guarantees.md) | Filesystem guarantees to clients | accepted | A crash leaves a prefix of the completion order; atomic requests; durability on `sync`; sticky fail-stop on storage errors; unnamed objects with atomic publish. | ADR-0002, ADR-0005, ADR-0006 |
| [ADR-0011](0011-filesystem-crash-consistency-mechanism.md) | Filesystem crash-consistency mechanism | accepted | Copy-on-write tree for data and metadata; commit groups in completion order; dual root records in a ring; mount selects the newest valid root with no replay. | ADR-0010, ADR-0015 |
| [ADR-0012](0012-filesystem-integrity.md) | Filesystem integrity | accepted | CRC32C of every block in its parent pointer; a total decoder with structural validation; one service per volume; no wrong data returned, metadata mismatch is fail-stop. | ADR-0002, ADR-0010, ADR-0011, ADR-0015 |
| [ADR-0013](0013-filesystem-names-and-namespace.md) | Filesystem names and namespace | accepted | UTF-8 names compared byte for byte; control characters forbidden; no hard links; no `..` in the server; symbolic links as inert objects followed by `libposix`. | ADR-0002, ADR-0005, ADR-0010 |
| [ADR-0014](0014-filesystem-metadata.md) | Filesystem metadata | accepted | `mtime`, `ctime` and an advisory mode; strictly increasing times on a volume, across remounts; no access times, owners, ACLs or extended attributes. | ADR-0002, ADR-0010, ADR-0013 |
| [ADR-0015](0015-block-device-contract.md) | Block device contract | accepted | The virtio contract as written: no write is atomic, flush is the only barrier; read-write mount needs flush; errors are typed; the device is untrusted. | ADR-0001 |
| [ADR-0020](0020-kernel-address-space.md) | Kernel virtual address space layout | accepted | 4 KiB granule, 48-bit VA; image at the top 2 GiB; a direct map of RAM, RW and never executable, with holes for the image and reserved regions; separate device, heap and stack windows; one assembly stub to turn the MMU on; guard page below the stack; KASLR later. | ADR-0001 |
| [ADR-0021](0021-physical-memory.md) | Physical memory management | accepted | Per-region two-bit-per-frame bitmap with next-fit search, metadata outside the managed frames; every reserved region excluded; exhaustion is a return value. | ADR-0001, ADR-0020 |
| [ADR-0022](0022-kernel-heap.md) | Kernel heap | accepted | Power-of-two size classes up to 2 KiB over an address-ordered span list in a virtual window backed page by page; infallible allocation only at boot and at bounded call sites, fallible on everything a system call can reach. | ADR-0001, ADR-0020, ADR-0021 |
| [ADR-0023](0023-interrupts-and-time.md) | Interrupts and time | accepted | GICv2 and GICv3 chosen from the device tree; virtual timer; one-shot deadline programming with no periodic tick; monotonic `Instant` in `u64` nanoseconds; deadlines round up so they never fire early. | ADR-0001, ADR-0006 |
| [ADR-0024](0024-kernel-fault-policy.md) | Kernel fault policy | accepted | Fail-stop: a full report then panic for any kernel fault, unexpected interrupt or SError; a stack check in the vectors moves to an emergency stack so an overflow can be reported. | ADR-0001, ADR-0004, ADR-0020 |

## Dependency order

```
ADR-0001
  └─ ADR-0002
       ├─ ADR-0003
       │    ├─ ADR-0004 ──┐
       │    │             ├─ ADR-0005
       │    └─ ADR-0006 ──┘   (ADR-0004 and ADR-0006 were drafted together and
       │                       say explicitly which parts belong to which)
```

## Cross-cutting conflicts each ADR resolves or strains

| Conflict (from `candidate-principles.md`) | Where it is taken up |
|---|---|
| X-1 capability-only access vs POSIX porting | ADR-0002 (namespace table; ambient within the table, accepted deliberately) |
| X-2 no fork vs the C toolchain | ADR-0003 (snapshot for `posix` jobs only), ADR-0005 (fork volume is a T2 property) |
| X-3 no async interruption vs signals | ADR-0004 (wait-point delivery; Option D kept as revisit trigger) |
| X-4 completion vs readiness | ADR-0006 (readiness-first port; completions reserved) |
| X-6 strict commit vs fork emulation | ADR-0003 open question, ADR-0005 staging of T2 |
| X-10 stable ABI early vs freedom to fix | ADR-0001 (what is "in force"), ADR-0003 and ADR-0004 (primitives deliberately not exposed yet) |
| X-12 inescapable containment vs breakaway | ADR-0004 (process groups as sub-jobs; `setpgid` after creation refused) |
| X-15 explicit configuration vs the POSIX layer | ADR-0002 rule 7 (`environ` from the spawn vector; `MAKEFLAGS` slots and `/tmp`) |
