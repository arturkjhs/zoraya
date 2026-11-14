# ADR-0324: A `web` network grant for browsers: any public host on ports 80 and 443

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0172, ADR-0220, ADR-0223, ADR-0224, ADR-0262, ADR-0268, ADR-0297, ADR-0317, ADR-0330
- **Evidence:** CP-01, DM-009, alternatives-postmortem.md#genode, network/survey.md#container-and-agent-sandboxes, network/survey.md#name-resolution-as-authority, network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak, network/survey.md#the-unix-baseline

## Context
NetSurf (TASK-27 9d, ADR-0317, ADR-0318) gets the network the way curl does: a `url` operand designates its host and
port (ADR-0224 item 2) and nothing else. A browser follows links and redirects to hosts it learns while it runs, which
ADR-0220's revisit trigger names ("a program in scope that must reach hosts it learns at run time beyond a wildcard (a
browser)"). With the per-URL rule every image from another host stays missing and every link to another site is refused
unless the person adds `+net HOST` words; with `+net any` the browser reaches the person's router, the host machine
behind QEMU (10.0.2.2) and every port of everything. The architect decided (2026-11-14) a third grant between the two.

What is measured: under `make test-netsurf` a link to another host is refused by `netd` (`NotGranted`) and NetSurf
shows curl's bare "Couldn't connect to server" (9d report). What is assumed: a browser needs ports 80 and 443 only;
the person's own network is what a page must not reach without a word of the person's (the browsers' "private network
access" rule, `network/survey.md#container-and-agent-sandboxes` lists the same split in agent sandboxes: an allow-list
for the Internet, the host and the LAN always closed).

## Decision drivers
1. No ambient authority (CP-01, DM-009): the grant is in the program's description, visible in `grants` and on the
   window, and never reaches this machine or its local network.
2. Resolution is inside the grant (`network/survey.md#name-resolution-as-authority`): names are resolved by `netd`, and
   a name's answer is checked against the grant, so DNS cannot point the browser into the local network (rebinding).
3. One bit says nothing about where data goes (`network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak`):
   the grant names what it reaches (public hosts, two ports, TCP), not "the network".
4. The rule kind goes into the network ABI like the echo-only rule did (ADR-0330 item 4), so `netd` enforces it and
   `narrow` keeps it a subset.

## Considered options
### Option A: `+net any` for the browser (the Unix baseline)
- **Description:** NetSurf's description, or the person, grants `any` (`network/survey.md#the-unix-baseline`: any
  process, any socket).
- **Evidence for:** Nothing new to build; every page works.
- **Evidence against:** Reaches the gateway, the LAN, loopback and every port: the opposite of driver 1.
- **Cost:** None.
- **Precedent:** Every Unix browser.

### Option B: the per-URL rule and `+net HOST` words (what 9d built)
- **Description:** Keep the `url` operand; the person adds a word per host.
- **Evidence for:** The smallest grant; already built.
- **Evidence against:** A page of ten hosts is ten words, and the person learns them by failure.
- **Cost:** None.
- **Precedent:** curl and wget here (ADR-0311).

### Option C: a `Public` reach target, written `web`, ports 80 and 443 (the architect's decision)
- **Description:** A new reach target `Public` in `zoraya_abi::net` (encoding kind 5) covers every public address and
  every name through its public answers; the shell's word `web` is `Public` on ports 80 and 443 over TCP. A description
  line `net web` gives it. The private, loopback, link-local and other local blocks are never covered by it.
- **Evidence for:** Drivers 1 to 4; the shape of Genode's NIC router domains, which separate an uplink from local
  domains by prefix (`alternatives-postmortem.md#genode`).
- **Evidence against:** Any public host is a wide grant; the check of each name's answer is a second place where a
  connection can be refused.
- **Cost:** *(estimate)* 150 lines in `zoraya-abi`, 60 in `netd`, 80 in the shell, tests.
- **Precedent:** Chrome's and Firefox's private network access (public pages may not reach private addresses);
  Genode's router.

### Option D: a proxy in front of the browser
- **Description:** The browser reaches only a local proxy that applies a policy per request (ADR-0220's alternative
  for finer rules).
- **Evidence for:** Per-path and per-host policy, logs of every request.
- **Evidence against:** A new service; TLS would end at the proxy or the proxy sees only names.
- **Cost:** *(estimate)* A service of its own.
- **Precedent:** Agent sandboxes' egress proxies (`network/survey.md#container-and-agent-sandboxes`).

## Recommendation
**Option C**, as the architect decided:

1. **The description line.** `net web` (in `zoraya-describe`, once, commands only) gives the program the web rule. The
   shell honours it only for a description of the system image (a base-system command or a POSIX program the image
   describes), as `needs net-status`; NetSurf's description has it, curl's and wget's do not (they keep the per-URL rule).
