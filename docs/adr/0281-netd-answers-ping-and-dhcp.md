# ADR-0281: What N2 decided where the network ADRs are silent: smoltcp as a registry crate, the scope's encoding, a diagnostic echo request, and the first supervision of the network

- **Status:** proposed
- **Date:** 2026-11-11
- **Depends on:** ADR-0052, ADR-0054, ADR-0055, ADR-0157, ADR-0220, ADR-0221, ADR-0222, ADR-0224, ADR-0229, ADR-0230, ADR-0280
- **Evidence:** CP-12, CP-14, network/survey.md#smoltcp, network/survey.md#qemu-networking-for-development-and-ci

## Context

N2 of TASK-26 builds `netd`: smoltcp over the frame protocol of ADR-0280, the DHCP client, `status`, a diagnostic echo,
the `Scope` type, and `init`'s supervision of both processes. The accepted ADRs fix the shape (ADR-0221 items 3 and 4,
ADR-0220 items 2, 3 and 5, ADR-0229) and leave a few things open that the code could not leave open. Each is small; the
first is a change to a written rule about the build, so it is listed first. A second implementation of the scope
decoder or a reviewer of the first needs them in one place (CLAUDE.md, "Decisions").

**What is measured.** `make test-net` under HVF (docs/design.md, "Network service"): a lease in about 110 ms after
`netd` starts, four echo replies from `10.0.2.2` with round trips of 0.2 to 0.5 ms, and the restart of both processes
after either is killed with frames in flight. **What is assumed.** That one interface, IPv4 only and one root
connection per launcher are all that N3 to N6 need.

## Decision drivers

1. Everything `netd` reads from outside the machine goes through code that is total and tested with hostile input
   (INV-0221-2, CP-12), and a connection holds a bounded amount of state (CP-14).
2. The build must stay reproducible and stay offline once its inputs are fetched (CLAUDE.md, "Environment").
3. A decision that only narrows (a scope, a right) must not be able to widen (ADR-0220 item 5).
4. Nothing here changes an accepted decision; where it would, it says so and waits for the architect.

## Considered options

### Option A: smoltcp 0.14.0 as a registry dependency, pinned (the recommendation)

`smoltcp = "=0.14.0"`, `default-features = false`, `alloc`, `medium-ethernet`, `proto-ipv4`, `socket-dhcpv4`,
`socket-icmp`, `socket-udp`, `socket-tcp`, `socket-tcp-reno`. `Cargo.lock` gets registry entries with checksums:
smoltcp, managed, byteorder, heapless (0.9.3), bitflags 1.3.2, cfg-if, hash32, stable_deref_trait, and smoltcp's
optional `defmt` crates, which no build compiles. The first build of a checkout needs crates.io (`cargo fetch`);
after it the build is offline. All are 0BSD, MIT or Apache-2.0 (docs/dependencies.md has the rows).

- *For:* the smallest change; the lock file and the exact pin make the build reproducible; upstream fuzzes the parsers.
- *Against:* CLAUDE.md says `make test` must not need network once the toolchain is installed, and
  docs/dependencies.md said `Cargo.lock` has no registry entries. Both are false after this change until the crates are
  fetched. A CI runner without a cache downloads them each time.

### Option B: vendor the crates into `vendor/`

`cargo vendor` with a source replacement in `.cargo/config.toml`: the build never needs the network. The tree gains
about 10 MB (the optional `defmt` crates come with it, as the lock lists them), third-party files that the host-path
check (author e-mail addresses in manifests) and clippy's workspace walk must skip, and a diff that nobody reads.

### Option C: write the stack in-house

Ruled out in `network/survey.md#smoltcp` and ADR-0221 (a TCP with congestion control, DHCP and ARP is the code whose
bugs are remote).

## Recommendation

**Recommendation (not a decision): Option A for now, Option B when a second registry crate arrives or the CI cache is
a cost** (the TLS provider of ADR-0228 brings `ring` and rustls, which is the natural time). Tell the architect the rule
changed.

1. **Registry dependency.** As Option A. `zoraya-net`'s test code also uses `heapless` directly (a scripted DHCP
   reply carries smoltcp's DNS list type).
2. **The scope's encoding** (`zoraya_abi::net`, ADR-0220 item 2, which fixes the semantics and not the bytes). Version
   byte 1; rule counts; reach rules `kind, target, ports, protocols`; listen rules `ports, protocols`; ports `0xFF` for
   any or a count of 1 to 8 and `(low, high)` pairs; protocols 1, 2 or 3. Where the ADR is silent: addresses are IPv4
   (ADR-0229 item 5 has one interface and no IPv6); a name's last label is not all digits, so no name reads as an address
   (INV-0220-4); a `Prefix` with host bits set is refused, so a set has one spelling and the decoder transforms
   nothing (INV-0220-3); a child rule must lie inside **one** rule of its parent, so two parent rules that together
   cover a child do not (conservative: `narrow` may refuse what a union would allow, never the reverse); a `Prefix`
   of length 32 is not under the `Address` it equals (same reason). The encoded scope is at most 24 KiB.
