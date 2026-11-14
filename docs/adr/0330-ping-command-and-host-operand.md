# ADR-0330: The `ping` command, the `host` operand and a per-request round trip in the diagnostic echo

- **Status:** accepted
- **Date:** 2026-11-13
- **Depends on:** ADR-0220, ADR-0222, ADR-0224, ADR-0281, ADR-0297
- **Evidence:** CP-12, DM-034

## Context
ADR-0281 item 3 gave `netd` a diagnostic echo request and said "the shell's future `ping` needs it". The architect's
hand-run found there is no `ping` command. Three things the code could not leave open: (a) how the shell designates the
host (ADR-0224 item 2 has the `url` operand, which carries a port, and `+net`, which the person writes); (b) the reply
of the diagnostic carries a minimum, mean and maximum only, so a command cannot print one line per reply; (c) names.
Measured: SLIRP answers echo requests to 10.0.2.2 with round trips of 0.2 to 0.5 ms (ADR-0281).

## Decision drivers
1. The network a command gets is what the person wrote on the line (ADR-0153 rule 1, ADR-0224; DM-034, nothing moves unasked).
2. The echo has no port: a rule for it should grant the echo and nothing else (ADR-0281 item 3).
3. A command's output should be the one every other `ping` has: a line per reply, a summary, and a refusal says what it refused (CP-12).

## Considered options
### Option A: a `host` operand kind and an echo-only rule (the recommendation, as the architect decided)
A new description kind `host`, beside `url`: a literal operand that is a host name or a dotted IPv4 address adds a rule for
that host, over ICMP echo only (no TCP, no UDP; `grants` prints `echo only (operand HOST)`). A value designates only inside
a `+net` word, as for `url`; a `+net` word covers TCP, UDP and echo. `netd` resolves a name inside the service, as `fetch`
does (ADR-0220).
### Option B: `+net HOST ping HOST` only
Nothing new in the shell; the person writes the host twice. Rejected as the only way (the `url` precedent), allowed still.
### Option C: a `raw` request in the protocol
ADR-0222's revisit trigger. Far larger; a raw socket is a right of its own.
### Option D: the operand's rule is TCP, any port (the first text of this ADR)
Reuses the scope encoding, but the connection `ping` holds could open TCP connections to every port of the host. Rejected
by the architect.

## Recommendation
**Recommendation (not a decision): Option A**, which is what the architect decided (below):
1. `ping [--count N] [--timeout SECONDS] HOST` (`-c`, `-W`), count 1 to 16 (default 4), timeout 1 to 30 s (default 2), one
   request every 200 ms. `PING HOST (ADDRESS): N requests`, a line per reply (`40 bytes from A: seq=N time=T ms`), a
   summary, exit status 1 when none came, 2 for wrong use.
2. `PingReply` gains `rtts`, one entry per request in order (microseconds, or `NO_REPLY` = `u32::MAX`), and `address`, the
   address pinged. It is a change of ADR-0281's message, in the one place every client is in this tree.
3. **`Ping` takes a name** (`Data<253>`, the bound of `Connect`'s and `Resolve`'s; empty means `address`). `netd` checks the
   name against a rule that grants echo, resolves it with its resolver (a `net.hosts` entry or the DNS), and pings the first
   unicast answer, other than our own address, that `is_local` does not count (this machine, the gateway, the DNS servers)
   unless an echo rule covers it (ADR-0223 item 4, as `resolve`).
   One ping at a time covers the lookup too.
4. **`Protocols` gains `ECHO` (4) and `ALL` (7)** in `zoraya_abi::net` (`BOTH` stays TCP and UDP). A reach rule's protocols
   decide what the rule grants: a connection (TCP) needs TCP, a ping needs ECHO, so an echo-only grant refuses every
   connection and a TCP/UDP grant refuses every echo. A listen rule with the echo bit does not decode. The root scope and the
   `+net` words are `ALL`. The ABI had no users outside this tree, so the scope's encoding changed in place (the protocol
   byte takes 1 to 7, was 1 to 3).
5. The service answers when the run ends, so the lines appear together at the end, not one by one.

## Consequences
- **Positive.** `ping 10.0.2.2` and `ping example.org` work under `make run-net`; the connection `ping` holds can send ICMP
  echo requests to the host written and do nothing else.
- **Negative.** No live lines. A `+net` word now also grants echo. A name resolves through the person's resolver for a
  ping, as for a fetch. Any code that built a reach rule with `BOTH` and expected a ping through it must use `ALL`.
- **Known limits (review of 62a7474).**
  - *`resolve` is not echo-gated.* `Request::Resolve` asks a rule that covers the name on any protocol, so an echo-only rule
    for a name also lets that connection `resolve` the name and read the whole address list. Same name, nothing beyond what
    `ping` shows leaks (the pinged address is in the reply anyway); kept, and INV-0330-2 does not cover `resolve`.
  - *What counts as this side of the network.* `is_local` counts this machine's address, the gateway and the DNS servers only:
    other hosts of the subnet and its broadcast address pass, as they do for `resolve` and `fetch`. A named ping skips a
    non-unicast answer and our own address, and uses the first remaining answer; an answer that `is_local` counts needs an
    echo rule that covers that address.
  - *Ports and echo disagree.* An echo rule has no port. A `+net example.org:443` word covers TCP, UDP and echo on port 443
    only, and the `host` operand's rule (any port) is not inside it, so `+net example.org:443 ping $h` is refused; write
    `+net example.org ping $h`.
  - *One ping at a time.* The slot is one per `netd`, and a ping by name holds it from the request through the lookup (the
    resolver's own wait, up to 8 s) to the end of the run. One holder of an echo grant can keep every other connection `Busy`
    for that time. A connection that goes while its run goes keeps the slot taken until the run ends (its result goes to
    nobody).
  - *No UDP request.* The protocol has no datagram request yet, so the UDP side of an echo-only rule is tested in
    `zoraya-abi` only.
- **Follow-up.** A streaming reply, with the services channel.

## Invariants
- **INV-0330-1.** The reply of a diagnostic echo has one `rtts` entry per request, in order, and the entries that are not
  `NO_REPLY` number `received`. *Check:* `cargo test -p zoraya-net` (the stack's echo run), `make test-net` (`t_netd`).
- **INV-0330-2.** A rule grants echo requests only if its protocols include echo, and a connection only if they include
  TCP: an echo-only grant opens no connection, a TCP/UDP grant sends no echo, for names and addresses alike. *Check:*
  `cargo test -p zoraya-abi`, `-p zoraya-net`, `-p zoraya-shell netgrant`, `make test-net` (`t_netd`); mutants in
  `tools/mutants/net-ping.toml`.

## Open questions
- Whether a streaming reply (a line per reply as it comes) should replace the answer at the end of the run.

## Revisit triggers
- A program needing more ICMP than an echo: the `raw` request of ADR-0222.

## Architect's decision
Decided by the architect 2026-11-13: (1) host operands may be names, `ping google.com` works like `fetch` (ADR-0220: the
grant names the host and the name is resolved inside the network service), so `netd` has a ping request that takes a name,
honouring the grant; (2) ping's grant is narrowed to ICMP echo only, an echo-only rule kind in the network ABI, with no TCP
and no UDP, enforced by `netd`; the earlier TCP rule is dropped. Recorded by an agent at the architect's request; the status
stays `proposed`.

Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

Option A.
