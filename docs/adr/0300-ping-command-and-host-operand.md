# ADR-0300: The `ping` command, the `host` operand and a per-request round trip in the diagnostic echo

- **Status:** proposed
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
2. The echo has no port: `Scope::reaches_address` is its only rule (ADR-0281 item 3).
3. A command's output should be the one every other `ping` has: a line per reply, a summary, and a refusal says what it refused (CP-12).

## Considered options
### Option A: a `host` operand kind (the recommendation)
A new description kind `host`, beside `url`: a literal operand adds a rule for that host, any port, over TCP only (the
mark of an operand's rule; `grants` prints `(operand HOST)`); a value designates only inside a `+net` word, as for `url`.
### Option B: `+net HOST ping HOST` only
Nothing new in the shell; the person writes the host twice. Rejected as the only way (the `url` precedent), allowed still.
### Option C: a `raw` request in the protocol
ADR-0222's revisit trigger. Far larger; a raw socket is a right of its own.

## Recommendation
**Recommendation (not a decision): Option A**, and:
1. `ping [--count N] [--timeout SECONDS] HOST` (`-c`, `-W`), count 1 to 16 (default 4), timeout 1 to 30 s (default 2), one
   request every 200 ms. A line per reply (`40 bytes from A: seq=N time=T ms`), a summary, exit status 1 when none came, 2
   for wrong use.
2. `PingReply` gains `rtts`, one entry per request in order (microseconds, or `NO_REPLY` = `u32::MAX`). It is a change of
   ADR-0281's message, in the one place every client is in this tree; the three summary fields stay.
3. **HOST is a dotted IPv4 address.** A name is refused by the command ("names are not looked up"): a name rule does not
   cover the address it resolves to (INV-0220-4), so a ping by name needs either `+net any` or a ping request that takes a
   name; neither is built here.
4. The service answers when the run ends, so the lines appear together at the end, not one by one.

## Consequences
- **Positive.** `ping 10.0.2.2` works under `make run-net`; nothing broader than the host written is granted.
- **Negative.** No names; the rule an operand adds says TCP though the echo has no protocol; no live lines.
- **Follow-up.** A ping that takes a name, or a streaming reply, with the services channel.

## Invariants
- **INV-0300-1.** The reply of a diagnostic echo has one `rtts` entry per request, in order, and the entries that are not
  `NO_REPLY` number `received`. *Check:* `cargo test -p zoraya-net` (the stack's echo run), `make test-net` (`t_netd`).

## Open questions
- Whether names should be pinged, and by which request.

## Revisit triggers
- A program needing more ICMP than an echo: the `raw` request of ADR-0222.

## Architect's decision
(Left empty for the human.)
