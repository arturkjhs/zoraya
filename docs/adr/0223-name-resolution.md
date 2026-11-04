# ADR-0223: Name resolution: in `netd`, bound to the connection's scope

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0002, ADR-0006, ADR-0052, ADR-0055, ADR-0107, ADR-0157, ADR-0171, ADR-0220, ADR-0221, ADR-0222
- **Evidence:** CP-21, CP-28, DM-075, DM-085, GR-008, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, network/survey.md#capsicum-and-casper, network/survey.md#container-and-agent-sandboxes, network/survey.md#fuchsias-socket-provider, network/survey.md#name-resolution-as-authority, network/survey.md#plan-9s-net-and-the-connection-server, network/survey.md#qemu-networking-for-development-and-ci, network/survey.md#smoltcp

## Context

ADR-0220 puts resolution inside the grant: a program may resolve the names it may reach. Something must do the
resolving. On Unix it is libc in the process, blocking, reading `/etc/resolv.conf` and `/etc/hosts` and speaking UDP
to port 53 (DM-075, `network/survey.md#name-resolution-as-authority`), which means every program with the network has
a channel out through the names it asks (MITRE's DNS tunnelling, there). Plan 9 made it a server (`/net/cs`, `/net/dns`,
`network/survey.md#plan-9s-net-and-the-connection-server`), Fuchsia a component with its own routed protocol
(`network/survey.md#fuchsias-socket-provider`), Casper a service whose limits name the hosts that may be resolved and
whose `CONNECTDNS` ties connecting to what was resolved (`network/survey.md#capsicum-and-casper`), and the agent
sandboxes resolve in the proxy and refuse names that resolve to local addresses
(`network/survey.md#container-and-agent-sandboxes`). smoltcp's DNS socket is UDP only, one server, one result, no
search path, no cache (`network/survey.md#smoltcp`). CP-28 asks for resolution as an asynchronous service. This ADR
decides who resolves, under which capability, caching, local names, and how a grant by name follows a host whose
addresses change.

**What is measured.** Nothing on Zoraya. musl's resolver needs `/etc/resolv.conf`, `/etc/hosts`, a UDP socket, `sendto`,
`poll` and a TCP fallback (`network/survey.md#name-resolution-as-authority`); `ethd` and `netd` do not exist.

**What is assumed.** That QEMU's user-mode DNS forwarder at `10.0.2.3` is the resolver in development and tests
(`network/survey.md#qemu-networking-for-development-and-ci`), and a DHCP-given server elsewhere; that a program's
resolutions are few.

## Decision drivers

1. **To resolve a name is authority** (`network/survey.md#name-resolution-as-authority`, GR-008): it must be inside
   ADR-0220's scope, so a program cannot ask for names it may not reach.
