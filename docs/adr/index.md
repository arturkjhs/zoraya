# Architecture decision records: index

All ADRs follow [`0000-template.md`](0000-template.md). Statuses and the rules for using ADRs are in [ADR-0001](0001-decision-process.md). Every invariant from every ADR is collected in [`invariants.md`](invariants.md). The independent review of this batch is in [`review-notes.md`](review-notes.md).

Evidence IDs (`DM-`, `GR-`, `CP-`, `X-`) resolve in `docs/research/`; `task-02/…` and `task-02b/…` citations resolve in `docs/research/task-02/` and `docs/research/task-02b/`.

| ADR | Title | Status | One-line summary (recommendation, not decision) | Depends on |
|---|---|---|---|---|
| [ADR-0001](0001-decision-process.md) | How decisions are made and used | proposed | Lightweight ADRs; only the architect sets `accepted`/`rejected`/`superseded`; accepted ADRs bind implementation agents, any deviation needs a new or superseding ADR; invariants get IDs and become CI checks after acceptance. | none |
| [ADR-0002](0002-native-posix-boundary.md) | Boundary between the native ABI and the POSIX personality | proposed | POSIX lives in a per-process library for the data path, a per-session broker for pids, groups and the terminal, and a per-session temporary directory service for `/tmp` and FIFOs; paths resolve through a launcher-built namespace table; the only POSIX-specific kernel facility is a copy-on-write snapshot for `fork`, gated by a job policy bit. | ADR-0001 |
| [ADR-0003](0003-process-creation.md) | Process creation | proposed | Native `spawn` from parts with an explicit handle list, argv and environment vectors, process-group handle and `start_suspended`; `fork` emulated for `posix` jobs by `snapshot_process`; `exec` is spawn plus exit; make, tcc, toybox and clang map onto `spawn` directly, dash's subshells use the snapshot. | ADR-0002 |
| [ADR-0004](0004-async-notification.md) | Asynchronous notification without Unix signals (native side) | proposed | No asynchronous handlers: exit records with a reason field on process objects; request signals (`Interrupt`, `Terminate`, `Hangup`, `Quit`, `Custom`) with kernel-held dispositions; `job_signal` fans out to a whole job atomically; faults go to exception channels with the thread suspended; the personality delivers POSIX handlers at wait points and maps "killed by signal N" through the exit record. | ADR-0002, ADR-0003 |
| [ADR-0005](0005-posix-scope.md) | Scope of POSIX compatibility | proposed | Tier T1 then T2 (autoconf `configure`), each defined by the measured CSV rows (248 / 291 libc symbols, 49 / 59 syscalls) and acceptance workloads; T2 accepted only after fork-emulation cost is measured on W4; threads, `ptrace`, System V IPC, sockets, general `poll`/`epoll`, fault handlers, record locks, `dlopen`, set-uid and T3 are out of scope with written reasons. | ADR-0002, ADR-0003, ADR-0004 |
| [ADR-0006](0006-event-mechanism.md) | Event and readiness mechanism (X-4) | proposed | Handle signals plus a port: persistent, handle-keyed, level-triggered subscriptions; `port_wait` with a monotonic deadline; user packets and port close as the only cancellation; readiness plus non-blocking transfer; completions reserved as a later packet kind on the same port; `pselect`+SIGCHLD and both jobserver styles walked through for every TASK-02b scenario. | ADR-0002, ADR-0003, ADR-0004 |

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
