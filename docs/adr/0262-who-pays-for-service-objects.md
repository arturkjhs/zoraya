# ADR-0262: Who pays for what a service holds for a client: the client creates what it can, and accounts bound the rest

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0050, ADR-0055, ADR-0056, ADR-0057, ADR-0093, ADR-0100, ADR-0101, ADR-0107, ADR-0153, ADR-0172, ADR-0221, ADR-0222, ADR-0223, ADR-0225, ADR-0260, ADR-0261, ADR-0263, ADR-0264, ADR-0265, ADR-0266, ADR-0269
- **Evidence:** CP-01, CP-14, alternatives-postmortem.md#genode, quotas/survey.md#the-service-pays-problem, quotas/survey.md#genode-resource-trading, quotas/survey.md#sel4-untyped-memory-and-scheduling-contexts, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#freebsd-rctl-and-jails, quotas/survey.md#zoraya-today, quotas/survey.md#cpu

## Context
ADR-0261 charges a kernel object to the job of the process that created it. When `fsd`, `netd`, the broker or the
compositor allocates for a client, that is the service's job, and a room's limit does not see it. What a service holds
per client today (`quotas/survey.md#zoraya-today`, the network ADRs):

- `fsd`: a connection's state, its handle slot (1024 per process, ADR-0057), attached memory objects, a held reply
  (ADR-0055); ADR-0056 sketched per-client *accounts* for exactly this.
- `netd`: per connection two stream rings of 64 KiB per direction, which `netd` creates (ADR-0222), three handles,
  smoltcp's buffers in its heap; listeners; pending resolutions and cache entries (ADR-0223); datagrams queued for a
  slow reader. ADR-0222 bounds connections per scope at 64 `[assumption]` "until ADR-0056's accounts carry a budget",
  and keeps open whether the client should create the rings.
- The compositor: a copy of every surface, up to the display's size each, 32 windows for all clients together (`quotas/survey.md#zoraya-today`).
- The broker and `tmpd`: one per shell (`docs/design.md`, "Rooms"); a broker's pid table and `tmpd`'s 16 MiB volume
  (ADR-0107).

The survey's two clean answers are Genode's (the client donates quota with each session) and seL4's (the client
creates the object from its own memory); the middle answer is a fixed bound per client in the service
(`quotas/survey.md#the-service-pays-problem`). This is the first question the architect is to decide.

## Decision drivers
1. A room's limit must bound what the room can make the machine hold, in services too, or a room with `--mem 64M`
   can make the compositor hold up to 32 surface copies, 96 MiB at 1024 × 768, and take every window from `home` (driver of ADR-0260).