2. **The rule** (`zoraya_abi::net::Target::Public`, kind 5, no bytes after it): it covers every address that
   `zoraya_abi::net::is_public` accepts, that is every address outside these blocks (`NOT_PUBLIC`): 0.0.0.0/8,
   10.0.0.0/8 (QEMU's 10.0.2.0/24 is in it), 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12, 192.0.0.0/24,
   192.168.0.0/16, 198.18.0.0/15, 224.0.0.0/4 and 240.0.0.0/4 (with 255.255.255.255). The documentation blocks
   (192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24) are public: no network uses them, and the NetSurf test runs QEMU's
   network on 203.0.113.0/24 as its Internet. It covers a name only through the name's answer: a connection or a lookup
   that only a web rule admits (`Scope::name_grant`, `Scope::resolve_grant`: `PublicOnly`) uses only answers that are
   public and not local in ADR-0223 item 4's sense (this machine, the gateway, the DNS servers), `net.hosts` answers
   included (their exemption is for a name the person granted by name). A web rule never *names* an address
   (`Scope::names_address`, `reaches_address`): an address it admits by range is still refused when it is local (a
   public gateway). `+net HOST` words and the URL operand's rule sit beside it on the line, as any rules do: a host
   they name is reached under their rule.
3. **The shell's rule.** `Public`, ports 80 and 443, TCP only: no UDP, no ICMP echo, no listen. It joins the line's
   other rules (ADR-0220 item 7: one connection, the union). It does not make a value designate: a URL from a variable
   still needs a `+net` word (ADR-0224 item 2), since the shell cannot know before a lookup whether its host is public.
4. **The subset relation.** Only a web rule (with ports and protocols inside) is within a web rule, and a web rule is
   within only a web rule or `any`: an address, prefix or name rule is never within it, because those rules skip the
   check of the answer (INV-0324-2). A program with the web grant can give a helper the web grant on fewer ports, and
   nothing else.
5. **Lookups.** `resolve` of any valid name is allowed under a web rule (names resolved for the program, ADR-0220 item
   4); the answer holds only the public, non-local addresses, and with none left it is `NotGranted` (not `Local`):
   the grant is why, and `libposix`'s `getaddrinfo` reports it as `EAI_SYSTEM` with `EACCES` (ADR-0325 uses it). Each
   lookup counts against the connection's account (Q3), as any lookup. A web grant can send any name to the DNS: the
   price of driver 2's resolution inside the service (ADR-0220's consequences, for wildcards).
6. **Showing it.** `grants` prints `net: web  reach public hosts, ports 80 and 443; not this machine or its network
   (description)`. The window of the program shows a pill `web` (not `+net web`, which would be a host named `web`).
7. **Untrusted rooms.** As every network grant, it is refused there while the gate's second stage is closed (ADR-0268
   point 7). Beside that refusal, a description's `net web` has a refusal of its own (`untrusted-gate: web-refusal`,
   in the shell's `netgrant::web_refused` and its call in `zosh`), which stays after the second stage opens: an
   untrusted room's network is its own words (ADR-0224 item 4), and a description must not add the public Internet to
   it. `tools/check-untrusted-gate.sh` fails if either marker is gone; `tools/untrusted-gate.txt` stays as it is (the
   gate stays closed).

## Consequences
- **Positive.** NetSurf follows links and redirects to other public hosts and loads their images; the router, the host
  machine, loopback and other ports stay out unless the line names them; the window says `web`.
- **Negative.** A wide grant: any public host can be reached on two ports, so data a page holds can go anywhere public.
  A second check (of answers) in `netd`. A name the web rule alone admits may not use `net.hosts` for a local address.
- **Follow-up.** IPv6's blocks when the stack has IPv6 (ADR-0229 item 5); the room words of ADR-0224 item 4 could take
  `+net web` later (not built: the word `web` on a line is a host named `web`).

## Invariants
- **INV-0324-1.** A connection or lookup that only a web rule admits uses no address that `is_public` refuses and no
  address that is local (this machine, the gateway, a DNS server), `net.hosts` answers included, unless another rule of
  the scope names the address; a web rule admits no port but its own and no protocol but TCP. *Check:* `cargo test -p
  zoraya-abi` (the blocks at their edges, the rule's matcher), `cargo test -p zoraya-net` (connect by name and address,
  resolve, `net.hosts`), `make test-netsurf` (links to 10.0.2.2, 127.0.0.1 and the gateway refused, with no request at
  the server).
- **INV-0324-2.** No rule but a web rule is within a web rule, and a web rule is within no rule but a web rule or
  `any`. *Check:* `cargo test -p zoraya-abi` (the subset property over random scopes with web rules), `cargo test -p
  zoraya-net` (`narrow` through the service).
- **INV-0324-3.** The shell gives the web rule only for a description of the system image with `net web`, only outside
  an untrusted room, and only as `Public`, ports 80 and 443, TCP. *Check:* `cargo test -p zoraya-shell` (netgrant),
  `make check-untrusted-gate` (the `untrusted-gate: web-refusal` markers).
- **INV-0324-4.** The window of a program with the web rule shows a `web` pill, and `grants` prints the rule.
  *Check:* `cargo test -p zoraya-shell` (pills and `grants` lines), `make test-netsurf` (the pill on the window).

## Open questions
- Whether a second word (`web+` for any port) is wanted; not now.
- Whether the documentation blocks should be excluded too once the tests do not need a public-looking network.

## Revisit triggers
- IPv6 (ADR-0229 item 5): the blocks gain fc00::/7, fe80::/10 and ::1.
- A browser that needs WebSockets on other ports, or HTTP/3 (UDP 443).

## Architect's decision

Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

The architect's decision (2026-11-14): a "web" network grant for browsers. A description line `net web` gives the
program any public host on ports 80 and 443, with names resolved for it; loopback, link-local, private ranges and
QEMU's 10.0.2.0/24 are excluded unless named by `+net HOST` words. The compositor shows a "web" pill; `grants` shows
it as text. NetSurf's description takes it; curl and wget keep the per-URL rule. Refused in untrusted rooms as before
(the gate stays closed), with a marker for the web grant's refusal and a test.
