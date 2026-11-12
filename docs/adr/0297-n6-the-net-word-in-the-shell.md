# ADR-0297: N6, the `+net` word in the shell where ADR-0224 is silent

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0153, ADR-0220, ADR-0222, ADR-0224, ADR-0229
- **Evidence:** CP-12, DM-034

## Context
ADR-0224 item 1 gives the shell a `+net HOST[:PORT]` word beside `+r` and `+w`, item 2 lets a value designate inside such
a word, and item 3 has `grants` print the scope. N3 built only the literal `url` operand. N6 builds the word in the
shell (not the rooms' words, the panel's pills or the refusal report and hint, which need the desktop half). The ADR
leaves open the grammar of the argument, which commands may take the word, and what `grants` shows for a rule.

## Decision drivers
1. The word is the person's: its token is literal, its one argument may come from a value (ADR-0153 rule 3; ADR-0224's
   example `+net $h curl ...`).
2. A scope no wider than what was written (ADR-0224 rule 5 and the architect's fix: never `any` by suggestion).
3. One reading renders everywhere (CP-12, a refusal says what it refused; DM-034, nothing moves unasked): `grants` prints exactly the rules the connection carries (INV-0224-2).

## Considered options

### Option A: the smallest grammar that covers the ADR's examples, any described program
- **Description.** The argument is `any`, a host name, `*.SUFFIX`, a dotted address, `ADDRESS/BITS`, each with an optional
  `:PORT` (no port: every port; one word covers TCP and UDP, as the architect decided). The word may stand before any
  program with a description (`+net example.org curl ...`, `+net 10.0.2.2:80 fetch $u`); the connection it makes is the
  shell's own `narrow`ed to those rules, so `netd` refuses what the shell's scope does not cover.
- **Evidence for.** Drivers 1 to 3. **Evidence against.** `any` exists (the ADR says so).
- **Precedent.** Deno's `--allow-net=HOST:PORT` (`network/survey.md` cites it in ADR-0224).

### Option B: only names and addresses, no wildcard or prefix
- **Evidence against.** The ADR's own examples use `*.example.org`; prefixes are in ADR-0220's rules.

### Option C: a separate command, `net allow`
- **Evidence against.** Against ADR-0153 rule 1: nothing but a word on the line grants.

## Recommendation
**Recommendation (not a decision): Option A.** Built in N6.

1. `+net WORD` is a prefix word like `+r`; any number of them. `zoraya_shell::netgrant::parse_word` reads the argument;
   a bad one refuses the line (`zosh: +net nonsense:: the port is not a number from 1 to 65535`).
2. A `url` operand of a base-system command that a word covers designates nothing more; one it does not cover and that
   is literal adds its own TCP rule (as N3); one that came from a value and is not covered is refused with the word that
   would allow it (`if that is meant: +net HOST:PORT COMMAND`).
3. `grants` prints one `net:` line per reach rule: `net: example.org:443        reach (grant +net)`, `net: HOST  reach, any
   port (grant +net)`, and `(operand URL)` for the operand's TCP-only rule.
4. **Not built:** `+listen`, the room words and the creator's-scope check (INV-0224-3), the pills and hover card
   (INV-0224-5), the `REPORT` message and the hint (INV-0224-4), the roots entry that goes with a network rule (the shell
   has no `tls.roots`), highlighting of the word in the line editor.

## Consequences
- **Positive.** `+net HOST fetch $u` works; the value rule of INV-0224-1 has its second half.
- **Negative.** The word is only in the shell; a window's shell has no network connection yet.

## Invariants
- **INV-0297-1.** The rules of `net_for` are the ones `grants` prints and nothing is added: the scope of a line is its
  `+net` words, plus its literal `url` operand's rule if no word covers it. *Check:* `cargo test -p zoraya-shell netgrant`
  (`grants_prints_exactly_the_rules_the_connection_carries` and the cases of value and literal operands).

## Open questions
- Whether a `+net` word with no port should also cover a `url` operand's default port: it does (any port).

## Revisit triggers
- People type `+net any` routinely (ADR-0224's trigger).

## Architect's decision
(Left empty for the human.)