2. **One resolver, so one policy** (DM-075's complaint that resolvers differ between libcs): every program sees the
   same answer for the same name.
3. **A grant by name must follow the host** (ADR-0220's assumption): the scope holds the name; the addresses are
   looked up when the connection is made.
4. **No configuration through the environment or per-program files** (CP-21, DM-085, ADR-0157): the servers and the
   local names are declared settings, not `/etc/resolv.conf` in each table.
5. **Blocking only the asker** (CP-28, ADR-0006): a resolution in flight stops neither `netd` nor other programs.
6. **A name must not become a path into the machine** (`network/survey.md#container-and-agent-sandboxes`): a public
   name resolving to a loopback, link-local or own address reaches the host under QEMU (`10.0.2.2` is the host).

## Considered options

### Option A: The libc resolver in every process (the Unix baseline)
- **Description:** `libposix` keeps musl's `getaddrinfo`, reading `/etc/resolv.conf` and `/etc/hosts` from the table
  and speaking UDP to the servers through a datagram socket.
- **Evidence for:** Exactly what C programs expect; no service on the path; works offline from `/etc/hosts`.
- **Evidence against:** The program then needs a reach rule for UDP port 53 to the servers, which is a channel to
  anyone through the names asked (driver 1), and a native program would need the same code in Rust; the lookup blocks
  the calling thread (DM-075); each program may read a different `resolv.conf` (driver 2).
- **Cost:** *(estimate)* Smallest for POSIX; a second resolver for native programs.
- **Precedent:** Unix, musl, glibc.

### Option B: A resolver service of its own, reached by its own capability (Plan 9's `cs` and `dns`, Fuchsia's resolver, Casper's `cap_dns`)
- **Description:** A `dnsd` beside `netd`, holding a datagram socket to the servers; programs hold a channel to it with
  limits on the names they may ask; `netd` knows no names.
- **Evidence for:** Separation: a resolver bug does not touch the stack; Plan 9's `cs` and `dns` and Fuchsia's
  resolver component ship this shape (`network/survey.md#plan-9s-net-and-the-connection-server`,
  `network/survey.md#fuchsias-socket-provider`; the two systems are `alternatives-postmortem.md#plan-9--inferno` and
  `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`); the resolver can be restarted alone.
- **Evidence against:** Two capabilities per program (resolve, reach) that must agree, and `netd` cannot enforce
  `connect(Name)` without asking `dnsd`, so either the program resolves then connects by address (and the scope must
  be by address, losing driver 3) or `netd` calls `dnsd` on every connect (one more hop and a second trusted service
  on the path). Casper's `cap_dns` is obsolete in favour of `cap_net`, which merged them, for this reason
  (`network/survey.md#capsicum-and-casper`).
- **Cost:** *(estimate)* A second service and protocol, and a cross-service call per connection by name.
- **Precedent:** Plan 9, Fuchsia, Casper's older service.

### Option C: Resolution inside `netd`, bound to the connection's scope (Casper's `cap_net` with `CONNECTDNS`)
- **Description:** `netd` holds the resolver: its own query engine over smoltcp's UDP sockets (not smoltcp's DNS
  socket, which is too small), a cache, the servers from the configuration and DHCP, the local names from the
  settings. `resolve(name)` on a scope channel is answered only for a name a reach rule covers. `connect(Name, port)`
  resolves and connects in one request, so the program never handles addresses, and the scope check is on the name.
- **Evidence for:** Drivers 1 to 6: the name and the packet meet in one place; a grant by name follows the host
  because the lookup happens at connect; the policy is one. It is what every agent sandbox does in its proxy
  (`network/survey.md#container-and-agent-sandboxes`) and what Casper's current service does.
- **Evidence against:** `netd` grows a resolver (parsing hostile DNS answers in the same process as the stack); a bad
  resolver cache entry affects every program; a server that is slow holds a request in `netd`'s loop state (never the
  loop itself, driver 5).
- **Cost:** *(estimate)* 800 lines: a query builder and a total answer decoder, a cache, retries, the TCP fallback.
- **Precedent:** FreeBSD's `cap_net` (production); Claude Code's and gVisor's proxies.

### Option D: No names at all: grants and programs use addresses (Genode's router)
- **Description:** Scopes name prefixes and ports only; programs that need names resolve outside the system.
- **Evidence for:** Nothing to resolve; no DNS channel; Genode's router works so.
- **Evidence against:** HTTPS needs the name for the certificate; people type names; a grant by address rots when
  the host moves (driver 3).
- **Cost:** *(estimate)* None, and the system cannot fetch a page.
- **Precedent:** Genode's NIC router.

## Recommendation

**Recommendation (not a decision): Option C.**

