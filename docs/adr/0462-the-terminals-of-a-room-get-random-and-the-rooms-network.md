# ADR-0462: The terminals a room's panel starts get the random service and the network its room's words allow

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0172, ADR-0221, ADR-0224, ADR-0227, ADR-0242, ADR-0340, ADR-0393
- **Evidence:** CP-01, CP-28

## Context

Only the shell `init` starts (the first terminal's, or the console's) got a network connection and a connection to the
random service (ADR-0221 item 4, ADR-0227 item 3). A terminal the panel starts, in a room or at home, got neither:
ADR-0224 item 2 says a host from a value designates "inside a room only inside the room's words", but a room's line had
no words besides `+r` and `+w`. A POSIX program started there could not seed OpenSSL (`ssh` and `ssh-keygen` stopped
with "PRNG is not seeded") and could not reach any host. The architect decided on 2026-11-16 that a room's terminals
always get a random-number connection, and the network by the room's grants (a room made with `+net HOST:22` can ssh to
HOST). This ADR records the shape built in 10g.

## Decision drivers

1. A program gets what the line says, and a room's terminals what the room says (ADR-0172, ADR-0224).
2. A room holds only what its creator holds (INV-0172-4).
3. One room's connection reveals nothing of another's (CP-01).

## Considered options

### Option A: the panel gives each terminal's shell a random connection, and the network narrowed to the room's words
- **Description:** a room's line may carry `+net WORD` (ADR-0224 item 1's words); `init` gives the panel the random
  service and a root network connection (slots 11 and 12); each shell it starts gets, at slots 10 and 9, its own
  `Share` of the random service and a connection narrowed to `REACH | NARROW` with the scope of the room's words (none
  when the room has none).
- **Evidence for:** drivers 1 to 3; each connection is the shell's own.
- **Evidence against:** the panel holds a root network connection.
- **Cost:** *(estimate)* 250 lines and a test.
- **Precedent:** ADR-0221 item 4 for `init`'s shell.

### Option B: every terminal gets the first shell's connection
- **Evidence against:** driver 1 and CP-28: a room would reach what its words do not name.

### Option C: nothing; a room's programs that need the network run in the first terminal
- **Evidence against:** the decision of 2026-11-16.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **Room words.** `/config/rooms` lines and `room new NAME ... +net WORD` carry `+net` words (at most eight), shown by
   `room`, written back by the reader's writer, and shown as pills of the terminal's frame. A room made from a shell that
   holds no network, or from inside a room whose words do not include the word, is refused. An untrusted room has none
   (the gate).
2. **The panel** gives every terminal's shell a connection to the random service of its own (home's too), and, only in a
   room with words, a network connection narrowed to `REACH | NARROW` and exactly those words; the shell gets the room's
   words with `--net` to show and use them (a value host designates inside them).
3. **What is not given:** the status right and the listen right (no `ping`, `net status` or listening in a room's
   terminal); no services channel, so a restart of the network service is not followed by those shells; no network at
   home's second terminals.

## Consequences

- **Positive.** `ssh` and `ssh-keygen` run in a room's window; the room's words bound what it reaches.
- **Negative.** The panel holds `init`'s root network connection; a network restart leaves panel-started shells
  with a closed connection until the terminal is restarted.
- **Follow-up.** The room overview form's words; `room set NAME +net` (not built: the line is changed by editing).

## Invariants

- **INV-0462-1.** The network connection of a room's terminal reaches exactly the hosts and ports of the room's `+net`
  words, and none when the room has none. *Check:* `cargo test -p zoraya-shell --lib netgrant` (the scope of the words),
  `cargo test -p zoraya-panel`, and `make test-apps` (`tests/gui/ssh-room.script`: a room with a word logs in, another
  port of the same host is refused, a room with none is refused).
- **INV-0462-2.** Every terminal's shell has a random connection of its own. *Check:* `make test-apps` (`ssh-keygen` in
  a room's window makes a key).

## Open questions

- Whether the panel should follow a restart of the network service for the shells it started.
- Whether home's second terminals should get the first terminal's network (the decision is for rooms).

## Revisit triggers

- Rooms' words for listening (`+listen`), or a rooms overview form.

## Architect's decision
