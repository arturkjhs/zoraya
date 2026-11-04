# ADR-0220: The network capability: a scoped connection to the network service

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0002, ADR-0005, ADR-0040, ADR-0050, ADR-0053, ADR-0055, ADR-0056, ADR-0057, ADR-0101, ADR-0107, ADR-0153, ADR-0155, ADR-0172
- **Evidence:** CP-01, CP-02, CP-28, DM-009, DM-014, DM-074, DM-076, GR-008, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#genode, alternatives-postmortem.md#plan-9--inferno, network/survey.md#capsicum-and-casper, network/survey.md#container-and-agent-sandboxes, network/survey.md#fuchsias-socket-provider, network/survey.md#genodes-nic-router, network/survey.md#name-resolution-as-authority, network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak, network/survey.md#openbsd-pledge-and-unveil, network/survey.md#plan-9s-net-and-the-connection-server, network/survey.md#posix-sockets-what-curl-and-wget-call, network/survey.md#the-unix-baseline, network/survey.md#wasi-sockets, network/survey.md#what-a-grant-names

## Context

Zoraya has no network. A program reaches only what it is given (CP-01, ADR-0002), and the network is where that rule
is least honoured elsewhere: on Unix any process may open a socket to anywhere (DM-009,
`network/survey.md#the-unix-baseline`), the retrofits name a subsystem (pledge's `inet`,
`network/survey.md#openbsd-pledge-and-unveil`) or one bit (Android, macOS, Flatpak,
`network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak`), and even the capability systems hand out the
whole network as one handle (Fuchsia's socket provider, `network/survey.md#fuchsias-socket-provider`; Plan 9's mounted
`/net`, `network/survey.md#plan-9s-net-and-the-connection-server`). The two designs that let a launcher say "this host
and no other" are Casper's `cap_net` limits and the proxies of today's agent sandboxes
(`network/survey.md#capsicum-and-casper`, `network/survey.md#container-and-agent-sandboxes`). This ADR decides what a
Zoraya program is given to reach the network, how it is narrowed and passed on, and what a program without it can do.
The service that enforces it is ADR-0221, the protocol over it ADR-0222, resolution ADR-0223, and the words a person
types ADR-0224.