3. **A diagnostic echo request** (`ping` in `zoraya-proto-net`, ordinal 2): `count` (at most 16) echo requests one every
   200 ms to an IPv4 address, answered when the run ends. It needs the right `reach` and a reach rule that covers the
   address on some port, and is the only ICMP `netd` sends; there is no raw socket. It exists because the plan's
   acceptance test (`ping 10.0.2.2` through a test request in `netd`) and the shell's future `ping` need it; ADR-0222's
   revisit trigger "a program needing ICMP: a `raw` request behind a right of its own" is where a general one would go.
   smoltcp's automatic reply to echo requests addressed to the guest is **off** in `netd` (feature `echo-reply` only in
   the host tests). It is not true that the guest answers nothing unasked: smoltcp answers ARP, a UDP datagram to an
   unbound port with ICMP port unreachable, an unknown IP protocol with ICMP protocol unreachable, and a TCP segment
   to a closed port with RST (N2's review, finding 7). One reply per received frame, bounded by `BURST`, so no
   amplification; smoltcp 0.14 has no switch for them (a raw socket would suppress the port-unreachable only), so they
   stay on, and turning them off would be a fork of smoltcp. The echo run's sockets are replaced when a run ends, so
   one request that could not leave never blocks the next run.
4. **Settings types.** ADR-0229 proposes `address` and `prefix` for ADR-0157's list; `zoraya-describe` has them
   (dotted decimal, no sign, no leading zero, `a.b.c.d/len` with `len` 0 to 32; the empty value means "not set"), so
   `settings net.gateway 10.0.2` is refused at the prompt: `zosh` declares `netd.cmd`'s settings beside its own and
   the compositor's, and `tests/shell/net-settings.script` runs `settings` (INV-0229-1). `net.dns` is `list of address 0 4`; `net.hosts`
   stays `list of text` (`name=address`, checked by `netd`).
5. **Start-up record and description key.** `native::Slots` gains `net`, the byte that was the first reserved byte
   (ADR-0220 item 7), and `zoraya-describe` gains `needs net-status` (ADR-0224 item 2), honoured by `zosh` for base-system
   commands. N2 uses them for `net status` only; the `+net` words, `grants` and the REPORT message stay with N3.
6. **`netd` reads `/config` once.** It reads `settings` at start and closes the connection; there is no `reload` and no
   watch (ADR-0229 item 2 and its open question). A change applies at the next start of `netd`.
7. **Supervision (ADR-0221 items 4 and 5, first half).** `init` starts `ethd` and `netd` once the storage stack is up
   (`netd` needs `/config`), keeps a root connection to `netd`, and makes the shell's root connection (slot 9) with
   `narrow` before each shell start. When either process ends, `init` ends both (service first, then the driver) and
   starts both again, at most three times per boot. A shell that is running keeps its dead connection: `net status` then
   says the network is not available until the shell is started again. ADR-0221 item 6's services channel
   (`NetworkRestarted`) is **not** built; the panel's and the broker's connections are N3's.
8. **What the lease contains under `restrict=on`** (`network/survey.md#qemu-networking-for-development-and-ci` has the
   network). SLIRP leaves the router and the DNS servers out of the lease when
   restricted (measured, `docs/design.md`), so under `make test` `net status` shows an address and a lease time only; the
   gateway `10.0.2.2` and DNS `10.0.2.3` appear under `make run-net`. `netd` takes what the lease gives and invents no
   default. ADR-0230 is unchanged: the test network is restricted, and the tests that need a gateway are N3's
   (the runner's HTTP server is on the host side of the same restriction).
9. **smoltcp's seed** is the monotonic time at start mixed with the MAC address. It is not a secret and not good
   entropy; ADR-0227's service replaces it in N5.

## Consequences
- **Positive.** `netd` answers ping and DHCP with all outside bytes parsed by tested code; the scope is a value other
  parts can build on; the supervision exists before the first program depends on the network.
- **Negative.** The build needs crates.io once; a running shell does not follow a restart of the network; one
  diagnostic request is outside ADR-0222's list.
- **Follow-up.** N3 completes the scope builder, the REPORT message and the launchers' connections; N5 replaces the seed.

## Invariants
- **INV-0281-1.** A request kind that exists in `zoraya-proto-net` is checked against the connection's rights and scope in
  one function (`Connection::decide`), and a rule `decide` allows for `Request::Ping` covers the address.
  *Check:* `cargo test -p zoraya-net` (`connection` and `service` tests, every refusal path, random requests).

## Open questions
- Option A or B for the build (above); the architect decides when to vendor.
- Whether a `ping` request should stay in the protocol once a `raw` request exists.
- Whether the shell should be restarted with the network, as ADR-0080 restarts it with the storage stack, or be told
  (ADR-0221 item 6).

## Revisit triggers
- A second registry crate, or a CI run that fails on a registry fetch: Option B.
- A launcher other than the shell asking `netd` for a connection: the services channel of ADR-0221 item 6.
- IPv6 in the stack: the scope's address kinds and `Prefix` get a family byte (version 2).

## Architect's decision

(Left empty for the architect.)
