# Quotas survey for TASK-25

The research behind proposed ADR-0260 to ADR-0270 (quotas: how much of the machine a job, a room or a client of a
service may use). Every source in [Sources](#sources) was fetched on 2026-11-10 from the architect's Mac, by helper
agents of this session that wrote notes per page; the note in the list says what was read. Text is paraphrased; short
quotations are marked and were copied by the fetching agent from the page. Pages this session could not read: every
primary page on KeyKOS and EROS space banks and meters (cap-lore.com failed its TLS certificate check, which was not
bypassed; the upenn.edu KeyKOS pages and agorics.com return 404; the ACM page of the EROS paper returns 403;
eros-os.org and coyotos.org return 522; web.archive.org is refused by the fetch tool), Fuchsia's job-policy concept
page, scheduler concept page and any memory-pressure or out-of-memory page (404), and the first 404 copy of the lottery
paper (another copy was read [Q36]). The seL4 manual [Q24] and the lottery paper [Q36] are PDFs whose text was
extracted locally from their compressed streams; word spacing is lost there, so nothing from them is quoted. Only the
first 100,000 of about 143,000 characters of the cgroup v2 page [Q1] were read. What KeyKOS did with space banks rests
on the catalog entry (`../alternatives-postmortem.md`) only, and is marked so.

Facts about Zoraya come from reading the code and documents at `f6d83d3` (*read*), listed in
[Zoraya today](#zoraya-today); nothing was built or run, as TASK-25 requires.

What the research catalog already holds, and this survey does not repeat: overcommit and the OOM killer (DM-030), no
memory-pressure signal to applications (DM-034), process-tree containment (DM-100), cgroup v1's several hierarchies
(DM-111), the candidate principles that every process is in an inescapable containment unit (CP-07) and that
exhaustion surfaces at the request (CP-14). Stage 4's sources on quotas ([K1], [Z8], [Z9], [G2] in
`../stage-4/sources.md#quotas`) are re-read here as [Q1], [Q19], [Q20] and [Q27].

## Systems

### Linux cgroups v2

**What is limited.** The cgroup v2 document [Q1] names four ways of distributing a resource: *weights* (a share among
active siblings, work-conserving, 1 to 10000, default 100), *limits* (a ceiling a child may use up to; the page says limits "can be over-committed": siblings' limits may sum past the parent's), *protections* (`memory.min` and `memory.low`, an amount
kept for a child while its ancestors are within theirs) and *allocations* (exclusive amounts that cannot be
over-committed). Controllers: memory (`memory.min`, `low`, `high`, `max`), pids (`pids.max`), cpu (`cpu.weight`,
`cpu.max` as quota and period, default `max 100000`), io (`io.max` in bytes and operations per second per device,
`io.weight`). Processes live only in leaves when a controller distributes to children (the "no internal process"
rule), and a delegated subtree cannot escape what its parent set.

`setrlimit`'s limits, the per-process baseline, are address space (`RLIMIT_AS`, `ENOMEM` past it), processes per real
user (`RLIMIT_NPROC`, `EAGAIN`), descriptors (`RLIMIT_NOFILE`, `EMFILE`) and CPU seconds (`RLIMIT_CPU`, `SIGXCPU`
then `SIGKILL`) [Q43]. systemd sets cgroup limits per unit (`MemoryMax=`, `CPUQuota=`) and owns the tree that others
are delegated from `[outside evidence: systemd documentation, not fetched]`.

**Who pays.** Memory is charged to the cgroup that "instantiated" it and stays charged there until freed; moving a
process does not move what it already instantiated. What is charged includes page cache, anonymous memory, kernel
structures such as dentries and inodes, and TCP socket buffers (`memory.stat`'s `kernel` and `sock` lines). Memory
shared between cgroups is charged non-deterministically, and the document says it tends over time to end up in a
cgroup with room for it [Q1].

**At the limit.** `memory.high` throttles the group and puts it under reclaim, and exceeding it "never invokes the OOM
killer"; at `memory.max`, if reclaim cannot bring use down, the OOM killer runs inside the group, and use may exceed
the limit for a moment. `memory.oom.group` kills a whole group together or not at all; the killer never kills outside
the group. `memory.events` counts `high`, `max`, `oom` and `oom_kill` events. `pids.max` makes `fork` and `clone`
fail with `EAGAIN` [Q1], which `fork(2)` lists beside `RLIMIT_NPROC` [Q42]. The cpu ceiling throttles the group's
threads until the next period [Q2].

**What went wrong.** The kernel's own OOM killer is a last resort that picks a task in the hope of freeing enough
[Q5]. Facebook's `oomd` README says the kernel can spend unbounded time swapping and evicting page cache before it
acts, cites host lockups of 30 minutes that `oomd` removed, and calls the kernel's policy configuration inflexible
[Q4]; `oomd` acts earlier from user space on pressure stall information, the share of time tasks wait for CPU, memory
or I/O [Q3]. These are the README's claims, not measured here. The pressure files give triggers a program can poll,
rate-limited to one notification per window [Q3]. The CFS bandwidth document notes that a hard ceiling makes a group
wait for the next period even with the CPU idle, and that small periods trade burst capacity for latency [Q2]; LWN
describes the latency cost for a bursty task (30 ms of work under a 25 ms quota per 50 ms period waits a period) and
the burst allowance that banks unused quota [Q35]; the kernel's bandwidth page documents the burst file, so the allowance shipped [Q2]. Shared memory's non-deterministic charge is admitted in
the document itself [Q1]. The v1 memory controller's soft limits are deprecated in favour of `memory.low` and
`memory.min` [Q6]; the redesign from several hierarchies to one is DM-111.

### FreeBSD rctl and jails

`rctl(8)` [Q7] holds rules `subject:id:resource:action=amount/per`: subjects are a process, a user, a login class or a
jail, and `per` says whose total is compared (each process, a user's processes, a class's or a jail's). Twenty-five resources, among them `memoryuse`, `vmemoryuse`, `maxproc`, `nthr`, `openfiles`, `pcpu`, `swapuse`, `msgqqueued`,
`readbps`, `writebps`. Actions: `deny` (the allocation fails; not available for CPU time or I/O rates), `log`,
`devctl` (a notice to `devd`), a signal such as `SIGTERM`, and `throttle` (I/O rates only). Rules apply at once to
running processes. Its BUGS section says that limiting `memoryuse` "may kill the machine due to thrashing", and that I/O counts are taken at the filesystem layer, so writes mostly count the cache, not the disk. The
accounting needs the `RACCT` and `RCTL` kernel options and a loader tunable [Q8]; the handbook's "Resource Limits"
section shows `user:username:maxproc:deny=10/user` and says rules in `/etc/rctl.conf` persist [Q9]. Jails limit
their descendants' count with `children.max` (0 by default, so a jail makes no jails) and their addresses with
`ip4.addr` and `ip6.addr` [Q10]. Who pays for what a daemon outside the jail does for it is not addressed: the charge
follows the process.

### Windows job objects

A job groups processes; a process cannot leave its job, and its children join it unless breakaway is allowed [Q11].
Limits include working-set size, priority, process count (`ACTIVE_PROCESS`), committed memory per process
(`ProcessMemoryLimit`) and for the whole job (`JobMemoryLimit`), independent of each other [Q12], and a CPU rate [Q13]; each applies only when its flag is set in the job's limit flags, so a job is unlimited until one is [Q12]. CPU: a hard cap in units of 1/10000 of the machine per scheduling interval, after which none of the job's
threads run until the next interval; or a relative weight from 1 to 9, which is not a cap; or a minimum and maximum
rate, where minimums across all jobs may not exceed 100% [Q13]. Nested jobs (Windows 8 and later): a process joins
every job up its chain, the effective limit is the strictest in the chain for commit, time and working set, accounting
is aggregated upward, and terminating a job kills from the bottom [Q14]. At a limit some calls succeed and are
silently ignored (raising a working set or priority past the job's) [Q11]; *notification* limits only send a message
to a completion port and let the job continue, and only those messages are guaranteed to arrive [Q11][Q15]; a time
limit set late is added to time already used [Q15]. The pages read do not state the error a process sees at the
memory limit [Q12].

### Apple's memory limits and jetsam

On Apple's mobile systems a process under memory pressure first gets a low-memory notice; if pressure continues, the
system terminates processes, a *jetsam event*, and writes a report whose `reason` is `per-process-limit` (the resident
limit was crossed; extensions have a much lower one), `vm-pageshortage` (background memory freed for the foreground),
`vnode-limit`, `highwater` (a daemon past its expected footprint) or `fc-thrashing` [Q16]. A jetsam kill of a visible
app looks to the person like a crash, but the report has no backtraces [Q16]. `os_proc_available_memory()` returns
what the app may still allocate before its limit, or 0; the value changes constantly and is advisory [Q17]. An
entitlement raises the limit on some devices, and the app must still behave when the extra memory is not there [Q18].
So Apple's answer at the limit is an asynchronous kill with a reason, softened by notices and a query: the model
CP-14 argues against, chosen where the person can relaunch an app and the system must keep the foreground responsive.

### Fuchsia jobs and memory attribution

Zircon's jobs form a tree in which every process is in one job, and the page says they track and limit "basic
resource (e.g., memory, CPU) consumption" [Q19], but neither the job page nor `zx_job_set_policy` [Q20] gives a memory
quota: the policies are yes-or-no per condition (creating a VMO, a channel, a process, a socket, a writable and
executable mapping, a bad handle, and others), with actions allow, deny, allow or deny with an exception, and kill.
`ZX_JOB_POL_TIMER_SLACK` combines with the parent's (the larger slack wins) [Q20]. A process can be made critical to
a job, which is then killed when the process ends [Q21]. Memory is *attributed*, not limited, in the documents read:
`ps` reports per process its private bytes, shared bytes and a proportional share (shared pages divided by the number
of sharers), grouped by job; the page admits that pages mapped twice are counted twice, that kernel memory for handles
is not counted, and that pages of copy-on-write clones may be counted as private by mistake, a fix the page leaves to do [Q22]. Its fair scheduler gives
threads CPU in proportion to their weight, ahead of which deadline tasks run on guaranteed capacity; under overload
all fair threads slow in proportion [Q23]. No page on what Fuchsia does at memory exhaustion could be read.

### seL4 untyped memory and scheduling contexts

The seL4 manual's section on kernel memory allocation [Q24] says the kernel does not allocate memory dynamically for
kernel objects: at boot it hands all free memory to the first thread as *untyped* capabilities, and every kernel
object is made by retyping untyped memory the caller holds, so the caller pays for every object by construction and a
system can divide physical memory exactly between applications. Each untyped region has a watermark; memory is
reused only after every capability derived from it is revoked [Q24][Q25]. The tutorial adds the cost: alignment gaps
are wasted unless the largest objects are made first [Q25]. CPU is the same shape under the MCS extension: a
*scheduling context* is a capability with a budget and a period; a thread with a partial context runs at most its
budget per period; a context is configured through a per-CPU `SchedControl` capability; with the default two
replenishments, a thread that blocks or is preempted more than twice in a period forfeits the rest of its budget
until the next [Q26]. The catalog records the cost in practice: its seL4 entry says building native systems on the kernel is hard (`../alternatives-postmortem.md#l4-family-and-sel4`), and its Barrelfish entry that adopting seL4's capability model for memory made heap allocation very complex (`../alternatives-postmortem.md#barrelfish`).

### KeyKOS and EROS space banks

No primary page was readable in this session (see the first paragraph). The catalog's entry
(`../alternatives-postmortem.md#keykos--eros--capros-and-coyotos`) records, from Hardy's own list of warts, that there
were "abstraction faults and a covert channel in the space bank". Nothing else about banks or meters is claimed here.

### Genode resource trading

Each protection domain's session with the core is a bank account whose balance is its RAM quota; at boot `init`'s
account holds all RAM [Q27]. A parent transfers part of its own quota to a child's account; quota moves only between
an account and its parent's, which prevents stealing across branches; closing an account returns what is left to the
parent [Q27]. *Session quota*: a client attaches quota to a session request, the server spends it on that session,
and it returns to the client when the session closes; the parent aborts the session if the donor's budget is too
small, and the server may use the amount to decide whether, and how well, to serve [Q27]. The session interface names
the arguments `ram_quota` and `cap_quota` and the refusals `Insufficient_ram_quota` and `Insufficient_cap_quota`
[Q30]; its quota guard keeps a limit and a use and refuses a downgrade past what is available [Q31]. Since 17.05 the
number of capabilities a component may hold is a second budget, traded the same way, and every start node of `init`
must state it [Q29]. At the limit mid-session the server returns a scarcity error and the client asks its parent for
an *upgrade*, a round trip. A child's own resource request may be ignored by its parent, "possibly stalling the child", and a parent can ask a child to *yield* quota back, which the child may ignore [Q27]. A
server that does not free everything a session paid for cannot return its quota, which is misbehaviour its parent
may punish; servers must not trust clients, must validate their arguments and must not depend on them for progress
[Q27][Q28]. The catalog adds that the price is that every component must handle running out
(`../alternatives-postmortem.md#genode`).

### Summary

| System | Unit | Limit or share | Who pays for service objects | At the limit | Admitted problem |
|---|---|---|---|---|---|
| Linux cgroup v2 | group tree | ceilings may over-commit; protections; weights | the group that instantiated the memory | `high` throttles, `max` kills inside the group; `pids` `EAGAIN` | shared memory charged at random; late kill; throttling latency |
| FreeBSD rctl | process, user, class, jail | ceilings per rule | the process's subject | deny, signal, log, throttle I/O | memory limit can thrash the machine |
| Windows jobs | nested jobs | strictest in chain; CPU cap, weight or min-max | the process's job | some calls ignored; notices; commit refused | late time limits add to past use |
| Apple jetsam | process | per-process resident limit | the process | asynchronous kill with a reason | looks like a crash |
| Fuchsia | job tree | yes-or-no policies; weights | attributed per process, not limited | policy action (deny, kill) | attribution counts wrongly |
| seL4 | untyped capability | exact, by construction | the caller, always | retype fails | native systems are hard to build |
| Genode | account tree | budgets, moved explicitly | the client, by session quota | scarcity error, upgrade round trip | every component handles running out |

## The service-pays problem

When `fsd`, `netd`, the broker or the compositor allocates for a client, the kernel sees the service's job allocate.
Every system above but two charges the service: cgroups charge the group that instantiated the memory [Q1], rctl the
process [Q7], Windows the job of the process [Q12]. So a client that opens many sessions makes the service pay, and
the service's limit, not the client's, is what fills. Fuchsia does not limit at all in the pages read [Q19][Q22].

The two clean answers:

- **Genode: the client donates.** The client attaches quota to each session, the server allocates from it, and it
  returns on close [Q27][Q30]. Every byte a service holds for a client is in the client's budget. Its costs, as
  Genode states them: an upgrade round trip when a session's quota runs out, a scarcity error every server must
  handle, a close that fails if the server leaked, and a second budget for capabilities that every start node must
  size [Q27][Q29]. Here it would need a quota argument and a scarcity status in every service protocol (ADR-0055's
  filesystem connections, ADR-0222's network, ADR-0104's broker, the compositor's ADR-0093 protocol), and a kernel
  call to move budget between jobs, since a service's heap is its own process's memory.
- **seL4: the client creates.** The client retypes its own memory into the object and hands the server a capability
  [Q24][Q25]. Here the analogue already exists for kernel objects: the job of the process that calls
  `channel_create`, `stream_create` or `memory_create` pays (ADR-0056's "who pays"), so a client that creates the
  rings and buffers it will use, and passes the service one end, pays for them; the network design kept this as an
  open question for stream rings (ADR-0221 item 7, ADR-0222). It cannot cover state that lives in the service's own
  heap: a file description, a TCP control block, smoltcp's buffers, a window's copy in the compositor.

What neither covers cheaply is the service's private state. The middle answer, which ADR-0056 already sketched, is
**accounts in the service**: the service counts what each client holds (connections, attached buffers, held replies,
surfaces) against a per-client bound, and sizes its own job for the number of clients it admits. It is a static
partition, so the service's memory is bounded by the product of the bounds, not by any client's budget, and it
cannot let a busy client borrow an idle one's share. Genode's session quota is the dynamic form of the same thing.

## CPU

A *share* says how CPU is divided among the ready: CFS's group weights [Q32], EEVDF's equal division with lag and
virtual deadlines, where a shorter requested slice gets earlier service without a larger share [Q33], Zircon's
weights [Q23], Windows' 1 to 9 weight [Q13], lottery tickets [Q36]. A *ceiling* says how much a group may use even
when nothing else wants the CPU: `cpu.max`'s quota per period [Q1][Q2], Windows' hard cap per interval [Q13], seL4's
budget per period [Q26], SCHED_DEADLINE's runtime per period with admission control (at most 95% of each CPU by
default) [Q34]. Ceilings are what "this agent may use at most a quarter of the machine" means; shares are what keeps
an interactive program responsive among many busy ones.

The lottery paper adds the piece servers need: a client blocked on a synchronous call transfers its tickets to the
server for the duration, so a server works at its client's rate; currencies isolate the tickets of one trust domain
from inflation in another; a thread that uses part of its quantum gets a compensation ticket so that its share is
kept [Q36]. Without such a transfer, a client limited to 25% can still make a service spend unlimited CPU for it.

Ceilings cost latency: a group out of budget waits for the next period even on an idle CPU [Q2], and a burst longer
than the budget waits a whole period, which LWN's article describes with numbers and Linux answers with a burst
allowance [Q35]; seL4's sporadic replenishment forfeits budget for a thread that blocks often [Q26].

**One CPU versus SMP.** On one CPU, a ceiling is a fraction of that CPU's time, and a share among the ready is plain
proportional scheduling. With several CPUs a percentage needs a reference: Windows' rate is of the whole machine
[Q13], `cpu.max` is CPU time per period, so a quota of two periods is two CPUs [Q2], and seL4 configures a context
per CPU [Q26]. CFS bandwidth hands runtime from a global pool to per-CPU pools in slices of 5 ms; the unused slice left on each CPU lets a group overrun briefly, typically by at most 1 ms per CPU [Q2].

## Storage

Filesystems limit space in three shapes. **Per owner** (`EDQUOT`, "disk quota exceeded" [Q41], which `write(2)`
separates from `ENOSPC`, a full device [Q40]): users and groups, also in `tmpfs` [Q37]. **Per tree**: XFS project
quotas mark a directory tree with a project id, charge new files to it, refuse a hard link whose project differs, and
refuse past the hard limit with `EDQUOT`; a soft limit becomes hard after a grace period; enforcement may come a few
blocks early because the filesystem reserves the most an allocation might need [Q38]. ZFS's `quota` caps a dataset
with its descendants and snapshots, `refquota` the dataset's own data; a descendant's quota does not override an
ancestor's, so both apply; `reservation` guarantees space; user quotas may lag by seconds [Q39]. Windows' File Server Resource Manager limits a volume or a folder, can apply a quota to every existing and future
subfolder, and notifies when a limit is approached or exceeded; its example extends a group's 500 MB quota to 520 MB
for a while so that people can delete [Q44]. **Per mount**:
`tmpfs`'s `size=`, by default half of RAM; its document warns that an oversized `tmpfs` cannot be freed by the OOM
handler and can deadlock the machine [Q37].

A per-tree quota is exact only where a file lies in one tree: XFS has to refuse hard links across projects for that
[Q38]. Zoraya has no hard links (ADR-0013), so every object has exactly one parent directory.

## Zoraya today

*Read* at `f6d83d3`, not run:

- **No charge anywhere.** `kernel/src/job.rs`'s `Job` holds its parent, depth (at most 32), the `posix` bit, its
  signal source and a state of children, processes, a live count, `closed` and `suspended`; no counter of memory or
  CPU. `kernel/src/memory/frames.rs` allocates frames from one global allocator with no owner.
- **The bounds that exist are per object or global**: 255 live processes (8-bit ASIDs); 1024 handles per process
  (ADR-0057); 256 mappings per process (ADR-0051); a channel direction queues at most 64 messages and 256 KiB
  (ADR-0050); a stream ring is 64 KiB per direction (ADR-0101); 1024 user packets per port; 64 queued requests per
  process (ADR-0046); 288 kernel deadlines in one global queue (`kernel/src/timer.rs`), one per sleeping thread; the
  compositor admits 8 clients and keeps a copy of every surface, each up to the display's size (`docs/design.md`, "Known limitations"), with at most 32 windows for all clients together (`MAX_WINDOWS` in `crates/zoraya-compositor/src/lib.rs`); at the 1024 × 768 of `tools/run-qemu.sh` one copy is 3 MiB.
- **Commit is eager.** `memory_create` and `spawn` commit and zero every frame at once; `snapshot_process` copies
  every page with interrupts masked (ADR-0102 items 2 and 8); a user heap grows by 1 MiB steps (`zoraya-heap`'s
  `GROW_SIZE`), so each running POSIX process holds at least one step (`docs/backlog.md`, stage 6e and the ASID test).
- **Scheduler.** One run queue, round robin, one CPU, 10 ms slices armed only when another thread is ready, no
  kernel preemption (`kernel/src/sched.rs`). A thread is charged nothing.
- **Jobs per POSIX process.** The broker makes a session job, a job per process group and a job per process
  (ADR-0104 item 1), so a `make -j4` makes jobs at three levels, and a room's shell adds its room job above them.
- **Rooms.** A room's job is per launcher: the panel's and a shell's job for one room are two jobs (ADR-0172 point 2
  and open question). `room new --untrusted` is refused with a hint naming ADR-0056 (`docs/design.md`, "Rooms").
- **Measured before** (TASK-21, `docs/design.md` "Known limitations"): nothing bounds a fork bomb but the 255
  processes and memory; the ASID test measured 15,045 frames for 255 suspended `hello` processes with 9,391 free under
  128 MiB (`docs/backlog.md`).

## Lessons for Zoraya

1. Refuse at the request, and keep the kill for the person (CP-14, DM-030): every system that kills at the limit
   admits the cost [Q1][Q4][Q16]; Zoraya commits eagerly (ADR-0051), so a refusal always has a caller to tell.
2. Ceilings that may over-commit (cgroups' *limits* [Q1], nested jobs' strictest-in-chain [Q14]) need no sizing of
   every job, which three jobs per POSIX process (ADR-0104) make necessary; carving (Genode [Q27], ADR-0056 Option B)
   guarantees more and wastes what is idle. A protection or reservation is the separate tool for a guarantee [Q1][Q39].
3. The service-pays problem has one exact answer for kernel objects (the creator pays, so let the client create
   [Q24]) and one for service state (donation [Q27], or per-client accounts as its static form). Both are needed.
4. A CPU ceiling is a budget per period; a share is a weight; a client's limit is hollow without lending CPU to the
   service working for it [Q36].
5. Tree quotas are exact where there are no hard links [Q38]; Zoraya has none (ADR-0013).
6. Show use against the limit, and say which limit was reached: `memory.events` [Q1], Windows' violation query [Q15],
   jetsam's reasons [Q16] and `os_proc_available_memory` [Q17] all exist because a bare failure is not enough.

## Sources

Fetched 2026-11-10. "Read" says how much.

- [Q1] Linux kernel documentation, Control Group v2, https://docs.kernel.org/admin-guide/cgroup-v2.html, primary.
  First 100,000 characters: distribution models, delegation, memory, pids, cpu, io, PSI pointers.
- [Q2] Linux kernel documentation, CFS Bandwidth Control, https://docs.kernel.org/scheduler/sched-bwc.html, primary.
  Whole page; it describes the v1 files, the burst file among them.
- [Q3] Linux kernel documentation, PSI, https://docs.kernel.org/accounting/psi.html, primary. Whole page.
- [Q4] Facebook, oomd README, https://github.com/facebookincubator/oomd, primary (project's own claims). README.
- [Q5] Linux kernel documentation, Concepts overview, https://www.kernel.org/doc/html/latest/admin-guide/mm/concepts.html,
  primary. The OOM killer section.
- [Q6] Linux kernel documentation, Memory Resource Controller (v1),
  https://docs.kernel.org/admin-guide/cgroup-v1/memory.html, primary. Deprecation notes.
- [Q7] FreeBSD, rctl(8), https://man.freebsd.org/cgi/man.cgi?query=rctl&sektion=8, primary. Whole page.
- [Q8] FreeBSD, rctl(4), https://man.freebsd.org/cgi/man.cgi?query=rctl&sektion=4, primary. Whole page.
- [Q9] FreeBSD Handbook, Chapter 16 Security, https://docs.freebsd.org/en/books/handbook/security/, primary. Section
  16.14, Resource Limits.
- [Q10] FreeBSD, jail(8), https://man.freebsd.org/cgi/man.cgi?query=jail&sektion=8, primary. Parameters on limits.
- [Q11] Microsoft, Job Objects, https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects, primary.
- [Q12] Microsoft, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
  https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information, primary.
- [Q13] Microsoft, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
  https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information, primary.
- [Q14] Microsoft, Nested Jobs, https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs, primary.
- [Q15] Microsoft, JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION,
  https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_notification_limit_information, primary.
- [Q16] Apple, Identifying high-memory use with jetsam event reports,
  https://developer.apple.com/documentation/xcode/identifying-high-memory-use-with-jetsam-event-reports, primary; its
  text read from https://developer.apple.com/tutorials/data/documentation/xcode/identifying-high-memory-use-with-jetsam-event-reports.json.
- [Q17] Apple, os_proc_available_memory, https://developer.apple.com/documentation/os/os_proc_available_memory,
  primary; text from the `.json` form under `tutorials/data/documentation/os/`.
- [Q18] Apple, com.apple.developer.kernel.increased-memory-limit,
  https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.kernel.increased-memory-limit,
  primary; text from the `.json` form.
- [Q19] Fuchsia, Job, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/job, primary. Whole page.
- [Q20] Fuchsia, zx_job_set_policy, https://fuchsia.dev/reference/syscalls/job_set_policy, primary. Whole page.
- [Q21] Fuchsia, zx_job_set_critical, https://fuchsia.dev/fuchsia-src/reference/syscalls/job_set_critical, primary.
- [Q22] Fuchsia, Memory, https://fuchsia.dev/fuchsia-src/development/kernel/memory/memory, primary. Attribution and
  stated difficulties.
- [Q23] Fuchsia, Zircon fair scheduler, https://fuchsia.dev/fuchsia-src/concepts/kernel/fair_scheduler, primary.
- [Q24] seL4 Foundation, seL4 Reference Manual, https://sel4.systems/Info/Docs/seL4-manual-latest.pdf, primary.
  Sections 2.4 and 2.4.1, text extracted locally.
- [Q25] seL4 Foundation, Untyped tutorial, https://docs.sel4.systems/Tutorials/untyped.html, primary.
- [Q26] seL4 Foundation, MCS tutorial, https://docs.sel4.systems/Tutorials/mcs.html, primary.
- [Q27] Genode Labs, Genode Foundations 23.05, Resource trading,
  https://genode.org/documentation/genode-foundations/23.05/architecture/Resource_trading.html, primary. Whole section.
- [Q28] Genode Labs, Genode Foundations 23.05, Recursive system structure,
  https://genode.org/documentation/genode-foundations/23.05/architecture/Recursive_system_structure.html, primary.
  Component ownership and client-server relationship.
- [Q29] Genode Labs, Release notes 17.05, https://genode.org/documentation/release-notes/17.05, primary. "Assignment
  and trading of capability quota".
- [Q30] Genode Labs, `session/session.h`,
  https://raw.githubusercontent.com/genodelabs/genode/master/repos/base/include/session/session.h, primary (code).
- [Q31] Genode Labs, `base/quota_guard.h`,
  https://raw.githubusercontent.com/genodelabs/genode/master/repos/base/include/base/quota_guard.h, primary (code).
- [Q32] Linux kernel documentation, CFS Scheduler, https://docs.kernel.org/scheduler/sched-design-CFS.html, primary.
- [Q33] Linux kernel documentation, EEVDF Scheduler, https://docs.kernel.org/scheduler/sched-eevdf.html, primary.
- [Q34] Linux kernel documentation, Deadline Task Scheduling, https://docs.kernel.org/scheduler/sched-deadline.html,
  primary. Sections 2 and 4.
- [Q35] J. Corbet, The burstable CFS bandwidth controller, LWN, 2021-02-08, https://lwn.net/Articles/844976/,
  secondary (reporting a patch set; [Q2] documents `cpu.cfs_burst_us`, so it is in the kernel).
- [Q36] C. Waldspurger and W. Weihl, Lottery Scheduling, OSDI 1994,
  https://www.waldspurger.org/carl/papers/lottery-osdi94.pdf, primary. Text extracted locally; sections 2, 3.1, 3.4.
- [Q37] Linux kernel documentation, Tmpfs, https://docs.kernel.org/filesystems/tmpfs.html, primary.
- [Q38] xfs_quota(8), https://man7.org/linux/man-pages/man8/xfs_quota.8.html, primary.
- [Q39] OpenZFS, zfsprops.7, https://openzfs.github.io/openzfs-docs/man/master/7/zfsprops.7.html, primary. `quota`,
  `refquota`, `reservation`, user quotas.
- [Q40] write(2), https://man7.org/linux/man-pages/man2/write.2.html, primary. ERRORS.
- [Q41] errno(3), https://man7.org/linux/man-pages/man3/errno.3.html, primary.
- [Q42] fork(2), https://man7.org/linux/man-pages/man2/fork.2.html, primary. ERRORS.
- [Q43] getrlimit(2), https://man7.org/linux/man-pages/man2/setrlimit.2.html, primary. `RLIMIT_AS`, `RLIMIT_NPROC`
  (per real user, not per process), `RLIMIT_NOFILE`, `RLIMIT_CPU` (`SIGXCPU`, then `SIGKILL`).
- [Q44] Microsoft, File Server Resource Manager, Quota Management,
  https://learn.microsoft.com/en-us/windows-server/storage/fsrm/quota-management, primary. Whole page; fetched
  during the review's fixes.