**What is known.** The survey's table (`network/survey.md#what-a-grant-names`) shows what each system's grant names:
nothing finer than a subsystem in Unix and pledge; host names, addresses and ports in Casper and the agent sandboxes;
prefixes and ports in Genode's router; a handle that "represents access to (a subset of) the network" in WASI, with the
subset left to the runtime (`network/survey.md#wasi-sockets`). Resolution is an authority of its own: a program that may
resolve any name can send data in the names (`network/survey.md#name-resolution-as-authority`), and a program that may
not resolve cannot reach anything by name (GR-008's tcpdump). Zoraya's own vocabulary for grants exists: a connection
whose rights the service enforces (ADR-0055), narrowed by `narrow` to one name (ADR-0153 rule 2), given by the shell
from the operands a person typed or from `+r`/`+w`, recorded in rooms (ADR-0172), with CP-28's principle and ADR-0153's and
ADR-0172's vision sections already naming `+net HOST` as the extension.

**What is measured.** Nothing on Zoraya. The programs Zoraya will want are plain stream clients that connect to a
host and port they were told (`network/survey.md#posix-sockets-what-curl-and-wget-call` is read in ADR-0225).

**What is assumed.** That a person will read a host name in a grant as they read a path in `+r PATH`
(ADR-0153 made the same assumption for paths). That the addresses behind a name change more often than the name,
so a grant by address would rot.

## Decision drivers

1. **No ambient network** (CP-01, DM-009): a program with no grant reaches nothing, not even a resolver.
2. **A grant names what a person means**, a host and a port, not a subsystem or a bit (DM-014's lesson that heavy
   policy is not used, and the survey's finding that one bit says nothing about where data goes,
   `network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak`).
3. **Resolution is inside the grant** (`network/survey.md#name-resolution-as-authority`; Casper's `CONNECTDNS`,
   `network/survey.md#capsicum-and-casper`): to resolve a name is to be allowed to reach it.
4. **Rights only narrow and travel as handles** (CP-02, ADR-0040, ADR-0055's `clone`): a program can hand a sub-grant
   to a helper, never widen one.
5. **One vocabulary with the files** (ADR-0153, ADR-0172): the same table entry, the same `grants` words, the same
   room record.
6. **Typed endpoints, not cast structures** (CP-28, DM-074): what the native protocol carries is a host name or an
   address and a port, checked by a total decoder.

## Considered options

### Option A: Any process, any socket (the Unix baseline)
- **Description:** Every process may create sockets and connect anywhere; the only policy is the user's identity and
  the port-1024 rule (DM-076).
- **Evidence for:** Every networked program works as written; nothing to grant, nothing to explain.
- **Evidence against:** It is ambient authority in its purest form (DM-009, `network/survey.md#the-unix-baseline`); an
  agent in an untrusted room (ADR-0172) could reach anything; it contradicts ADR-0002 outright. Every sandbox in the
  survey exists to undo it (DM-014).
- **Cost:** *(estimate)* None now; the isolation direction becomes impossible.
- **Precedent:** Unix, Linux, Windows.

### Option B: One handle for the whole network (Fuchsia's socket provider; one bit)
- **Description:** A program holds a channel to the network service or does not. Holding it is holding the whole
  network; resolution is a second handle, as Fuchsia routes `fuchsia.posix.socket.Provider` and `fuchsia.net.name.Lookup`
  separately (`network/survey.md#fuchsias-socket-provider`, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence for:** The simplest capability: a handle, routed by the parent (ADR-0053), nothing to parse. Fuchsia ships
  it. Android, macOS and Flatpak show people accept one bit.
- **Evidence against:** The bit says nothing about *where* data goes
  (`network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak`); an untrusted room could be given the
  network or nothing, never "its API and nothing else", which is the use the task names. The two-handle split does
  let a launcher deny resolution, but then a program with the network handle still reaches any *address*.
- **Cost:** *(estimate)* Smallest; a channel per program.
- **Precedent:** Fuchsia (shipping), Android `INTERNET`, macOS entitlements, Flatpak `--share=network`.

### Option C: A scoped connection: a channel whose rights are a set of destinations and listening ports (Casper, the agent sandboxes, Genode's domains)
- **Description:** A program is given a channel to the network service (ADR-0221). The service keeps, per connection,
  a *scope*: a set of **reach rules** (a host name, a `*.` wildcard, an address or a prefix; a port, a range or any; TCP,
  UDP or both), and a set of **listen rules** (a port or range, TCP, UDP or both). Resolution is permitted for exactly
  the names in the reach rules; a connection to a host is permitted when the host and port match a rule; a listen when
  the port matches a listen rule. `narrow(scope, server_end)` makes a new connection with a subset. A program holding
  nothing has no channel. The scope is written by the launcher (the shell from `+net`, a room's record, a description's
  operand), never by the program, which is the difference from Casper, where the program limits itself
  (`network/survey.md#capsicum-and-casper`).
- **Evidence for:** Drivers 1 to 6. It is Casper's `cap_net` with its limits set from outside, the allow-list every
  agent sandbox converged on (`network/survey.md#container-and-agent-sandboxes`), and Genode's domain rules in a form
  that can name hosts (`network/survey.md#genodes-nic-router`, `alternatives-postmortem.md#genode`). It is ADR-0055's
  shape exactly: the kernel sees a channel, the service enforces rights per connection, `narrow` only subtracts.
- **Evidence against:** A filter to parse and keep per connection; the service must resolve names to enforce
  "host", so it is the resolver too (ADR-0223), and a name that resolves to a changed address is allowed by the name,
  which is both the point and a risk (DNS rebinding, met in ADR-0223). Wildcards and ranges are policy a person must
  read. It cannot be enforced by a stack per process (ADR-0221).
- **Cost:** *(estimate)* The scope type and its total decoder in `zoraya-abi` (200 lines), matching and narrowing
  in the service (300 lines), the words in `zosh` (ADR-0224).
- **Precedent:** Casper (FreeBSD, production), Claude Code's and Deno's allow-lists (production), Genode's router
  (production, prefixes only).

### Option D: Pre-opened connections only (socket activation, Capsicum's delegation)
- **Description:** A program never names a destination: its launcher connects and hands it the stream, as a service
  manager passes a listening socket to a daemon (DM-076's alternative) and as Capsicum delegates descriptors
  (`network/survey.md#capsicum-and-casper`).
- **Evidence for:** The strongest form: the program holds a byte stream and nothing else, and ADR-0101's stream ends
  are already what a connection would be (ADR-0222). No filter anywhere.
- **Evidence against:** A client that opens connections as it goes (HTTP redirects, several hosts for one page, a
  retry) cannot be served; the person would have to open every connection by hand. It is GR-008's lesson in reverse:
  fine for programs that take what they are given, useless for programs that name hosts late.
- **Cost:** *(estimate)* Nothing beyond ADR-0222.
- **Precedent:** systemd and launchd socket activation; Plan 9's `import` of a remote `/net`
  (`network/survey.md#plan-9s-net-and-the-connection-server`) as the whole-network form, from the system whose
  namespaces are `alternatives-postmortem.md#plan-9--inferno`'s lasting idea.

## Recommendation

### Decision

Accepted as recommended (architect, 2026-11-04), with one fix: a `Wildcard` covers the names under its suffix and not the suffix itself, as a TLS wildcard does; `*.example.org` does not grant `example.org`. Item 2 and the subset relation say so (`Name(n)` is under `Wildcard(s)` only when `n` ends in `.s`). The capability names what Option C says: names, wildcards, addresses, prefixes and `any`, with optional ports.

The original recommendation follows.

**Recommendation (not a decision): Option C**, with Option D kept as what a scope of one connection amounts to: a
program given a connected stream and no channel holds exactly that connection.

1. **The object.** A *network connection* is a channel end to `netd` (ADR-0221) with a server-side record holding a
   scope and the connection's rights. The kernel knows a channel (ADR-0050: `READ`, `WRITE`, `WAIT`, `TRANSFER`, no
   `DUPLICATE`); the scope is `netd`'s, as a directory connection's rights are `fsd`'s (ADR-0055).
2. **The scope** (`zoraya_abi::net::Scope`, flat, total decoder, at most 64 rules `[assumption]`):
   - a reach rule is `{ target, ports, protocols }` with `target` one of `Name(ascii)` (an exact host name),
     `Wildcard(ascii)` (every name under a suffix, written `*.example.org`: it covers `a.example.org` and
     `a.b.example.org`, not `example.org` itself, as a TLS wildcard does not), `Address(ip)`, `Prefix(ip, len)` and
     `Any` (every name and every address, the word `any` of ADR-0224); `ports` a set of at most 8 ranges
     `[assumption]` or `Any`; `protocols` a set of `Tcp`, `Udp`;
   - a listen rule is `{ ports, protocols }`;
   - names are ASCII, lower case, at most 253 bytes, labels of at most 63 (DNS's limits, which the survey's resolver
     sources follow); the decoder refuses upper case and non-ASCII (the shell lowers the case when it builds the
     scope; internationalised names are not handled, an open question); no name holds an address, no address a name.
   A scope with no rules is a connection that may only ask `status` (ADR-0229): it is what a program is given when
   its launcher wants it to see the network's state and nothing else.
3. **Rights** on a network connection, kept by `netd` and only narrowed: `reach`, `listen`, `status`, `narrow`. A
   request lacking its right or outside the scope gets `NotGranted`, the status ADR-0155 already names for a refused
   path, so programs and the shell report both the same way (ADR-0224). A refusal because a name's answer is a local
   or host-side address (ADR-0223 item 4) is `Unresolvable` with a reason, reported too.
4. **Resolution is in the scope** (driver 3): `resolve(name)` succeeds only for a name a reach rule covers; the
   addresses it returns are the ones a `connect` to that name would use (ADR-0223 decides caching and the rebinding
   rule). A program with an `Address` rule and no `Name` rule cannot resolve anything.
5. **Narrowing and passing on.** `narrow(scope, rights, server_end)` attaches a new connection whose scope is a subset
   of the caller's and whose rights are no wider (as `clone` and `narrow` of ADR-0055). A rule is a subset of another
   when its target is equal or under it (every target under `Any`; `Name(n)` under `Wildcard(s)` when `n` ends in
   `.s`; `Wildcard(a)` under `Wildcard(b)` when `a` ends in `.b`; `Address` inside `Prefix`; `Prefix` inside a
   shorter `Prefix` that covers it), its ports inside, its protocols inside; `Name` and `Address` targets are never
   under each other. The channel end moves with `TRANSFER`, in a `spawn` list or a message, so a program can give
   a helper part of what it holds and nothing more.
6. **What a program without the capability can do.** Nothing: it has no channel to `netd`, so there is no call to
   refuse. `libposix`'s `socket()` still succeeds (ADR-0225), and the first `connect`, `bind`, `sendto` or
   `getaddrinfo` fails with `EACCES` and is reported (ADR-0224). A native program's `zoraya-net` client reports
   `NotGranted` from its first request.
7. **Where it sits in the records.** ADR-0153's `Startup` and ADR-0107's `Bootstrap` gain a fixed slot `NET`, the
   network connection, present only when the launcher granted anything. Several grants on one line are one connection
   whose scope is their union, so a program holds at most one (ADR-0224). A network grant also adds the read-only
   roots entry of ADR-0228 to the table (ADR-0224 item 1).
8. **Established connections are handles too.** A `connect` returns stream ends and a control channel (ADR-0222);
   those can be passed to another process, which then holds that one connection and nothing else (Option D's form).

**Examples.** What a person types is ADR-0224's; here only what the capability is.

```
~/proj> grants +net api.example.org:443 agent
agent gets:
  in, out, err              the terminal
  random                    the random service
  ~/proj                    read (current directory)
  net: api.example.org:443  reach (grant +net)
  /system/tls/roots         read (with the network)
~/proj> grants +net '*.example.org' +listen 8080 serve
serve gets:
  ...
  net: *.example.org        reach, any port (grant +net)
  net: port 8080            listen (grant +listen)
  /system/tls/roots         read (with the network)
```

`reach` on a name includes resolving it (item 4); `grants` does not print resolution as a line of its own (ADR-0224
item 3). The second grant covers `api.example.org` and `www.example.org`, not `example.org` (item 2).

**Tensions with accepted ADRs.**
- *ADR-0002 rule 5 and ADR-0107 item 4* list the kinds of a table entry; the network is not a path. Item 7 adds a
  fixed slot beside `BROKER` and `REPORT` rather than a prefix, so no path names the network. INV-0002-5's list of a
  POSIX process's handles gains `NET`; INV-0153-1's list of a command's handles gains it too.
- *ADR-0153 rule 1* ("nothing else") stands: `NET` is present only with a grant or a designating operand (ADR-0224).
- *ADR-0055*: the words `clone` and `narrow` are reused for a second service with the same meaning; nothing in
  ADR-0055 changes.
- *ADR-0005*: sockets are out of its scope; ADR-0225 proposes the rows.
- *ADR-0101*: an established connection's stream ends are handles a program may pass on (item 8), which ADR-0101's
  duplicable ends allow as written.

### Effect of the vision
This is the network half of target 2: an agent in an untrusted room holds a scope its supervisor wrote, every host it
may reach is a line the person can read, and nothing it computes can widen it (ADR-0153 rule 3's rule for grants
applies, ADR-0224). For target 3, a desktop program gets its scope from its description's operand (a URL) or from the
chooser's later sibling, and the compositor shows it (ADR-0172). Threads: a connection is a channel, so ADR-0050's
per-message atomicity holds. Moving later from C to B costs nothing (a scope of one `Any` rule is B), from C to
anything finer (per-path HTTP rules) would be a proxy, not a capability.

## Consequences
- **Positive.** The network follows the files: a grant is a word on the line, visible in `grants`, in a room and on a
  window; an untrusted room can be given one API; resolution cannot leak past the grant.
- **Negative.** A filter to parse and a resolver in the service (ADR-0223). Wildcards and ranges are policy people must
  read. A grant by name trusts the name's DNS operator, and whoever controls the zone, for *which public hosts* the
  name means: `+net example.org` is "whatever example.org's DNS says", and ADR-0223's local-address rule only keeps
  that from pointing into the machine. A `Wildcard` grant lets the program resolve any name under the suffix, and
  each query carries up to 253 bytes to the zone's authoritative servers: a channel to the zone's operator that
  resolution alone opens (`network/survey.md#name-resolution-as-authority`), which is the cost of wildcards.
- **Follow-up.** `zoraya_abi::net::Scope` with its decoder and subset test; the `NET` slot in both records; ADR-0221
  to ADR-0225.

## Invariants
- **INV-0220-1.** A program whose launcher granted no network holds no channel to `netd`: its handle table has no
  `NET` slot and no connection to the network service. *Check:* INV-0153-1's and INV-0002-5's enumeration tests, run
  with no grant and with one.
- **INV-0220-2.** No frame leaves `netd` on behalf of a connection's request (`connect`, `send`, `listen`, `resolve`)
  for a destination outside that connection's scope, and no `resolve` is answered for a name outside it: both get
  `NotGranted`. `netd`'s own frames (ARP, DHCP, its queries to the configured DNS servers, later SNTP) are outside
  this rule and covered by INV-0223-1. *Check:* a host test of `netd`'s logic over a fake device that records every
  frame with the request that caused it, driving `connect`, `send` to a name and to an address, `listen` and `resolve`
  inside and outside each kind of rule, `Any` included.
- **INV-0220-3.** `narrow` never yields a scope or a right its caller lacked, `Scope::decode` is total, and a scope
  that encodes then decodes is equal (the decoder transforms nothing). *Check:* a host property test of the subset
  relation over random scopes (as INV-0055-3's), including `Any`, nested wildcards and nested prefixes, and
  `cargo test -p zoraya-abi` with truncations and mutations (as INV-0107-1's) and mixed-case names refused.
- **INV-0220-4.** A scope holds names or addresses as typed values: no rule's target is parsed from a free-form string
  after decoding, and a name never matches an address rule or the reverse. *Check:* a unit test over the matcher with
  names that look like addresses and addresses written as names.

## Open questions
- Whether one grant should cover both TCP and UDP, or UDP needs its own word (ADR-0224 keeps one).
- The bounds: 64 rules, 8 port ranges, 253-byte names, all `[assumption]`.
- Whether a `status`-only scope is worth a kind of its own, or `net status` (ADR-0229) should be a right on the
  shell's connection alone (ADR-0224 item 2 gives `net` one through a declaration).
- Internationalised names: the shell could convert to A-labels (punycode) so that the scope stays ASCII; not needed by
  any program in scope.

## Revisit triggers
- A program in scope that must reach hosts it learns at run time beyond a wildcard (a browser): the grant's shape
  moves toward a proxy policy, or the person grants `any`.
- A second network interface (two scopes per interface?) or IPv6 by default.
- ADR-0056 accepted: the scope gains budgets (connections, buffers), as ADR-0056's accounts.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request. The capability names what Option C says: names, `*.` wildcards, addresses, prefixes and `any`, with optional ports. The architect's second fix: `*.example.org` does not cover `example.org` itself, as in TLS wildcards; item 2 and the example say so.
