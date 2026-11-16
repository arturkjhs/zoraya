# ADR-0462: The terminals a room's panel starts get the random service and a network bounded by their room's words

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0172, ADR-0221, ADR-0224, ADR-0227, ADR-0242, ADR-0262, ADR-0288, ADR-0340, ADR-0393, ADR-0435, ADR-0453
- **Evidence:** CP-01, CP-28

## Context

Only the shell `init` starts (the first terminal's, or the console's) got a network connection and a connection to the
random service (ADR-0221 item 4, ADR-0227 item 3). A terminal the panel starts, in a room or at home, got neither:
ADR-0224 item 2 says a host from a value designates "inside a room only inside the room's words", but a room's line had
no words besides `+r` and `+w`. A POSIX program started there could not seed OpenSSL (`ssh` and `ssh-keygen` stopped
with "PRNG is not seeded") and could not reach any host. The architect decided on 2026-11-16 that a room's terminals
always get a random-number connection, and the network by the room's grants (a room made with `+net HOST:22` can ssh to
HOST). This ADR records the shape built in 10g and the second review's findings (docs/tasks/TASK-31-review-10g-2.md).

**What is known.**

- `rngd` decides what a connection may `Share` by its depth (ADR-0288 item 6, ADR-0435): `init`'s is depth 0, a shell's
  depth 1 (64 live children), a program's depth 2 (8), its children's depth 3 (none). A share of the panel's depth-1
  connection would make every shell the panel starts a depth-2 connection, which `rngd` treats as a program.
- `netd` carves at most four accounts at once, session-wide, and eight TCP connections in all of them (ADR-0262); each
  POSIX program that is given a network takes one account.
- A room's *file* grants stand on every line of its terminals (ADR-0172 point 3).

## Decision drivers

1. A program gets what the line says, and a room's terminals what the room says (ADR-0172, ADR-0224).
2. A room holds only what its creator holds (INV-0172-4).
3. One room's connection reveals nothing of another's, and one room's ordinary use does not take another's network
   (CP-01); a connection reaches what its holder's words name (CP-28).
4. A shell the panel starts is a shell for the random service, not a program (ADR-0288, ADR-0453 item 3).

## Considered options

### Option A: the room's words bound a line; the line's own words ask (as built)
- **Description:** a room's line may carry `+net WORD` (ADR-0224 item 1's words, at most eight); `init` gives the panel a
  *launcher* connection to the random service and a root network connection narrowed to `REACH | NARROW`
  (slots 11 and 12); each shell it starts gets a random connection of its own and, in a room with words, a network
  connection narrowed to `REACH | NARROW` and exactly those words (none when the room has none). In the shell, the
  room's words **bound** every line: a line gets a network connection only if its own words ask for it (a `+net` on
  the line, or the description's designation resolved from its operands: ssh's `HOST`, curl's URL) **and** the room's
  words allow it; a rule outside them is refused before the program runs, with the room's words in the hint; a line
  that asks for nothing (`cat x`, `vi`) gets no network connection and no carved account. A value host designates
  inside the room's words.
- **Evidence for:** drivers 1 to 3; a room's ordinary programs take no account.
- **Cost:** *(estimate)* 400 lines and tests.
- **Precedent:** ADR-0221 item 4 for `init`'s shell; ADR-0224's `+net` on a line.

### Option B: every line in the room gets the room's connection
- **Description:** the words stand on every line as the person's own (what the first build did).
- **Evidence against:** drivers 1 and 3: `cat x` gets `REACH`; each POSIX program takes one of `netd`'s four carved
  accounts, so four background programs of one room (an editor, a pager) starve home's `curl` and `ssh`, silently.
- **Cost:** *(estimate)* none beyond A's.
- **Precedent:** ADR-0172 point 3 for file grants.

### Option C: nothing; a room's programs that need the network run in the first terminal
- **Description:** as before.
- **Evidence against:** the architect's decision of 2026-11-16.
- **Cost:** *(estimate)* none.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **Room words.** `/config/rooms` lines and `room new NAME ... +net WORD` carry `+net` words (at most eight), shown by
   `room`, written back by the writer, and shown as pills of the terminal's frame. A room made from a shell that holds no
   network, or from inside a room whose words do not contain the word (by the rules' `within`, not by the text), is
   refused. An untrusted room has none (the gate). A word of a hand-edited line that is no word is said once when the
   room is entered and ignored: it bounds nothing and spoils no command.
2. **Random.** The panel's connection is a *launcher's*: `rngd` records it at `init`'s depth (a new message,
   `ShareLauncher`, which only `init`'s own connection may send), so the shells it shares are depth-1 shells with a
   shell's room (64 programs, and a program may `Share`), as under `init`'s shell. This changes ADR-0288 item 6 and
   ADR-0435's table by one row (the launcher); a launcher cannot make another launcher.
3. **Network.** The panel's root connection has `REACH | NARROW` and no status or listen right; each shell's is that
   narrowed to the room's words (plus `NARROW` so that it can carve a program's own). The panel gives the shell `--net`
   with the words so that it can bound and show them. A room with `/system/tls` to read holds it (a program with a
   network is given the roots to read, ADR-0311 item 4): the frame shows `+r /system/tls` though it is no grant of the
   room.
4. **At the limit.** A line that asks for the network and meets `netd`'s limit on carved accounts is refused with
   "the network has no room for another program's connection" and the hint to wait for one to end; a connection the
   service refuses for another reason is refused naming the room's words.
5. **What is not given:** the status and listen rights (no `ping`, `net status` or listening in a room's terminal); no
   services channel, so a restart of the network service is not followed by those shells; no network at home's second
   terminals (home's `init` shell keeps its own).
6. **The first terminal after `room r`** holds `init`'s root connection: the words bound its lines in the shell
   (items 1 and the room bound above), but the connection itself is not narrowed there.

## Consequences

- **Positive.** `ssh` and `ssh-keygen` run in a room's window; the room's words bound what it reaches; a room's
  ordinary programs take no network account.
- **Negative.** The panel holds a `REACH | NARROW` root connection and a launcher's random connection; a network restart
  leaves panel-started shells with a closed connection until the terminal is restarted; a room made with
  `+w /data/ROOM` cannot run a program with an own directory (ADR-0393).
- **Follow-up.** The room overview form's words; `room set NAME +net` (not built: the line is changed by editing).

## Invariants

- **INV-0462-1.** The network connection of a room's terminal reaches exactly the hosts and ports of the room's `+net`
  words, none when the room has none, and a line gets a connection only when it asks for one inside them. *Check:*
  `cargo test -p zoraya-shell --lib netgrant` (the scope of the words, the bound, a line that asks for nothing),
  `cargo test -p zoraya-panel` (which launch is given words), and `make test-apps` (`tests/gui/ssh-room.script`: a
  room with a word logs in, another port of the same host is refused, a room with none is refused).
- **INV-0462-2.** Every terminal's shell has a random connection that is a shell's: it makes programs up to the
  shell's cap, and its programs may `Share`. *Check:* `cargo test -p zoraya-rng` (`a_launchers_shells_are_shells_not_programs`)
  and `make test-apps` (`ssh-keygen` in a room's window makes a key).

## Open questions

- Whether the panel should follow a restart of the network service for the shells it started.
- Whether home's second terminals should get the first terminal's network (the decision is for rooms).
- The room's own bound in the first terminal after `room r` is the shell's; narrowing that shell's connection when it
  enters a room is not built.

## Revisit triggers

- Rooms' words for listening (`+listen`), or a rooms overview form; a second consumer of the launcher's connection.

## Architect's decision