2. A service must not fail every client because one filled it (CP-14 at the service's level): the bound must be per
   client.
3. Protocol changes to accepted services cost: ADR-0055's filesystem protocol, ADR-0222's network protocol and the
   compositor's are in use and tested.
4. No ambient identity (CP-01): a service knows a client only by the connection it came on, so "one client" must be
   a connection tree the launcher made, not a process or a user.
5. Simple, least surprising, safest (TASK-25's rule where accepted ADRs are silent).

## Considered options

### Option A: The service pays, with no bound per client (Linux/FreeBSD baseline)
- **Description:** What a daemon allocates is the daemon's; the client's limit does not see it. This is what cgroups
  and rctl do: the charge follows the process (`quotas/survey.md#linux-cgroups-v2`,
  `quotas/survey.md#freebsd-rctl-and-jails`).
- **Evidence for:** No protocol changes; services stay simple; a service's own limit catches it running away.
- **Evidence against:** One client fills the service and every other client fails (driver 2); a room's limit is
  hollow for everything it does through services (driver 1).
- **Cost:** *(estimate)* None now; the cost is the denial of service.
- **Precedent:** Linux, FreeBSD; Zoraya today.

### Option B: Session quota donated by the client (Genode)
- **Description:** A client's connection carries an amount of its job's memory, moved by the kernel to the service's
  job for the connection's life and returned on close; the service allocates from it, returns a scarcity error when it
  runs out, and the client asks for an upgrade (`quotas/survey.md#genode-resource-trading`).
- **Evidence for:** Exact: every byte a service holds for a client is in the client's budget, at any number of
  clients; the service can serve better when given more. The strongest fit for hostile tenants.
- **Evidence against:** A kernel call that moves budget between jobs (against ADR-0261's "a charge never moves"),
  a quota argument and a scarcity status in every protocol (driver 3), an upgrade round trip, and a close that fails if
  the service leaked (`quotas/survey.md#genode-resource-trading`, `alternatives-postmortem.md#genode`). The service's
  heap must be split into per-client arenas to know what to return. Capabilities needed a second budget in Genode.
- **Cost:** *(estimate)* 200 lines in the kernel, an arena allocator in each service, protocol changes in four
  services: two to three tasks.
- **Precedent:** Genode, since its early releases; capability quota since 17.05.

### Option C: The client creates every object it uses (seL4)
- **Description:** The client creates the rings, buffers and objects the service will use for it and passes the
  service one end, so its job pays by ADR-0261 (`quotas/survey.md#sel4-untyped-memory-and-scheduling-contexts`).
- **Evidence for:** Exact for kernel objects with no protocol beyond passing a handle; no budget moves; window
  buffers are already the client's memory objects; ADR-0222 already names it as the alternative for stream rings.
- **Evidence against:** It cannot cover state in the service's own heap: a file description, a TCP control block,
  smoltcp's buffers, the compositor's copy of a surface (it must be memory the client cannot write, so the client
  cannot create it).
- **Cost:** *(estimate)* One protocol change in `netd`'s `connect` and `accept`.
- **Precedent:** seL4's retype; Zoraya's own ADR-0055 pattern of the client making its connection.

### Option D: Client-created kernel objects where possible, and accounts with fixed bounds for the rest
- **Description:** Option C for kernel objects the client will use; for state the service must hold itself, an
  *account* per client (ADR-0056's design, kept and extended): a tree of connections that one launcher grant started,
  with a fixed bound per kind of state. A service's own size is the bound times the number of accounts it admits, and
  it refuses a new account past that number.
- **Evidence for:** Drivers 1, 2, 4, 5. No kernel change and no budget moves; the bounds are counts a service
  already keeps (ADR-0222's 64 connections per scope is one). It is the static form of Genode's session quota
  (`quotas/survey.md#the-service-pays-problem`).
- **Evidence against:** A static partition: a busy client cannot borrow an idle one's share, and a service is sized
  for its worst case. A room's limit sees the client-created objects but not the account's state, which is bounded by
  the account, not by `--mem`.
- **Cost:** *(estimate)* 150 to 300 lines per service, one protocol change in `netd`.
- **Precedent:** ADR-0056's accounts; Genode's session quota in its static form.

## Recommendation
**Recommendation (not a decision): Option D**, with Option B recorded as the upgrade when a service's static size is
measured to be too large or too small.

1. **Client creates.** A kernel object that exists for one client's use is created by the client and passed to the
   service:
   - `netd`: `connect` and `accept` take the client's far ends of two streams (ADR-0101) the client created, instead
     of `netd` making them; the reply `connected { peer }` then carries no handles. The 128 KiB per connection is the
     room's memory from the request on, also for a connection that fails, until the client closes its ends. `netd`
     checks that both handles are streams of ADR-0101's capacity, distinct, empty, with the rights it needs, and uses
     them only with non-blocking calls, so a client cannot make `netd` wait on it (the rule Genode states for
     servers, `quotas/survey.md#genode-resource-trading`); a client that keeps duplicates of the far ends can only
     read or write its own connection's data. This answers ADR-0222's open question and changes its recommendation,
     which needs the architect (tension below).
   - The compositor: window buffers are already the client's memory objects (ADR-0093).
   - `fsd`: attached memory objects are already the client's (ADR-0055).
2. **Accounts.** Every service that holds state per client keeps accounts:
   - **What an account is.** The connections reached from one launcher grant: a connection made by `open`, `clone`,
     `narrow` or `accept` joins its parent's account; `clone(rights, server_end, budget)` with a budget makes a new
     account, carved from the caller's account (ADR-0056's shape, kept). The launcher of an untrusted room (one at a
     time, ADR-0269) makes one account per service when it first launches into the room, so "one client" means "one
     untrusted room" for every service. Trusted rooms and `home` share the session's account.
   - **Bounds, carved from each service's global bound** `[assumption, every value]`, so that all accounts together
     never pass what the service can hold, and the untrusted accounts together leave the session most of it:

     | Service | Global bound today | Per untrusted account | All untrusted accounts | Session's account |
     |---|---|---|---|---|
     | `fsd` | 896 open (`MAX_OPEN`, ADR-0055), 240 mapped buffers | 32 open, 4 buffers, 1 held reply | 4 accounts: 128 open, 16 buffers | the rest: 768 open, 224 buffers |
     | `netd` | 64 connections per scope (ADR-0222); 256 resolver cache entries (ADR-0223) | `--conns` (default and most 16), 0 listeners unless `+listen`, 4 pending resolutions, 16 cache entries, 4 datagram sockets | 4 accounts: 64 connections, 64 cache entries | as ADR-0222 has it |
     | compositor | 32 windows (`MAX_WINDOWS`), 8 clients | 3 surfaces, 2 display areas of pixels | 4 accounts: 12 surfaces, 8 display areas | the rest: at least 20 windows |
     | broker, `tmpd` | per shell | none: an untrusted room's are in its job (ADR-0265) | — | — |

     So every service admits at most four untrusted accounts, and the room keeper runs at most four untrusted rooms
     at once (ADR-0269 item 2): each room's bounds are its own, and no room is refused below them because others
     filled the column. smoltcp's buffers in `netd` are a fixed size per connection `[assumption]`, so the connection bound bounds them;
     a datagram socket's queue is ADR-0050's per-channel bound.
   - **At the bound.** The request of that account alone is refused, nothing of any other account changes, and the
     service tells the account's launcher (a count it reads), which forwards it to the room keeper (ADR-0269) for the
     shell's hint and `top`. Statuses: `fsd` keeps `TooManyOpen` (ADR-0055; `ENFILE` by ADR-0100); `netd`'s `connect`
     gains a failure reason `OverQuota` and the compositor's `CreateToplevel`, `CreatePopup` and buffer attach a
     refusal `OverQuota` (ADR-0263; `ENOBUFS` for sockets, a new row of ADR-0225 item 11).
   - **A service's size.** A service has no job ceiling (ADR-0266 item 4); its worst case is its global bound, which
     the carve does not raise. On 128 MiB the compositor's worst case (32 copies of the display) is larger than the
     services' quarter, as it is today (`docs/design.md`, "Known limitations"); the untrusted part of it is 8 display areas.
3. **Queued data.** By ADR-0261 point 7 the writer pays for queued messages, so a service that writes replies a slow
   client does not read pays for them: bounded by the per-channel queue (ADR-0050) times the account's connections. Stream rings are fixed at creation and cost the writer nothing more.
4. **CPU spent for a client** is not charged to it in stage 8. A service serves its accounts in turn, one request per
   account per pass `[assumption]`, so a flooding account delays others by at most one request each pass. Lending the
   client's CPU to the service (lottery's ticket transfer, `quotas/survey.md#cpu`) waits for a measured need
   (ADR-0264).
5. **Later, Option B.** If a service's worst-case size is measured to waste memory the machine needs, the kernel
   gains `job_donate(from, to, bytes)` with return on close, and that service's protocol a quota argument. Nothing in
   points 1 to 4 precludes it.

**Tensions with accepted ADRs.** *ADR-0222:* `connect` and `accept` change shape (the client passes two stream ends);
its recommendation had `netd` make them, "simpler for the client". *ADR-0221 item 7:* fulfilled: the scope carries a
budget, as it foresaw. *ADR-0055:* `clone` gains a budget argument, as ADR-0056 proposed. *ADR-0093* and the
compositor's protocol: `CreateToplevel`, `CreatePopup` and a buffer attach gain an `OverQuota` refusal. *ADR-0222*
again: `connect`'s failure reasons gain `OverQuota`; *ADR-0225 item 11:* a new `errno` row, `ENOBUFS`. *ADR-0153*,
*ADR-0172:* the launcher of an untrusted room makes one account per service. *ADR-0223:* the resolver's shared cache
gains a bound per account.

**Examples.** What a person sees when a program in `agent-1` opens its seventeenth connection:

```
agent-1 ~/sandbox/agent-1> fetch https://api.example.com/a
fetch: api.example.com:443: over quota
hint: agent-1 is at its connection limit (16 of 16)
the untrusted rooms may hold 16 connections each
```

And in `top`'s room view, the services' columns:

```
ROOM      MEM            CONNS      FILES     SURFACES
agent-1   498M of 512M   16 of 16   9 of 32   2 of 3    (refused: 1 connection)
```

### Effect of the vision
For untrusted agents every service a room can reach bounds what the room can make it hold, and the room cannot
starve another room's service state. For the general-purpose base the session keeps today's bounds less what untrusted rooms may take (768 of `fsd`'s 896 open connections, at least 20 of the compositor's 32 windows), and all of them while no untrusted room exists. If Zoraya grows into many tenants, Option B is the step, and the account is where its
quota would attach.

## Consequences
- **Positive.** No kernel change beyond ADR-0261; every service's worst case is a product of written numbers; the
  network's rings move into the room's memory.
- **Negative.** Static partitions waste memory a busy room could use; each service carries account code; a room's
  service state is not in `--mem`, so `top` shows it separately.
- **Follow-up.** Accounts in `fsd` and the broker (Q3), in `netd` with the client-created rings (Q3 with N6), in the
  compositor (Q6); the sizes in ADR-0266.

## Invariants
- **INV-0262-1.** In each service, an account at a bound gets that bound's refusal, and no request of any other
  account is refused because of it. *Check:* host tests in each service's crate: one account opening, attaching or
  creating without end while a second does the same to its full bound.
- **INV-0262-2.** Every account's state and every connection belongs to exactly one account, and closing an account's
  last connection frees its state. *Check:* host model test of each service's account table with random opens,
  clones, attaches and closes.
- **INV-0262-3.** In each service, the bounds of all accounts together never pass the service's global bound, and the
  untrusted accounts together never pass their column of the table. *Check:* host model test per service with random
  `clone`s with budgets, opens and closes, comparing the sums with the crate's constants.
- **INV-0262-4.** A `netd` connection's stream rings are charged to the client's job. *Check:* in-QEMU test: a client
  in a limited job connects and `job_get_usage` grows by the two rings, and `netd`'s does not.

## Open questions
- The bounds in the table: measure the tour, the POSIX session and an agent-like workload (many short connections).
- Whether the compositor's pixel budget should count the display's scale (a scale-2 surface is four times the bytes).
- Whether one request per account per pass is fair enough in `fsd`, whose requests vary from a `stat` to a 1 MiB write.
  *Note (Q3, 2026-11-11):* `fsd` serves ready connections in the order the port gives them, not by account; it does
  not yet take one request per account per pass. A flooding account's requests are bounded by its connections (32), not
  by a pass. Not changed; measure when Q7 floods the service.
- The bounds of the table, `fsd` part measured (Q3, 2026-11-11): an untrusted account (32 open, 1 held) costs the
  service at most about 90 KB (`zoraya-fs-service`'s `worst_case`); the whole service is about 59 MB, 56 MiB of it the
  session's possible held replies (`MAX_HELD` is `MAX_OPEN`). Details and the other choices are in proposed ADR-0291.

## Revisit triggers
- A service's static size measured to keep memory from rooms that need it: consider Option B.
- A client measured to make a service spend more CPU for it than its room's ceiling: consider lending.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The client creates the kernel objects it uses, and services keep fixed per-room bounds; Genode-style donation (Option B) stays the recorded later step. ADR-0222's `connect` changes so that the client supplies the connection buffers; ADR-0222 and ADR-0225 carry a note saying so. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