1. **Who resolves.** `netd`, with a resolver of its own over smoltcp UDP sockets: A and AAAA queries, retries with
   backoff over the configured servers in order, a fallback to TCP for a truncated answer (as musl's), a total decoder
   for answers (ADR-0052's discipline), and a cache keyed by name with the record's TTL, bounded (`[assumption]` 256
   entries, least recently used out). No search domains: a name is what the program or the grant wrote.
2. **Under which capability.** `resolve(name)` and `connect(Name(name), ...)` on a scope channel (ADR-0222) are allowed
   when a reach rule covers the name (ADR-0220 item 4); `NotGranted` otherwise, before any query is sent. Reverse
   lookup is not offered (no measured user: `getnameinfo` has no call site in curl or wget).
3. **A grant by name follows the host.** The scope holds the name; `netd` resolves at `connect` (or from the cache
   within the TTL) and connects to the addresses it got, trying them in order (IPv4 first until IPv6 is configured;
   ADR-0229). A program that resolves first and then connects by address is allowed only if an `Address` or `Prefix`
   rule covers the address: a `Name` rule does not grant its addresses as addresses. So the common path is
   `connect(Name)`, and `libposix`'s `getaddrinfo` then `connect(sockaddr)` is mapped by ADR-0225 onto it without the
   program noticing.
4. **Local addresses.** A name whose answer is a loopback, link-local, multicast or the machine's own address, or the
   gateway or a DNS server of the current lease or of `net.gateway` and `net.dns` (under QEMU these are the host's
   side, `10.0.2.2` and `10.0.2.3`, but `netd` learns them from the lease and the settings, never from a constant), is
   refused with `Unresolvable { reason: Local }` unless an `Address` or `Prefix` rule covers that address (driver 6).
   Private ranges are allowed, as Claude Code's proxy allows them (`network/survey.md#container-and-agent-sandboxes`);
   names from `net.hosts` (item 5) are the person's own authority and exempt, so a person may point a name at the
   gateway on purpose. The refusal is reported on `REPORT` with the address, so the
   shell's hint can offer `+net ADDRESS` (ADR-0224 rule 5).
5. **Local names.** `net.hosts`, a `list of text` setting (ADR-0171) of `name=address` items, declared by `netd`
   (ADR-0229), replaces `/etc/hosts`: `netd` answers those names without a query. `libposix` does not read
   `/etc/hosts` or `/etc/resolv.conf` (ADR-0225).
6. **Servers.** `net.dns`, a list of addresses, when set; else the servers DHCP gave (ADR-0229); `status` reports which.
7. **Not blocking.** A query in flight is a record in `netd` with a deadline on its port; the asker's request waits
   for the reply as any service request does (ADR-0055's one request per connection), other connections go on.

**Examples.**

```
~/proj> +net example.org fetch https://example.org/ -o page.html
~/proj> +net example.org fetch https://93.184.216.34/ -o page.html
fetch: 93.184.216.34: not granted
  hint: fetch tried to reach 93.184.216.34:443, which it was not given
  to allow it: +net 93.184.216.34:443 fetch https://93.184.216.34/ -o page.html
~/proj> settings net.hosts +'build=10.0.2.100'
~/proj> +net build fetch http://build/status
```

**Tensions with accepted ADRs.**
- *ADR-0107*'s deferred `/etc/hosts`-like names: resolved here as a setting, not a file in the table.
- *ADR-0002 rule 5* lists `/etc/passwd` and `/etc/group` as table entries; no `/etc/resolv.conf` joins them.
- *ADR-0157 rule 3*'s types: `net.hosts` uses ADR-0171's list; nothing new.

### Effect of the vision
For target 2 this is the piece that closes DNS as a channel: an agent asks only the names in its scope, and a name
that points into the machine is refused. Target 3: the same for desktop programs. Threads: none. Moving to Option B
later is splitting `netd` along an internal interface; the scope semantics stay.

## Consequences
- **Positive.** One resolver, one policy, no DNS channel out of a sandbox, grants that follow hosts.
- **Negative.** A resolver inside the stack's process; no reverse lookup; no search domains; `getaddrinfo`'s
  numeric-only and passive forms must be handled in the library (ADR-0225).
- **Follow-up.** The resolver and its decoder in `zoraya-net` with host tests over recorded answers; the local-address
  table; `net.hosts` and `net.dns` (ADR-0229).

## Invariants
- **INV-0223-1.** No DNS query leaves `netd` for a name outside the asking connection's scope. *Check:* the fake-device
  frame recorder of INV-0220-2, asking names in and out of scope and asserting which queries appear.
- **INV-0223-2.** A `Name` rule never allows a connection by address: `connect(Address)` under a scope of names only is
  `NotGranted`, even for an address the name resolves to. *Check:* a host test of the scope matcher.
- **INV-0223-3.** A name resolving to a local address, or to the lease's gateway or DNS server, is refused with
  `Unresolvable { reason: Local }` and reported, unless an address rule covers it; a `net.hosts` name is not. *Check:*
  a host test with recorded answers for each case of item 4, over a scripted DHCP lease.
- **INV-0223-4.** The answer decoder is total. *Check:* deterministic random tests over mutated DNS answers.

## Open questions
- Whether `netd` should also answer `resolve` for a `Wildcard` rule's names that it has never seen connected (it
  does, by this text; a stricter reading would allow only `connect`).
- The cache's bounds and whether negative answers are cached.
- DNS over TCP as the transport to the server by default, or only as the truncation fallback.

## Revisit triggers
- A program in scope needing reverse lookup, search domains or `SRV` records.
- A measured resolver load that argues for Option B's separate process.
- Encrypted DNS (DNS over TLS or HTTPS) once ADR-0228's TLS exists in `netd`'s process.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request.
