# Network survey for TASK-23

The research behind proposed ADR-0220 to ADR-0230 (networking). Every source in [Sources](#sources) was fetched on
2026-11-04 from the architect's Mac, and the note there says what was read. Text is paraphrased; short quotations are
marked. Where a page could not be fetched, the claim rests on a catalog entry (`DM-`, `GR-`, `CP-` in
`../design-mistakes.md`, `../held-up-well.md` and `../candidate-principles.md`) or is left out. Pages this session
could not read: Arm's register reference for `RNDR` (rendered by script, no text; replaced by the Linux kernel's
hwcaps document [N76]), the QEMU wiki (a bot wall; replaced by the QEMU manual [N65][N66]), the savannah project
page of lwIP (connection refused; its README was read from the project's GitHub mirror [N39], since the savannah `cgit` copy returned 404), one QEMU mailing-list archive (403; replaced by the
patch's cover letter on patchew [N72]), the Codex security page (a redirect; the sandboxing page was read instead
[N27]), the Mbed TLS porting article (404; the 3.6 configuration header was read instead [N54]), and Fuchsia's
`netstack3.cml` (404; the test shard [N13] and the FIDL reference [N12] were read instead). The Capsicum paper [N7]
is a PDF whose text was extracted locally from its compressed streams; the quotations come from that extraction, and
its word spacing is lost there. Nothing below rests on a page that was not fetched, except one sentence marked
`[outside evidence]` in [Entropy](#entropy).

Numbers measured in this session (the `cargo check` runs, `sysctl`, the device-tree fixtures, the call-site counts)
are marked *measured* and listed in [Checks run in this session](#checks-run-in-this-session); everything else is
what the source says.

What the research catalog already holds, and this survey does not repeat: the BSD sockets API as a design that was
right at the time (DM-074), name resolution in libc as a tradeoff (DM-075), privileged ports (DM-076), `SIGPIPE` on
sockets (DM-077), the in-kernel versus user-space stack question as *contested* (DM-078), and CP-28, the candidate
principle that network access and endpoints are capabilities and that name resolution is an asynchronous service.
Capsicum's tcpdump losing DNS in capability mode is GR-008's example.

## How systems grant network access

### The Unix baseline

On Unix any process may call `socket` and `connect` to any address: the network namespace is global and the check is
the user's identity, which is DM-009's ambient authority in its purest form. The one restriction, binding a port below
1024, is tied to the port number and to root, not to a program (DM-076). Everything below is a retrofit on this
baseline or a system that never had it.

### OpenBSD pledge and unveil

`pledge(2)` [N1] lets a program declare which of about three dozen subsystems it will use; what is not listed becomes
unavailable, and later calls can only reduce the set. The network promises: `inet` allows `socket`, `listen`, `bind`,
`connect`, `accept`, `getpeername`, `getsockname` and a reduced `setsockopt`/`getsockopt` in the `AF_INET` and
`AF_INET6` domains; `unix` the same for `AF_UNIX`; `mcast` restores multicast options; `route` and `wroute` read and
change the routing table; `dns` allows reading `/etc/hosts`, `/etc/resolv.conf`, `/etc/protocols` and `/etc/services`
and the socket calls, but only on sockets of the special type `SOCK_DNS`, which the resolver uses on port 53 alone.
A violation kills the process with an uncatchable `SIGABRT`, or fails with `ENOSYS` under the `error` promise. So
pledge names a *subsystem*, never a host or a port: a program with `inet` reaches the whole Internet. Its `dns` promise
is the one place where the network is scoped by purpose, and it is scoped by port and socket type, not by name.

`unveil(2)` [N2] is the file half: a path and a permission string of `r`, `w`, `x`, `c`, locked by a final call; a
directory rule covers everything beneath it; paths not unveiled give `ENOENT`, access without the permission
`EACCES`. The page warns that "programs misbehave badly when their files unexpectedly disappear" and calls hiding
arbitrary parts of the filesystem from arbitrary programs "a dangerous concept". unveil has no network form.

### Capsicum and Casper

Capsicum's capability mode (`cap_enter(2)` [N6]) removes access to global namespaces: "Access to global name spaces,
such as file system or IPC name spaces, is prevented." The `capsicum(4)` page [N3] says that "Capsicum makes file
descriptors into capabilities", narrowed with `cap_rights_limit(2)`. The paper [N7] says system calls "requiring global
namespace access are unavailable, while others are constrained", and that "capability-related failures return a new
errno value, ENOTCAPABLE". Its Figure 9 is the DNS lesson: a `ktrace` of the sandboxed tcpdump shows
`open("/etc/resolv.conf")` failing, then `socket(PF_INET, SOCK_DGRAM, IPPROTO_UDP)` succeeding, then `connect` failing,
with the caption that "DNS resolution depends on filesystem and TCP/IP namespaces after cap_enter". So in capability mode a
socket can be *created* but not *connected* to an address, and the resolver, which needs both a file and an address,
is the first thing to break (GR-008).

Casper answers this with services a sandboxed program reaches over a channel opened *before* `cap_enter`.
`cap_net(3)` [N4] (service `system.net`) provides `cap_bind`, `cap_connect`, `cap_getaddrinfo`, `cap_gethostbyname`,
`cap_getnameinfo` and `cap_gethostbyaddr`. By default the channel gives the whole network; the program narrows it
with `cap_net_limit_init(channel, mode)` and `cap_net_limit()`. The modes: `CAPNET_NAME2ADDR` (resolve names),
`CAPNET_ADDR2NAME`, `CAPNET_BIND`, `CAPNET_CONNECT`, and `CAPNET_CONNECTDNS`, under which `connect` is allowed to the
addresses an earlier `cap_getaddrinfo` or `cap_gethostbyname` returned. The limit functions name what is allowed:
`cap_net_limit_name2addr(limit, name, service)` the host names (and service, which is the port) that may be resolved,
`cap_net_limit_connect(limit, sockaddr)` and `cap_net_limit_bind(limit, sockaddr)` the addresses that may be reached
or bound, and the `_family` variants the address families. Once applied, limits only narrow: "subsequent calls to
cap_net_limit() will fail unless the new set is a subset of the current set". `cap_dns(3)` [N5] is the older,
resolution-only service (modes `NAME2ADDR`, `ADDR2NAME`, a family limit), marked obsolete in favour of `cap_net`.

Casper is the closest existing thing to a network *capability*: a channel whose rights are a set of names, addresses
and ports, enforced by a service outside the sandbox, narrowable and never widenable, with resolution and connection
bound together (`CONNECTDNS`). Its weaknesses: the limits are set by the program itself, not by whoever launched it
(the trust is in the program's own first lines of code), and the service is a daemon the program must find.

### Plan 9's `/net` and the connection server

Plan 9 presents each protocol as a directory, `/net/tcp`, with a `clone` file and numbered connection directories
holding `ctl`, `data`, `listen`, `local`, `remote` and `status` [N8]. To connect, a program opens `clone`, reads the
connection number, writes an address to `ctl` and opens `data`. The `dial` library hides this; its address format is
`network!netaddr!service` [N9], for example `net!helix!9fs` or `tcp!135.104.117.5!513` [N8]; `net` means any network
shared with the destination. Name resolution is done by a user-level process, the connection server: "CS is a file
server serving a single file, /net/cs"; "A client writes a symbolic name to /net/cs then reads one line for each
matching destination" [N8]. `ndb/cs` "is a server used by dial to translate network names" and "finds out what
networks are configured by looking for /net/*/clone when it starts" [N10]; `ndb/dns` "serves ndb/cs and remote
systems by translating Internet domain names" and "By default dns serves only requests written to /net/dns" [N10].
Because `/net` is a directory in a per-process namespace (GR-004), a process without `/net` has no network, and
`import -a helix /net` gives a machine without an Internet connection the networks of one that has them [N8].

What Plan 9 grants is therefore *a whole network* (a mounted `/net`) or nothing, with resolution as a separate file
server. It never scoped by host or port; the authors' own limits list says network addressing and authentication did
not map cleanly onto files (GR-004, `alternatives-postmortem.md#plan-9--inferno`). Its two ideas that carry over are the
textual dial string and the resolver as a service a program talks to, not code linked into it (DM-075).

### Fuchsia's socket provider

On Fuchsia a component reaches a protocol only if its parent routes it: "Components route protocol capabilities by
exposing them to their parent and offering them to their children", and a `use` "populates the protocol in the
component's namespace at the well-known path" `/svc/<name>` [N15]. The network stack exposes
`fuchsia.posix.socket.Provider`, whose reference says "Provider implements the POSIX sockets API", with
`StreamSocket(domain, proto)`, `DatagramSocket(domain, proto)`, `GetInterfaceAddresses`, `InterfaceIndexToName`,
`InterfaceNameToIndex` and `InterfaceNameToFlags`, and its socket protocols' `Describe` answers carry a Zircon socket
handle, for datagrams one "on which data is sent and received", beside the FIDL channel that carries the calls [N12]; a test topology shard offers exactly
`fuchsia.posix.socket.Provider` and `fuchsia.net.routes.State` from the netstack child to the DNS resolver child [N13].
Name resolution is a separate component with its own protocols: `fuchsia.net.name.Lookup` ("Provides name and address
resolution", `LookupIp`, `LookupHostname`) and `fuchsia.net.name.LookupAdmin` (`GetDnsServers`, `SetDnsServers`)
[N14]. RFC-0184 [N11] requires "any system netstack to target a POSIX-like API"; POSIX components "SHOULD link in the
`fdio` compatibility library" and be "routed the appropriate socket creation capabilities"; direct FIDL use is
"actively discouraged"; divergences are accepted where Fuchsia has no UIDs (`SO_REUSEPORT`'s same-UID rule) and
where the behaviours behind Linux's `CAP_NET_RAW` and `CAP_NET_BIND_SERVICE` are to be supported subject to the
feasibility of mapping them to Fuchsia concepts. Fuchsia
replaced its Go netstack with a Rust one, netstack3, because Go was "not an approved language" and the old stack was
owned by the gVisor team with different goals; the roadmap calls the netstack "extremely load bearing", since every workflow depends on connectivity, names
interoperability, performance ("We still can't estimate N3 performance to validate we're at parity") and POSIX
compatibility as the risks, and mitigates the first with industry conformance tools (IxANVL, INTACT) and tests against
the existing stack and Linux [N16].

So Fuchsia's grant is a *protocol handle*: holding the socket provider is holding the whole network, resolution is a
second handle, and policy finer than that (hosts, ports) is not in the capability. The split between stack and
resolver as two routed capabilities is the part that fits Zoraya's parent-routed handles (ADR-0053).

### Genode's NIC router

Genode's NIC router [N17] sits between NIC sessions (clients) and uplink sessions (drivers) and routes at the network
and transport layers. Each session is an interface assigned to a *domain* by a `<policy>` matched on the session's
label; a new interface, though its session exists, "remain[s] dead ends towards the router" until a policy assigns it
a domain. A domain has an IPv4 identity (static or
by DHCP client), and rules as sub-tags: `<tcp dst="...">` and `<udp dst="...">` with `<permit port="..."/>` or
`<permit-any/>`, `<icmp dst="...">`, `<ip dst="...">` for plain forwarding, `<tcp-forward>` and `<udp-forward>` for
inbound ports, `<nat domain="..." tcp-ports="..." udp-ports="..." icmp-ids="..."/>` to bound how many source ports a
client domain may use, and a `<dhcp-server>` that can hand out DNS settings. The example line is
`<domain name="home_lan" interface="10.0.2.55/24" gateway="10.0.2.1" />`. The router is itself a component in the
parent-routed tree (`alternatives-postmortem.md#genode`), so the grant is: *which domain your session lands in*, and
the domain's rules say which destination prefixes and ports it may reach. This is the one surveyed system where the
network grant is a filter written by the launcher, not by the program, and where each client runs its own TCP/IP
stack (Genode components use their own stacks over a NIC session) behind the router. The filter names IP prefixes and
ports, never host names: Genode has no resolver in the router.

### WASI sockets

WASI sockets [N18] ("Unlike BSD sockets, WASI sockets require capability handles to create sockets and perform domain
name lookups") is in Phase 3 and merged into the WASI repository. The `network` resource is "An opaque resource that
represents access to (a subset of) the network", which "enables context-based security for networking" [N20]; a
component gets one from `instance-network: func() -> network` ("Get a handle to the default network") [N19]; `connect`
must use the same `network` as `bind`, and the design says "This enables interactive WASI hosts to inject permission
prompts" [N21]. The error codes include `access-denied` and `permanent-resolver-failure` [N20]. Name lookup is a
separate interface, `wasi-ip-name-lookup`, which "focuses strictly on translating internet domain names to ip
addresses and nothing else", deliberately without `getaddrinfo`'s service-name mapping [N18]. The README is explicit
that "This proposal does not specify how wasm runtimes should handle network permissions" and that "WASI
implementations should deny all network access by default" [N18]. So the shape is a handle that *may* be a subset of
the network, with the subset's definition left to the runtime.

### One bit for the whole network: Android, macOS, Flatpak

Android's `INTERNET` permission "Allows applications to open network sockets" and has protection level `normal`
[N22]; normal permissions are install-time permissions that "the system automatically grants" when the app is
installed, with no runtime prompt [N23]. macOS's App Sandbox has `com.apple.security.network.client`, "A Boolean value
indicating whether your app may open outgoing network connections", and `com.apple.security.network.server` for
listening; the entitlements "restrict only the initiation of a network connection, not the flow of data" for TCP, and
both initiation and flow for UDP [N24]; nothing in them names a host. Flatpak's `--share=network` is "Access the
network", and the default sandbox has "No access to the network"; the documentation adds that network access "also
grants access to all host services listening on abstract Unix sockets" [N25]. All three are one bit: an application
either has the Internet or does not. Why it is too coarse: every application with a network reason (an update
check) can reach every host, so the permission tells the person nothing about *where* data goes, and the interesting
distinction for an untrusted program, "this host and no other", cannot be expressed at all.

### Container and agent sandboxes

Docker's `none` network driver exists to "completely isolate the networking stack of a container": "Within the
container, only the loopback device is created" [N29]. gVisor runs its own user-space TCP/IP stack, netstack, inside the sandbox and writes
link-layer frames to the container's virtual device; `--network=none` keeps only a loopback, and `--network=host`
hands the sandbox the host's stack at the cost of isolation [N28]. Both are per container, all or nothing.

Agent sandboxes, which have to let an agent fetch some things and not others, have converged on an allow-list of host
names enforced by a proxy outside the sandbox. Claude Code's sandbox [N26] gives commands "no direct route to the
network": on Linux a separate network namespace, on macOS a Seatbelt profile that blocks everything but the proxy; the
proxy checks each connection's host name against `network.allowedDomains` (entries such as `"github.com"`,
`"*.npmjs.org"`, `"myapp.test:3000"` and `"[::1]:443"`, where an entry without a port allows every port),
`network.deniedDomains` and `network.strictAllowlist`; it "enforces the allowlist based on the requested hostname and,
by default, does not terminate or inspect TLS traffic" (an experimental `network.tlsTerminate` makes it terminate TLS);
it resolves names itself, so inside the sandbox `curl --noproxy '*'` fails with "Could not resolve host"; it refuses an
allowed name that resolves to a loopback, link-local or own address unless the address is listed, while "The names
`localhost` and `*.localhost` are allowed to resolve to loopback" and "An allowed intranet hostname that resolves to a
private range such as `10.0.0.0/8` connects"; UDP, QUIC and ICMP cannot leave; a refused proxy-aware tool gets a `403`, others "Operation not
permitted" or "Network is unreachable"; and the denied host is named in the command's result so the agent can ask.
Codex's sandbox [N27] decides "whether commands can use the network"; `danger-full-access` "removes the filesystem and
network boundaries"; in `on-request` mode the agent "asks when it needs to go beyond that boundary"; the hosted
variant can restrict commands to "required hostnames from a managed allowlist". Deno, the runtime, refuses network
access unless `--allow-net` is given, scoped as `--allow-net=example.com`, prompts `Allow? [y/n/A]` on a terminal, and
throws `NotCapable` otherwise [N30].

The agent sandboxes are the best evidence of what a *useful* network grant for untrusted code looks like in practice:
a list of host names with optional ports, resolution done by the enforcing side, a refusal that names the host, and a
way for the person to allow it afterwards. They also show the costs of doing it with a proxy: only proxy-aware
protocols work, UDP and ICMP do not, and `ssh` and database drivers fail.

### What a grant names

| System | Unit of grant | Names a host? | Names a port? | Resolution | Who sets it |
|---|---|---|---|---|---|
| Unix | the user | no (everything) | below 1024 by root | libc in the process | nobody |
| pledge `inet`/`dns` [N1] | a subsystem | no | `dns`: port 53 | libc, under `dns` | the program |
| Capsicum + `cap_net` [N4] | a Casper channel with limits | yes, names and addresses | yes | the service, bound to connect by `CONNECTDNS` | the program, before `cap_enter` |
| Plan 9 `/net` [N8] | a mounted network | no | no | `/net/cs`, a server | the namespace builder |
| Fuchsia [N12][N14] | a routed protocol | no | no | a second protocol | the parent component |
| Genode router [N17] | a domain with rules | prefixes only | yes | none in the router | the launcher's configuration |
| WASI [N20] | a `network` handle, "a subset" | left to the runtime | left to the runtime | a second interface | the runtime |
| Android, macOS, Flatpak [N22][N24][N25] | one bit | no | no | in the process | install time or manifest |
| Claude Code, Deno [N26][N30] | an allow-list | yes, with wildcards | yes | the proxy or the runtime | the person or the settings |

## Name resolution as authority

DNS is both a way to reach a name and a channel in itself. MITRE's T1071.004 [N31] records that "Adversaries may
communicate using the Domain Name System (DNS) application layer protocol" for command and control, that DNS traffic
"may also be allowed even before network authentication is completed", and that data can be carried "by adding it to
the subdomain portion of a DNS request" (its NightClub example); its mitigation is to filter requests to unknown,
untrusted or known bad domains and to resolve through a proxy. Capsicum's tcpdump [N7] shows the other side: a program that may *not* resolve cannot
reach anything by name. So a system that wants to scope the network must scope resolution too, or the grant "connect
to example.org" is also "send up to 253 bytes to anyone, encoded as a name".

How the surveyed systems scope it: pledge's `dns` promise allows UDP to port 53 only, to any server, so any name may be
asked [N1]. Casper's `cap_net_limit_name2addr` names which hosts may be resolved, and `CONNECTDNS` lets a program
connect only to what the limited resolver returned [N4]. Plan 9 resolves in `/net/cs` and `/net/dns`, servers a
process reaches only if its namespace has them [N10]. Fuchsia routes `fuchsia.net.name.Lookup` as a capability of its
own, and a component without it cannot resolve [N14]. Claude Code's proxy resolves names itself and refuses a name
that resolves to a local address, so the sandbox never speaks DNS [N26]. WASI's lookup is a separate interface that
takes the `network` handle [N18].

What a resolver needs on the wire, from musl's source [N82]: `/etc/resolv.conf` is read for the servers, `/etc/hosts`
for local names, the query goes out on a `SOCK_DGRAM` socket bound to an ephemeral port with `sendto` to each server
and `poll` for the answers, and a truncated answer falls back to a `SOCK_STREAM` connection to the same server
(`res_msend.c`); the whole of it runs in the calling thread (DM-075).

## TCP/IP stacks usable here

### smoltcp

smoltcp [N32] is "a standalone, event-driven TCP/IP stack" for "bare-metal, real-time systems", under the 0-clause BSD
licence (`0BSD` in `Cargo.toml` [N34]); version 0.14.0 of 2026-08-07, with a minimum Rust of 1.91 [N33][N34], below
the repository's 1.99.1. Its design goals are "simplicity and robustness", and it "does not need heap allocation *at
all*": sockets own buffers the caller gives them, and `std` and `alloc` are optional features [N32][N34]. Mandatory
dependencies: `managed`, `heapless`, `byteorder`, `bitflags`, `cfg-if`; optional: `log`, `defmt`, `libc` (for the host
phy backends) [N34]. Media: Ethernet with ARP, raw IP, IEEE 802.15.4; 802.1Q and jumbo frames "**not** supported"
[N32]. IPv4 and IPv6 (source address selection by RFC 6724 since 0.11.0, SLAAC behind a feature since 0.13.0) [N33]. TCP: window
scaling and MSS negotiated, Nagle, keep-alive, retransmission from an RTT estimate doubling on each retry, with the backoff
aligned to RFC 6298 in 0.13.0 ("Fix retransmit exponential backoff, align to RFC 6298"), zero-window probing, out-of-order reassembly with a bounded number of gaps,
delayed acknowledgements, a fixed 10 s time-wait, congestion control optional with "`CUBIC` and `Reno`" (first in
0.12.0, made RFC-compliant in 0.14.0); not implemented: selective acknowledgements, timestamps, silly window
syndrome avoidance, path MTU discovery; the urgent pointer is ignored [N32][N33]. UDP, ICMP, raw sockets. A DHCPv4
client socket that "acquires an IP address configuration through DHCP autonomously", with lease capping, retries,
rebinding since 0.10.0, and the parameter request list asking for subnet, router and DNS servers [N33][N37]. A DNS
client socket since 0.9.0: UDP only, `A`, `Aaaa`, `Cname`, `Ns`, `Soa` query types [N36], a server list truncated to
`MAX_SERVER_COUNT` (a build-time feature, default 1), results bounded by `DNS_MAX_RESULT_COUNT` (default 1), "no
support for DNS search path" [N35][N34]; its page describes a UDP socket and mentions neither a TCP fallback nor a
cache, so their absence is inferred from the page, not stated by it. The device interface is the
`phy::Device` trait with `receive` and `transmit` returning tokens that are consumed when the frame is read or
written, so an echo needs no allocation [N38]. Buffer sizes and table counts are compile-time features
(`iface-max-addr-count-*`, `assembler-max-segment-count-*`, `reassembly-buffer-size-*`, `dns-max-server-count-*`)
[N34]. Correctness fixes in 2026 include a SACK sequence overflow panic and a SYN from the unspecified address
(0.13.1) [N33], which says the stack is used and maintained, and that its edges were still being found this year.

*Measured* (details in [Checks run in this session](#checks-run-in-this-session)): smoltcp 0.14.0 with
`default-features = false` and the features `medium-ethernet`, `proto-ipv4`, `proto-dhcpv4`, `proto-dns`,
`socket-tcp`, `socket-udp`, `socket-dhcpv4`, `socket-dns` and `socket-tcp-reno` passes `cargo check` for
`aarch64-unknown-none` on the pinned toolchain, pulling eight crates. With `socket-tcp-cubic` instead of Reno it does
not: CUBIC uses `f64::powi` and `f64::mul_add`, which `core` does not provide, so CUBIC needs `std` or a math crate.
Reno is the congestion control available without `std`.

### lwIP

lwIP [N39] is "freely available under a BSD license", in C, "suitable for use in embedded systems with tens of
kilobytes of free RAM" and "around 40 kilobytes of code ROM". Protocols: IPv4 and IPv6 with forwarding, ICMP, IGMP,
UDP, TCP "with congestion control, RTT estimation fast recovery/fast retransmit and sending SACKs" (which smoltcp
does not send), DNS, DHCP, AutoIP, PPP, and among its applications an SNMPv2c agent; APIs: a "raw/native API for enhanced performance" and an "Optional Berkeley-like socket API". It is the
mature C choice, with a long record in embedded systems, and it would come into Zoraya the way musl did (ADR-0100): a
C port with a shim for its platform layer, compiled with the cross toolchain of ADR-0109. Against it: C in a service
that parses hostile packets (DM-048 and CP-15 for the native services), and a second foreign code base to carry.

### Other stacks

- **embassy-net** [N40] describes itself as "a no-std no-alloc async network stack, designed for embedded systems"
  with async TCP, UDP, DNS, DHCPv4 and IPv6; its README now names a stack called `xarxa` underneath, and crates.io
  shows `xarxa` as a reserved name with no published code (`0.0.0`, `0BSD`, no repository) [N41], so the stack it
  wraps today is not something this survey could read. It is an async wrapper, which Zoraya's single-threaded port
  loop (ADR-0006) does not need.
- **Fuchsia's netstack3** [N16] is Rust and the most complete Rust stack in a shipping system, but it is written
  against FIDL and Fuchsia's component model, not as a library; adopting it would mean adopting that model.
- **gVisor's netstack** [N28] is Go, which rules it out for a `no_std` service.
- The Linux, BSD and Windows stacks are in-kernel C (DM-078) and not candidates for a user-space Rust service.

### One stack as a service, or a stack per process

Two placements fit ADR-0054's user-space drivers. **One service** (Fuchsia's netstack [N12], Google's Snap and the
user-space stacks of DM-078, gVisor's netstack inside its sentry [N28]): one process owns the NIC through the driver
and all TCP/IP state; clients hold connections to it; it is where a grant can be enforced because every packet passes
through it; its costs are one more process boundary per transfer and one component whose failure takes every
connection with it (DM-078's "a user-space service adds a hop"). **A stack per process behind a multiplexer** (Genode's
NIC router [N17], and the per-application stacks of kernel bypass in DM-078): each client runs its own smoltcp over a
NIC session, and a router demultiplexes frames by address and port, applies the domain's rules and does NAT; a hostile
client corrupts only its own TCP state, and a slow client blocks nobody else; its costs are a stack's code and buffers
in every program, a router that must still parse every frame to route it (so the hostile-input surface is not
smaller, only moved), an address or a NAT port range per client, and name resolution that the router cannot do
because it does not see names. Genode's router shows the shape works, and that its filter can only speak in prefixes
and ports.

## TLS

### What a fetch over HTTPS needs

A client fetching `https://example.org/` needs: a TCP connection; a TLS 1.2 or 1.3 client with at least one AEAD
cipher suite and a key exchange; a certificate chain validator with a set of trust anchors; the server's name to match
against the certificate; **the current date**, because validation checks the certificate's validity period
([N46] rustls: the time is used "to, for example, check if a certificate has expired during certificate validation";
[N54] Mbed TLS: "This is used to verify the validity period of X.509 certificates"; [N51] BearSSL: "the date and time
must be provided externally with a `br_x509_minimal_set_time()` call"); and **entropy**, for the client random, the
ephemeral key exchange and, in TLS 1.3, the key shares. Without a clock that is right to within the validity period,
every certificate check is either skipped (BearSSL and embedded-tls offer that explicitly: `InsecureIgnoreDates`
[N47]) or fails. Zoraya's time base today is seconds since the volume's first use (ADR-0191), so it has neither.

### rustls

rustls [N42][N44] is a TLS 1.2 and 1.3 client and server in Rust, "Apache-2.0 OR ISC OR MIT", latest stable 0.23.46
(2026-10-14), with a development line 0.24 in which the crypto providers have moved out of the crate: "From 0.24, users
must explicitly provide a crypto provider when constructing `ClientConfig` or `ServerConfig` instances", with `rustls-aws-lc-rs` recommended
("a complete feature set", "can be harder to build on some platforms") and `rustls-ring` ("easier to build on a
variety of platforms", "a more limited feature set") [N42]; the 0.24 manifest lists `subtle`, `pki-types` and
`zeroize` as its only required dependencies and `webpki` as optional, with `rust-version = "1.85"` [N43]. In 0.23 the
`std` feature (on by default) gates "the high-level (buffered) Connection API and other functionality which relies on
the `std` library": `Connection`, `ClientConnection`, `ServerConnection`, `Stream`, `StreamOwned`, `Reader`, `Writer`
and `KeyLogFile` are `std`-only, the ticketer needs `hashbrown` or `std`, and `custom-provider` "disables implicit use
of built-in providers" [N45]. Time comes through the `TimeProvider` trait: `current_time` "Returns the current wall
time" as `Option<UnixTime>`, "Return `None` if unable to retrieve the time", and `DefaultTimeProvider` exists only with
`std` [N46]. The documentation pages this session read do not spell out the `no_std` contract in one place; the
measurements below do.

*Measured*: `rustls 0.23.46` with `default-features = false` and the features `custom-provider` and `tls12` passes
`cargo check` for `aarch64-unknown-none` on the pinned toolchain, with `rustls-webpki 0.103.16`, `rustls-pki-types
1.15.1`, `untrusted`, `subtle`, `zeroize` and `once_cell` (so the library itself, with `alloc`, builds without `std`).
With the `ring` feature instead it does not: `getrandom 0.2.17` refuses the target ("target is not supported") and,
after a custom `getrandom` backend is registered, ring's build script fails compiling its C code because the target
has no libc headers (`assert.h` not found with the host's clang). ring's own documents say it "currently requires a C
(but not C++) toolchain", that cross-compiling usually needs "a sysroot for the target" with `TARGET_CC` set [N57],
that it builds the C with the `cc` crate [N56], and that its licence is "Apache-2.0 AND ISC" [N56]. `getrandom`
documents the way out for a target with no operating system: "The `register_custom_getrandom!` macro allows a user to
mark their own function as the backing implementation", which "only has an effect on targets that would otherwise not
compile" [N58]. So rustls on Zoraya needs a provider that builds for `aarch64-unknown-none`: the two candidates found
are **graviola** [N61][N62] ("a compendium of high quality, fast and easy to build cryptography for Rust", aimed at
rustls, `aarch64` and `x86_64` only, needing `aes`, `sha2`, `pmull` and `neon` on aarch64, built from s2n-bignum's
formally proven assembler routines with "no C compiler, assembler or other tooling needed: just the Rust compiler",
"Apache-2.0 OR ISC OR MIT-0", version 0.4.3 of 2026-11-03, described by its author as "very new"; its page says its
lower layers use "minimal std and alloc", which points toward `std`, not away from it) and **rustls-rustcrypto**
[N59][N60] (pure Rust over RustCrypto, "MIT OR Apache-2.0", version `0.0.2-alpha` of 2024, "USE THIS AT YOUR OWN RISK!
DO NOT USE THIS IN PRODUCTION", and needing `std` today with "foundational support for future `no_std` expansion").
Neither was checked for `aarch64-unknown-none` in this session.

### embedded-tls

embedded-tls [N47][N48] is "TLS 1.3 client with no_std support and no allocator", Apache-2.0, version 0.19.0
(2026-06-01), maintained under embassy-rs; TLS 1.3 only, one frame at a time with a caller-sized buffer; crypto through
`embassy-crypto` driver crates; certificate verification either through `webpki::CertVerifier` (rustls-webpki, one
trusted root, "works without `std`") or the crate's own `pki::CertVerifier`, which "accepts only a server certificate the
trusted root signed directly"; a `TlsClock` for dates, with `InsecureIgnoreDates` for devices without a clock; and the
warning that "Some features and extensions are not yet implemented". *Measured*: 0.19.0 with `webpki` fails `cargo
check` for `aarch64-unknown-none` at the same two points as rustls with ring (`getrandom` unsupported target, then
ring's C build), because its webpki verifier pulls in ring.

### BearSSL

BearSSL [N49][N52] is C under the MIT licence, version 0.6, "beta-quality software" whose "successive 0.x versions
will be incompatible with each other"; it runs "on bare metal without `malloc()`" with a streamed design; "BearSSL
implements TLS 1.0, 1.1 and 1.2" [N85] (TLS 1.3 "is not implemented yet in BearSSL", with a roadmap [N50]). Its minimal X.509 engine validates "only the chain as received;
there is no path building", matches the server name against `dNSName` entries, and takes the current time as a day
count in the proleptic Gregorian calendar plus seconds [N51]. It is the natural TLS for *C* programs on Zoraya: curl
has a BearSSL backend, and the library's no-allocation, I/O-free state machine fits a `libposix` process. Its gaps
are TLS 1.3 and the pre-1.0 status.

### Mbed TLS

Mbed TLS [N53] is C under "Apache-2.0 OR GPL-2.0-or-later" with TLS 1.2 and 1.3 (both enabled in the 3.6
configuration [N54]), portable C99, configured by `mbedtls_config.h`. Its platform needs are explicit [N54]:
`MBEDTLS_HAVE_TIME` ("The time does not need to be correct, only time differences are used"), `MBEDTLS_HAVE_TIME_DATE`
("The time needs to be correct ... This is used to verify the validity period of X.509 certificates"),
`MBEDTLS_NO_PLATFORM_ENTROPY` for a platform without `/dev/urandom`, `MBEDTLS_ENTROPY_HARDWARE_ALT` for "your own
implementation of a hardware entropy collector", `MBEDTLS_PLATFORM_MEMORY` and `MBEDTLS_PLATFORM_NO_STD_FUNCTIONS`
for platforms without the standard functions. It is the larger C choice with TLS 1.3; the dual licence is permissive
under the Apache option.

### Plan 9's TLS filters and key agent

Plan 9 put TLS outside the program: `tlsclient` "dials address, starts TLS, and then relays between the network
connection and standard input and output", and `tlssrv` is used "to establish an SSL or TLS connection before
launching cmd args", with the server's private key held by `factotum` rather than in the certificate file [N86].
`factotum` "is a user-level file system that acts as the authentication agent for a user": a program opens
`factotum/rpc` ("each open represents a new private channel to factotum"), sends a `start` naming the protocol and
role, and shuttles the handshake's messages through it, so that the agent performs the proof and secret attributes
(`!password`) are never read back ("For secret attributes, only the attribute name follow by a ? is returned") [N87].
The two shapes a Zoraya design can take from this: TLS as a filter process between a program and its connection, and
keys in an agent that signs on a program's behalf.

### The root store and its licence

A validator needs trust anchors. The usual source is Mozilla's root store, packaged for Rust as `webpki-roots`
("Mozilla's CA root certificates for use with webpki"), whose current licence is `CDLA-Permissive-2.0` [N63], a
permissive data licence that is not one of the five licences `CLAUDE.md` allows for crates. Shipping any root store is
a decision: the data is not code, and the alternative, no roots in the image and the person supplying a file, means
HTTPS does not work out of the box.

### A real clock

QEMU's `virt` board has an RTC [N64]; its source creates it with `sysbus_create_simple("pl031", ...)` at `0x09010000`,
size `0x1000`, interrupt 2, device-tree compatible `"arm,pl031\0arm,primecell"`, clocked by the 24 MHz `apb-pclk`
[N69]. *Measured*: the repository's own device-tree fixtures from QEMU contain the node `pl031@9010000` with
`arm,pl031`. The PL031's registers, from Linux's driver [N78]: `RTC_DR` at `0x00` (data, read), `RTC_MR` `0x04`
(match), `RTC_LR` `0x08` (load, write to set), `RTC_CR` `0x0c` (control, bit 0 enables the counter), `RTC_IMSC`
`0x10`, `RTC_RIS` `0x14`, `RTC_MIS` `0x18`, `RTC_ICR` `0x1c`; `read_time` is one 32-bit read of `RTC_DR` converted
with `rtc_time64_to_tm`, so the register counts seconds since the Unix epoch. QEMU drives it from the host: "By
default the RTC is driven by the host system time", "The default base is UTC", with `-rtc clock=rt` to isolate the
guest and `clock=vm` for determinism under icount [N66]. So one aligned 32-bit read at boot gives the real date to the
second, on every QEMU host, with no network. Later, SNTP (RFC 4330 [N79]) is one UDP packet to port 123 whose reply's
transmit timestamp gives the time; the RFC says "A client MUST NOT under any conditions use a poll interval less than
15 seconds", and that "accuracies of up to a minute may suffice" for simple cases. A real clock also raises ADR-0191's
rule: the root volume's stored times may be above the RTC (a host clock that went back), so the floor "never below the
latest stored time" stays.

### Entropy

Four sources exist on this platform.

- **`FEAT_RNG`, the `RNDR` register.** Linux documents `HWCAP2_RNG` as "Functionality implied by ID_AA64ISAR0_EL1.RNDR
  == 0b0001" [N76]. QEMU's TCG emulates it: its list of emulated features includes "FEAT_RNG (Random number
  generator)", enabled by the `max` CPU ("The `max` CPU is an alias for `max-v9` under AArch64 TCG emulation") [N73];
  `tools/run-qemu.sh` uses `cortex-a72` under TCG, an Armv8.0 core, which does not have it `[outside evidence: Arm's
  Cortex-A72 documentation; FEAT_RNG is an Armv8.5 feature; not fetched]`. Under HVF QEMU passes
  the host's `ID_AA64ISAR0_EL1` through unmasked (`hvf_arm_get_host_cpu_features` reads `HV_FEATURE_REG_ID_AA64ISAR0_EL1`
  and `-cpu host` copies it; no code masks its fields) and has no handler for a trapped `RNDR` read [N68], so the
  guest sees exactly what the Apple core offers. *Measured*: on the architect's M4, `sysctl hw.optional.arm` lists 71
  keys and none for `FEAT_RNG`; Apple's documented list of `hw.optional.arm.FEAT_*` keys has 27 entries and none
  for `FEAT_RNG` either [N77]. Whether the core lacks the feature or macOS does not report it is not settled by these
  two facts; the way to settle it is for the kernel to print `ID_AA64ISAR0_EL1` at boot (the plan's first task).
  Until then `RNDR` cannot be counted on under either accelerator.
- **virtio-rng.** The virtio entropy device (device ID 4, one queue `requestq`, no feature bits) fills device-writable
  buffers with random bytes; the driver "must check the length the device wrote" and the device "must write at least
  one byte"; the specification calls it "high-quality randomness for guest use" without saying where it comes from
  [N75]. In QEMU the device is `virtio-rng` with a backend: `rng-random` "obtains entropy from a device on the host",
  `/dev/urandom` by default, and `rng-builtin` "obtains entropy from QEMU builtin functions", which is the device's
  default backend [N66]. On `virt` it is added as `-device virtio-rng-device`, which takes a virtio-mmio transport as
  `virtio-blk-device` does in `tools/run-qemu.sh` (ADR-0091's placement rules apply).
- **The device tree's `rng-seed`.** *Measured*: the repository's QEMU fixtures contain a `rng-seed` property, the seed
  QEMU writes into `/chosen` for the guest kernel. It is one boot-time seed, enough to start a deterministic random
  bit generator, not a continuing source.
- **Timing jitter** from the counter is a last resort no source here endorses.

Where other systems serve it: Zircon's `zx_cprng_draw` is a system call that will "Draw from the kernel's CPRNG.", with
no rights required and no failure but an invalid buffer [N88], so Fuchsia serves randomness from its kernel, not from
a component; Linux likewise (`getrandom`). No surveyed system serves it from a user-space service.

## The device: virtio-net under QEMU

The virtio network device [N74] has virtqueues `receiveq1` (0), `transmitq1` (1), then `receiveqN`, `transmitqN` and,
with `VIRTIO_NET_F_CTRL_VQ`, a `controlq`; `N` is 1 unless `VIRTIO_NET_F_MQ` (bit 22) or `RSS` (60) is negotiated, in
which case the configuration space's `max_virtqueue_pairs` says how many. Every packet is preceded by
`struct virtio_net_hdr`: `flags`, `gso_type`, `hdr_len`, `gso_size`, `csum_start`, `csum_offset`, `num_buffers`, 12
bytes, 20 with `HASH_REPORT`. The configuration space has `mac[6]` (valid with `VIRTIO_NET_F_MAC`, bit 5), `status`
(with `STATUS`, bit 16: `LINK_UP`, `ANNOUNCE`), `max_virtqueue_pairs` and `mtu` (with `MTU`, bit 3; "between 68 and
65535", "at least 1280"). The offload features a driver may ignore: `CSUM` (0) and `GUEST_CSUM` (1) for checksum
offload, `GUEST_TSO4/6`, `GUEST_ECN`, `GUEST_UFO`, `HOST_TSO4/6`, `HOST_ECN`, `HOST_UFO` (7 to 14) for segmentation,
`MRG_RXBUF` (15) for merged receive buffers, `CTRL_*` (17 to 23) for the control queue, and the newer `RSS`,
`HASH_REPORT`, `NOTF_COAL`, `GUEST_USO`. A driver that negotiates none of them sends fully checksummed packets with
`flags = 0` and `gso_type = VIRTIO_NET_HDR_GSO_NONE`, sets `num_buffers` to 0 on transmit, and, without `MRG_RXBUF`,
must post receive buffers of at least 1526 bytes (1534 with `HASH_REPORT`); the device then sets `num_buffers` to 1.
QEMU's device offers `csum`, `guest_csum`, `gso`, the TSO and UFO bits, `mrg_rxbuf`, `status`, `ctrl_vq`, `ctrl_rx`,
`ctrl_vlan`, `ctrl_mac_addr`, `ctrl_guest_offloads`, `guest_announce` and the USO bits by default and `mq`, `rss` and
`hash` off; the number of queue pairs follows the backend's queues (`n->max_ncs = MAX(n->nic_conf.peers.queues, 1)`)
[N70], and the user-mode backend has no `queues=` option (`tap`, `vhost-user` and `af-xdp` have one [N66][N67]), so under user-mode
networking there is one queue pair whatever the driver asks. The driver Zoraya needs is therefore the smallest one:
one receive and one transmit queue on ADR-0054's split-queue code (`zoraya-virtio` already has a generic split queue),
no offloads, 1526-byte receive buffers in a pinned memory object, the MAC from the configuration space, and the
`STATUS` bit for link state.

## QEMU networking for development and CI

QEMU's user-mode networking (SLIRP) is the default when no `-net` option is given and needs no privilege [N65]: the
guest gets `10.0.2.15` from a built-in DHCP server, the gateway is `10.0.2.2`, the virtual nameserver `10.0.2.3`, incoming connections are blocked except those forwarded with
`hostfwd=[tcp|udp|unix]:[hostaddr]:hostport-[guestaddr]:guestport`; general ICMP does not work (only pinging
`10.0.2.2`, and on Linux hosts with the `ping_group_range` set) [N65][N66]. The options that matter for a test
[N66][N67]: `restrict=on` ("the guest will be isolated, i.e. it will not be able to contact the host"; "Explicit
forwarding rules are not affected"), `guestfwd=[tcp]:server:port-dev` ("Forward guest TCP connections to the IP
address server on port port to the character device dev", or "a program executed by cmd:command which gets spawned for
each connection"), `net=`, `host=`, `dns=`, `dnssearch=`, `domainname=`, `ipv4=`/`ipv6=`, and `tftp=`. Packet capture:
`-object filter-dump,id=id,netdev=dev[,file=filename][,maxlen=len]` dumps "the network traffic on netdev dev to the
file", "At most len bytes (64k by default) per packet", in libpcap format "so it can be analyzed with tools such as
tcpdump or Wireshark" [N66]. `passt` is a faster unprivileged alternative to SLIRP with full IPv6 [N65]; TAP needs a
helper or root on Linux [N66]. On macOS the `vmnet-host`, `vmnet-shared` and `vmnet-bridged` backends exist, but their
cover letter says "vmnet requires either a special entitlement, granted via a provisioning profile, or root access",
and since QEMU is not signed with the entitlement the netdev "currently requires that qemu be run with root access"
[N72]; QEMU's code maps the framework's failure to "general failure (possibly not enough privileges)" [N71]. So vmnet
is not for `make test`. On GitHub's Ubuntu 24.04 runner, where CI runs `make test` under TCG, the `qemu-system-arm`
package (QEMU 8.2.2) depends on `libslirp0`, so user-mode networking is present there [N80].

A deterministic network test without the Internet follows from these: a `-netdev user` with `restrict=on`, so the
guest reaches nothing real; a `guestfwd` from a fixed guest-visible address and port to a server the test runner
itself listens on (a local TCP port or a Unix socket chardev), so the "remote host" is the runner; the guest's DNS
either answered by the runner through a second `guestfwd` or avoided by a local name in the guest's configuration;
and a `filter-dump` pcap kept with the run's logs. The 10.0.2.0/24 addresses, the DHCP offer and the one queue pair
are what such a test may assume.

## POSIX sockets: what curl and wget call

TASK-02 measured its surface with static import bounds and dynamic traces in a VM (`task-02/method.md`); nothing
network-related is in its measured tiers because no T0 to T2 workload used the network, and `socket`, `getaddrinfo`,
`poll` and the other socket calls are `T3` rows of `task-02/libc-surface.csv`, imported by the full busybox and
toybox binaries, clang and lld. This survey could not run that pipeline (the task forbids building and running
anything of the repository's, and the measurement VM is a second virtual machine on the same host), so the programs
Zoraya will want, curl and wget, were measured *statically from their sources*: a count of call sites of each socket
function in curl 8.17.0's `lib/` with its subdirectories (`vtls/`, `vquic/`, `vauth/`, `curlx/`: 180 C files) and
wget 1.25.0's `src/` (38 C files), by the regular expression `(^|[^A-Za-z0-9_])NAME[[:space:]]*\(` over every `.c`
file, which matches the function name followed by `(` with GNU-style spacing allowed [N83][N84]. It is an upper bound of a weaker
kind than TASK-02's static bound: it counts sites in every backend and platform branch (curl's Windows and vendor
TLS branches included), not what a `libposix` build would link.

| Function | curl `lib/` | wget `src/` | Function | curl | wget |
|---|---|---|---|---|---|
| `socket` | 15 | 2 | `getaddrinfo` | 12 | 1 |
| `connect` | 16 | 1 | `freeaddrinfo` | 3 | 1 |
| `bind` | 10 | 2 | `gethostbyname` | 10 | 1 |
| `listen` | 2 | 1 | `getnameinfo` | 0 | 0 |
| `accept` / `accept4` | 4 / 1 | 4 / 0 | `gai_strerror` | 0 | 1 |
| `send` / `sendto` / `sendmsg` | 12 / 10 / 4 | 0 / 0 / 0 | `inet_pton` / `inet_ntop` | 1 / 4 | 2 / 2 |
| `recv` / `recvfrom` / `recvmsg` | 7 / 3 / 3 | 2 / 0 / 0 | `if_nametoindex` | 1 | 0 |
| `setsockopt` / `getsockopt` | 26 / 3 | 3 / 0 | `poll` / `select` / `pselect` | 15 / 18 / 0 | 0 / 9 / 0 |
| `getsockname` / `getpeername` | 12 / 2 | 3 / 2 | `fcntl` / `ioctl` | 5 / 2 | 14 / 11 |
| `shutdown` | 2 | 0 | `socketpair` | 3 | 0 |

What it says: both programs are plain stream clients, `socket`, `connect`, `getaddrinfo`, `send`/`recv` or
`read`/`write`, with a readiness call (`select` in wget, `poll` or `select` in curl) and `O_NONBLOCK` through `fcntl`;
`bind`, `listen` and `accept` are FTP's active mode in both; `sendto`/`recvfrom` are curl's DNS-over-UDP and QUIC
paths; `socketpair` is curl's thread wake-up; neither uses `getnameinfo`. musl's `src/network` directory, which
ADR-0100 replaces with Rust, holds 76 files: the socket calls, `getaddrinfo` and its `lookup_*.c`, the `res_*`
resolver, `gethostby*`, `getservby*`, `inet_*`, `if_*`, `getifaddrs`, `netlink` and the byte-order functions [N81].
`git` over HTTPS uses libcurl and adds nothing below it; a C TLS library adds no socket calls of its own (BearSSL and
Mbed TLS leave I/O to the caller [N49][N53]).

## What this suggests for Zoraya

Not decisions; the ADRs weigh these.

1. The useful unit of a network grant, in every system built for untrusted code (Casper, the agent sandboxes, Deno),
   is a *set of host names with optional ports*, enforced outside the program, with resolution bound to it. Nothing
   coarser (a bit, a subsystem, a mounted `/net`) lets a person say "this host and no other", and nothing finer (an
   address) survives a host whose addresses change.
2. Resolution must be inside the grant, or DNS is an open channel out (MITRE, Capsicum's tcpdump).
3. One stack as a service is where a filter by name can live; a stack per process cannot filter by name.
4. smoltcp builds without `std` on the pinned toolchain with Reno; its DNS client is too small to be the system
   resolver (UDP only, one server, no cache), so the service needs its own resolver over smoltcp's UDP sockets.
5. TLS in Rust without `std` is possible (rustls checks), but its crypto provider is the open problem: ring needs a C
   toolchain with target headers, and the pure-Rust providers are new or alpha. For C programs BearSSL (TLS 1.2, MIT)
   or Mbed TLS (TLS 1.3, Apache) are ports like musl.
6. The PL031 gives a real date in one register read; entropy has no sure hardware source yet (RNDR unconfirmed on
   the architect's Mac, absent under `cortex-a72`), so virtio-rng, which QEMU offers on every host, is the source to
   build on, with the device tree's `rng-seed` as the boot seed.
7. The virtio-net driver can be the smallest possible: one queue pair, no offloads.
8. A test without the Internet is `restrict=on` plus `guestfwd` to the runner, with a pcap from `filter-dump`.

## Checks run in this session

All on the architect's M4 (macOS 26.4), 2026-11-04, with nothing of the repository built or run. The `cargo check`
runs used scratch crates in a directory outside the repository, `CARGO_BUILD_JOBS=2`, the toolchain `rust-toolchain.toml`
pins (1.99.1) and the installed `aarch64-unknown-none` target; each crate is a `#![no_std]` library with one
dependency.

| Check | Result |
|---|---|
| `smoltcp 0.14.0`, `default-features = false`, features `medium-ethernet proto-ipv4 proto-dhcpv4 proto-dns socket-tcp socket-udp socket-dhcpv4 socket-dns socket-tcp-reno`, `cargo check --target aarch64-unknown-none` | passes; 9 crates checked (`managed`, `heapless`, `byteorder`, `bitflags`, `cfg-if`, their dependencies, smoltcp, the scratch crate) |
| the same with `socket-tcp-cubic` instead of `socket-tcp-reno` | fails: `no method named powi found for type f64` and `mul_add` in `src/socket/tcp/congestion/cubic.rs` (3 errors) |
| `rustls 0.23.46`, `default-features = false`, features `custom-provider tls12` | passes; `rustls-webpki 0.103.16`, `rustls-pki-types 1.15.1`, `untrusted 0.9.0`, `subtle 2.6.1`, `zeroize 1.9.1`, `once_cell 1.21.4` |
| `rustls 0.23.46`, features `ring tls12` | fails: `getrandom 0.2.17` "target is not supported" |
| the same with `getrandom 0.2` (`custom` feature) added | fails: ring 0.17.14's build script, "fatal error: 'assert.h' file not found" compiling `crypto/curve25519/curve25519.c` with the host clang |
| `embedded-tls 0.19.0`, `default-features = false`, feature `webpki` | fails at the same two points (`getrandom`, then ring) |
| `sysctl hw.optional.arm` | 71 keys, none named `FEAT_RNG`; `machdep.cpu.brand_string` is `Apple M4` |
| `strings` over `crates/zoraya-dtb/tests/fixtures/virt-hvf-default.dtb` | `pl031@9010000`, `arm,pl031`, `arm,primecell`, `rng-seed`, 32 `virtio,mmio` nodes |
| call-site counts in curl 8.17.0 (SHA-256 `e8e74cde…b7910`, 180 files under `lib/`) and wget 1.25.0 (`766e4842…b784`, 38 files under `src/`) | the table above; the counts over `lib/*.c` alone (128 files) are lower for `socket`, `send`, `getsockname`, `sendmsg`, `recvmsg`, `recvfrom`, `poll` (13) and `select` (15), since the TLS and QUIC backends live in the subdirectories |

## Sources

Each was fetched on 2026-11-04 (this session) unless noted. "Primary" means the project's own documentation,
specification or source, or the authors' paper.

- [N1] OpenBSD, `pledge(2)`, https://man.openbsd.org/pledge.2, primary. The promises, `inet`, `dns`, `SOCK_DNS`, violation behaviour.
- [N2] OpenBSD, `unveil(2)`, https://man.openbsd.org/unveil.2, primary. Permissions, locking, `ENOENT`/`EACCES`, the warnings.
- [N3] FreeBSD, `capsicum(4)`, https://man.freebsd.org/cgi/man.cgi?query=capsicum&sektion=4&format=ascii, primary. Capability mode, rights on descriptors, Casper.
- [N4] FreeBSD, `cap_net(3)`, https://man.freebsd.org/cgi/man.cgi?query=cap_net&sektion=3&format=ascii, primary. Functions, modes, limits, narrowing.
- [N5] FreeBSD, `cap_dns(3)`, https://man.freebsd.org/cgi/man.cgi?query=cap_dns&sektion=3&format=ascii, primary. Functions and limits; obsolete in favour of `cap_net`.
- [N6] FreeBSD, `cap_enter(2)`, https://man.freebsd.org/cgi/man.cgi?query=cap_enter&sektion=2&format=ascii, primary. Global namespaces prevented.
- [N7] R. N. M. Watson, J. Anderson, B. Laurie, K. Kennaway, "Capsicum: practical capabilities for UNIX", USENIX Security 2010, https://www.cl.cam.ac.uk/research/security/capsicum/papers/2010usenix-security-capsicum-website.pdf (text extracted locally from the PDF's streams), primary. Capability mode's rules, `ENOTCAPABLE`, Figure 9 (tcpdump's DNS), libcapsicum.
- [N8] D. Presotto, P. Winterbottom, "The Organization of Networks in Plan 9", https://9p.io/sys/doc/net/net.html, primary (authors' paper). `/net`, `dial`, `cs`, `ndb`, `import`.
- [N9] Plan 9, `dial(2)`, https://9p.io/magic/man2html/2/dial, primary. The address format, `dial`, `announce`, `listen`, `accept`.
- [N10] Plan 9, `ndb(8)`, https://9p.io/magic/man2html/8/ndb, primary. `ndb/cs`, `ndb/dns`, `/net/cs`, `/net/dns`.
- [N11] Fuchsia, RFC-0184 "POSIX Compatibility for the System Netstack", https://fuchsia.dev/fuchsia-src/contribute/governance/rfcs/0184_posix_compatibility_for_the_system_netstack, primary. The decision, fdio, routed capabilities, accepted divergences.
- [N12] Fuchsia, FIDL reference `fuchsia.posix.socket`, https://fuchsia.dev/reference/fidl/fuchsia.posix.socket, primary, first 100,000 characters. The `Provider` protocol and its methods.
- [N13] Fuchsia, `isolated-network-offers.shard.cml`, https://fuchsia.googlesource.com/fuchsia/+/HEAD/src/connectivity/network/meta/isolated-network-offers.shard.cml, primary. Protocols offered from netstack to the DNS resolver.
- [N14] Fuchsia, FIDL reference `fuchsia.net.name`, https://fuchsia.dev/reference/fidl/fuchsia.net.name, primary. `Lookup`, `LookupAdmin`, `DnsServerWatcher`.
- [N15] Fuchsia, "Protocol capabilities", https://fuchsia.dev/fuchsia-src/concepts/components/v2/capabilities/protocol, primary. Declaring, routing, `/svc`.
- [N16] Fuchsia, roadmap "Netstack3 - A Fuchsia owned rust based netstack", https://fuchsia.dev/docs/contribute/roadmap/2021/netstack3, primary. Motivation, goals, risks.
- [N17] Genode, `nic_router` README, https://github.com/genodelabs/genode/blob/master/repos/os/src/server/nic_router/README, primary. Domains, policies, rules, NAT, DHCP, uplinks.
- [N18] WebAssembly, WASI sockets README, https://github.com/WebAssembly/wasi-sockets/blob/main/README.md, primary. Capability handles, deny by default, the lookup interface, status.
- [N19] WASI sockets, `wit/instance-network.wit`, https://raw.githubusercontent.com/WebAssembly/wasi-sockets/main/wit/instance-network.wit, primary.
- [N20] WASI sockets, `wit/network.wit`, https://raw.githubusercontent.com/WebAssembly/wasi-sockets/main/wit/network.wit, primary. The `network` resource, error codes.
- [N21] WASI sockets, `wit/tcp.wit`, https://raw.githubusercontent.com/WebAssembly/wasi-sockets/main/wit/tcp.wit, primary. `bind`, `connect`, permission prompts.
- [N22] Android, `Manifest.permission`, https://developer.android.com/reference/android/Manifest.permission, primary, the `INTERNET` entry (characters 150,000 to 250,000 of the page). Description and protection level.
- [N23] Android, "Permissions on Android", https://developer.android.com/guide/topics/permissions/overview, primary. Install-time and normal permissions.
- [N24] Apple, entitlement `com.apple.security.network.client` (data endpoint), https://developer.apple.com/tutorials/data/documentation/bundleresources/entitlements/com.apple.security.network.client.json, primary. Abstract, discussion, the TCP and UDP note, the server entitlement.
- [N25] Flatpak, "Sandbox Permissions", https://docs.flatpak.org/en/latest/sandbox-permissions.html, primary. `--share=network` and its footnote.
- [N26] Anthropic, Claude Code "Sandboxing", https://code.claude.com/docs/en/sandboxing, primary, first 100,000 characters. Network restrictions, allow-lists, the proxy, refusals.
- [N27] OpenAI, Codex "Sandboxing", https://learn.chatgpt.com/docs/sandboxing (redirected from developers.openai.com/codex/sandboxing), primary. Network boundary, approvals, platforms.
- [N28] gVisor, "Networking", https://gvisor.dev/docs/user_guide/networking/, primary. netstack, `network=none`, `network=host`.
- [N29] Docker, "None network driver", https://docs.docker.com/engine/network/drivers/none/, primary.
- [N30] Deno, "Security and permissions", https://docs.deno.com/runtime/fundamentals/security/, primary. `--allow-net`, prompts, `NotCapable`.
- [N31] MITRE ATT&CK, T1071.004 "Application Layer Protocol: DNS", https://attack.mitre.org/techniques/T1071/004/, secondary (a curated catalogue of reported incidents). DNS as a channel, mitigations.
- [N32] smoltcp, README, https://raw.githubusercontent.com/smoltcp-rs/smoltcp/main/README.md, primary. Goals, licence, features and missing features, Cargo features.
- [N33] smoltcp, CHANGELOG, https://raw.githubusercontent.com/smoltcp-rs/smoltcp/main/CHANGELOG.md, primary. Versions and dates, congestion control, DNS, DHCP, IPv6, MSRV, fixes.
- [N34] smoltcp, `Cargo.toml`, https://raw.githubusercontent.com/smoltcp-rs/smoltcp/main/Cargo.toml, primary. Version 0.14.0, `0BSD`, `rust-version`, dependencies, the features table.
- [N35] smoltcp 0.14.0, `socket::dns::Socket`, https://docs.rs/smoltcp/0.14.0/smoltcp/socket/dns/struct.Socket.html, primary.
- [N36] smoltcp 0.14.0, `wire::DnsQueryType`, https://docs.rs/smoltcp/0.14.0/smoltcp/wire/enum.DnsQueryType.html, primary.
- [N37] smoltcp 0.14.0, `socket::dhcpv4::Socket`, https://docs.rs/smoltcp/0.14.0/smoltcp/socket/dhcpv4/struct.Socket.html, primary.
- [N38] smoltcp 0.14.0, `phy::Device`, https://docs.rs/smoltcp/0.14.0/smoltcp/phy/trait.Device.html, primary.
- [N39] lwIP, README, https://raw.githubusercontent.com/lwip-tcpip/lwip/master/README (the project's GitHub mirror; the savannah `cgit` copy returned 404 this session), primary. Licence, protocols, APIs, footprint.
- [N40] embassy-net, README, https://github.com/embassy-rs/embassy/blob/main/embassy-net/README.md, primary.
- [N41] crates.io API, `xarxa`, https://crates.io/api/v1/crates/xarxa, primary (registry data). Reserved name, no code.
- [N42] rustls, README, https://raw.githubusercontent.com/rustls/rustls/main/README.md, primary. Providers, 0.24's change, licence, TLS versions.
- [N43] rustls, `rustls/Cargo.toml` (main, 0.24.0-dev.2), https://raw.githubusercontent.com/rustls/rustls/main/rustls/Cargo.toml, primary. Dependencies, features, `rust-version`.
- [N44] crates.io API, `rustls`, https://crates.io/api/v1/crates/rustls, primary (registry data). 0.23.46, licence.
- [N45] rustls 0.23.46, crate documentation, https://docs.rs/rustls/0.23.46/rustls/index.html, primary. The `std` feature and the items it gates, MSRV.
- [N46] rustls 0.23.46, `time_provider::TimeProvider`, https://docs.rs/rustls/0.23.46/rustls/time_provider/trait.TimeProvider.html, primary.
- [N47] embedded-tls, README, https://github.com/drogue-iot/embedded-tls/blob/main/README.md, primary. Scope, verifiers, clock, limits.
- [N48] crates.io API, `embedded-tls`, https://crates.io/api/v1/crates/embedded-tls, primary (registry data). 0.19.0, Apache-2.0, description.
- [N49] BearSSL, "Goals", https://bearssl.org/goals.html, primary. Licence, no `malloc`, TLS versions, design.
- [N50] BearSSL, "TLS 1.3", https://bearssl.org/tls13.html, primary. Not implemented, roadmap.
- [N51] BearSSL, "X.509", https://bearssl.org/x509.html, primary. The minimal engine, `br_x509_minimal_set_time`, anchors, names.
- [N52] BearSSL, home page, https://bearssl.org/index.html, primary. Version 0.6, beta quality.
- [N53] Mbed TLS, README, https://raw.githubusercontent.com/Mbed-TLS/mbedtls/development/README.md, primary. Licence, configuration, PSA.
- [N54] Mbed TLS 3.6, `include/mbedtls/mbedtls_config.h`, https://raw.githubusercontent.com/Mbed-TLS/mbedtls/mbedtls-3.6/include/mbedtls/mbedtls_config.h, primary, first 100,000 characters. Time, entropy and platform macros, TLS 1.3.
- [N55] ring, README, https://raw.githubusercontent.com/briansmith/ring/main/README.md, primary. Toolchain caution.
- [N56] ring, `Cargo.toml`, https://raw.githubusercontent.com/briansmith/ring/main/Cargo.toml, primary. Licence, features, `cc`, `getrandom`.
- [N57] ring, `BUILDING.md`, https://raw.githubusercontent.com/briansmith/ring/main/BUILDING.md, primary. C toolchain, cross-compiling.
- [N58] getrandom 0.2.17, crate documentation, https://docs.rs/getrandom/0.2.17/getrandom/, primary. Unsupported targets, `register_custom_getrandom!`.
- [N59] rustls-rustcrypto, README, https://raw.githubusercontent.com/RustCrypto/rustls-rustcrypto/master/README.md, primary. Status, `std` requirement, suites, licence.
- [N60] crates.io API, `rustls-rustcrypto`, https://crates.io/api/v1/crates/rustls-rustcrypto, primary (registry data).
- [N61] graviola, README, https://raw.githubusercontent.com/ctz/graviola/main/README.md, primary. Scope, architectures, status, algorithms, licence.
- [N62] crates.io API, `graviola`, https://crates.io/api/v1/crates/graviola, primary (registry data). 0.4.3, licence.
- [N63] crates.io API, `webpki-roots`, https://crates.io/api/v1/crates/webpki-roots, primary (registry data). `CDLA-Permissive-2.0`.
- [N64] QEMU, "'virt' generic virtual platform", https://www.qemu.org/docs/master/system/arm/virt.html, primary. Devices, options.
- [N65] QEMU, "Network emulation", https://www.qemu.org/docs/master/system/devices/net.html, primary. User-mode networking, addresses, `hostfwd`, ICMP, passt, TAP.
- [N66] QEMU, "Invocation", https://www.qemu.org/docs/master/system/invocation.html, primary, characters 100,000 to 200,000. `-netdev user` options, `-netdev tap`, `filter-dump`, `rng-random`, `rng-builtin`, `-rtc` (the `-rtc` text from `qemu-options.hx` [N67]).
- [N67] QEMU, `qemu-options.hx`, https://gitlab.com/qemu-project/qemu/-/raw/master/qemu-options.hx, primary, characters 100,000 to 295,778. `restrict`, `guestfwd`, `queues=`, `-rtc`, the vmnet help strings.
- [N68] QEMU, `target/arm/hvf/hvf.c`, https://gitlab.com/qemu-project/qemu/-/raw/master/target/arm/hvf/hvf.c, primary. Host ID registers passed through; no `RNDR` handler.
- [N69] QEMU, `hw/arm/virt.c`, https://gitlab.com/qemu-project/qemu/-/raw/master/hw/arm/virt.c, primary, first 100,000 characters. `create_rtc`, the virtio-mmio transports, the default NIC on PCIe.
- [N70] QEMU, `hw/net/virtio-net.c`, https://gitlab.com/qemu-project/qemu/-/raw/master/hw/net/virtio-net.c, primary, characters 100,000 to 146,538. The device's properties and defaults, queue pairs from the peer.
- [N71] QEMU, `net/vmnet-common.m`, https://gitlab.com/qemu-project/qemu/-/raw/master/net/vmnet-common.m, primary. The failure strings.
- [N72] P. Ennen, "[PATCH 0/2] net/macos: implement vmnet-based network device", qemu-devel, 2021, https://patchew.org/QEMU/20210204162544.65439-1-phillip.ennen@gmail.com/, primary (the patch author's cover letter). Entitlement or root, modes, macOS version.
- [N73] QEMU, "A-profile CPU architecture support", https://www.qemu.org/docs/master/system/arm/emulation.html, primary. `FEAT_RNG` emulated, the `max` CPU.
- [N74] OASIS, Virtio specification, network device (`device-types/net/description.tex`), https://raw.githubusercontent.com/oasis-tcs/virtio-spec/master/device-types/net/description.tex, primary, first 100,000 characters. Feature bits, queues, header, configuration, buffer sizes, the no-offload rules.
- [N75] OASIS, Virtio specification, entropy device (`device-types/entropy/description.tex`), https://raw.githubusercontent.com/oasis-tcs/virtio-spec/master/device-types/entropy/description.tex, primary.
- [N76] Linux, "ARM64 ELF hwcaps", https://www.kernel.org/doc/html/latest/arch/arm64/elf_hwcaps.html, primary. `HWCAP2_RNG`.
- [N77] Apple, "Determining Instruction Set Characteristics" (data endpoint), https://developer.apple.com/tutorials/data/documentation/kernel/1387446-sysctlbyname/determining_instruction_set_characteristics.json, primary. The documented `hw.optional.arm.FEAT_*` keys.
- [N78] Linux, `drivers/rtc/rtc-pl031.c`, https://raw.githubusercontent.com/torvalds/linux/master/drivers/rtc/rtc-pl031.c, primary. Register offsets, `read_time`, `set_time`.
- [N79] D. Mills, RFC 4330, "Simple Network Time Protocol (SNTP) Version 4", https://www.rfc-editor.org/rfc/rfc4330.html, primary. The exchange, accuracy, poll interval.
- [N80] Ubuntu, package `qemu-system-arm` in noble, https://packages.ubuntu.com/noble/qemu-system-arm, primary (distribution data). Depends on `libslirp0`; QEMU 8.2.2.
- [N81] musl, `src/network` (directory listing), https://github.com/kraj/musl/tree/master/src/network (mirror of git.musl-libc.org), primary.
- [N82] musl, `src/network/res_msend.c`, `lookup_name.c`, `resolvconf.c`, `getaddrinfo.c`, `lookup_serv.c`, https://raw.githubusercontent.com/kraj/musl/master/src/network/ (downloaded and read locally), primary. The resolver's system calls and files.
- [N83] curl 8.17.0 source archive, https://curl.se/download/curl-8.17.0.tar.gz (SHA-256 `e8e74cdeefe5fb78b3ae6e90cd542babf788fa9480029cfcee6fd9ced42b7910`), primary. Call-site counts over the 180 `.c` files under `lib/`, subdirectories included.
- [N84] GNU wget 1.25.0 source archive, https://ftp.gnu.org/gnu/wget/wget-1.25.0.tar.gz (SHA-256 `766e48423e79359ea31e41db9e5c289675947a7fcf2efdcedb726ac9d0da3784`), primary. Call-site counts over `src/*.c`.
- [N85] BearSSL, "Supported features", https://bearssl.org/support.html, primary. Protocol versions, cipher suite families.
- [N86] Plan 9, `tlssrv(8)`, https://9p.io/magic/man2html/8/tlssrv, primary. `tlssrv`, `tlsclient`, the key in factotum.
- [N87] Plan 9, `factotum(4)`, https://9p.io/magic/man2html/4/factotum, primary. The agent, `rpc`, secret attributes.
- [N88] Fuchsia, `zx_cprng_draw`, https://fuchsia.dev/reference/syscalls/cprng_draw, primary. The kernel CPRNG system call.
