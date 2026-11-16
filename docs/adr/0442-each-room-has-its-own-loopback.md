# ADR-0442: Each room has its own loopback (plan part F9): `netd` keys listeners and loopback connects by a room name that only a launcher's connection can give

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0172, ADR-0220, ADR-0262, ADR-0268, ADR-0434, ADR-0440, ADR-0441
- **Evidence:** CP-01, GR-008

## Context

The architect accepted ADR-0441 on 2026-11-16 with one change: **each room has its own loopback**. A connect to 127.0.0.1:PORT
reaches only listeners of the connecting program's room; reaching another room is a later, explicit grant. ADR-0441's
Recommendation said the loopback belongs to the machine and listed the consequence as a condition of the untrusted-room gate
(ADR-0268): a program with a reach rule for 127.0.0.1 could connect to any listener, learn which ports are listened on (a refused
connect against an accepted one) and fill a listener's 64-slot queue. This ADR records how part F9 built the decision, because
the text of ADR-0441's item 4 and its INV-0441-2 ("a port has at most one") no longer describe the build.

What `netd` knew before: an *account* (ADR-0262), not a room. The shell carves one account per POSIX program, so `python3` and
`curl` of one line are two accounts of one room, and an account-keyed loopback would fail ADR-0441's acceptance (`make
test-python-serve`). A room is not a thing `netd` can see: a trusted room is a mode of the shell (`room NAME`), whose one root
connection (from `init`, slot 9) is the same in every room; an untrusted room will have its own launcher (ADR-0269).

## Decision drivers

1. **The room is the unit of the gate** (ADR-0268, ADR-0172): the loopback key must be the room, and two accounts of one room share it.
2. **The key must come from a trusted source** (CP-01, GR-008): a program must not be able to claim another room, by any request,
   including one that makes a connection from its own.
3. **A refusal must say nothing about other rooms**: no error code, port number or counter may differ with what another room does.
4. **The least commitment**: no new service, no kernel change, no new notion of room identity beyond what the shell already has
   (`room NAME`, names unique in `/config/rooms`, at most 24 bytes).

## Considered options

### Option A: a room name on the connection, given only through a right (built)

- **Description:** a new right `ROOM` (16, in `zoraya_abi::net::rights`; `ALL` has it, so `init`'s control connection and the shell's
  root, which are `ALL`, hold it) and a new request `NarrowRoom` (ordinal 9: `Narrow`/`NarrowAccount` with a `room` name of at most 24
  bytes; `budget` 0 joins the caller's account). A connection has a room (empty: `home`). A request that names a room is refused
  `NOT_GRANTED` unless the asker holds `ROOM`; one that names none gives the new connection the asker's room, so `Narrow`,
  `NarrowAccount`, `NarrowRoom` with no name and `Duplicate` all keep it. `ROOM` is a right: it cannot be granted by a connection that
  lacks it (the containment of `Narrow`), and the shell gives no program a right set with it. The shell narrows every program's
  connection from its root with its current room's name. `netd`'s listener table is keyed by (room, port).
- **Evidence for:** drivers 1 to 4. Nothing about a room outside the connection's own state; the program holds no connection that may
  name a room.
- **Evidence against:** the shell is trusted to name its room truthfully: a shell in room `b` that named `a` would put a program in
  `a`'s loopback. That is the shell's own authority (it gives the program every other grant too, ADR-0172's rooms are
  the shell's mode). The launcher of an untrusted room (ADR-0269), which will hold its own connection from `init`, must be given one
  whose room `init` fixed and without `ROOM`; not built, because net grants in an untrusted room are refused today. `ROOM` is a new bit
  in `rights::ALL`: a program whose connection has `ALL` (a test's) has it.
- **Cost:** *(estimate)* small: one request, one field, one right; 200 lines with the tests.
- **Precedent:** ADR-0434's `Duplicate` (a message of the connection's own authority); capabilities that carry a label (seL4 badges).

### Option B: a numeric room id given by `init`

`init` would number rooms; the shell would need `init`'s answer for each room entered, a new channel and a state `netd` and `init` share.
Same trust as A with more parts. Not built.

### Option C: the loopback per account (an ACL by account, ADR-0441's option b)

Fails the acceptance (`python3` and `curl` of one line are two accounts) unless the shell lists accounts in each listener: a grant
per pair, more state in `netd`, and it is not what the architect decided.

### Option D: the baseline, machine-wide (what ADR-0441 recommended)

The three leaks above; replaced by the architect's decision.

## Recommendation

**Recommendation (not a decision): Option A, as built.**

1. **The key.** `netd` keeps a room name per connection. The session's own connections are in room `home` (empty). A listener is in
   the room of the connection that listened.
2. **Listeners.** `bind`/`listen` of a port in use is `BUSY` (`EADDRINUSE`) only if the port is in use *in the asker's room*; another
   room's listener on the port is invisible and does not collide. A `listen` of port 0 picks a port no listener of the room has.
3. **Connects.** A `connect` to 127.0.0.0/8 that the scope allows (ADR-0441 item 1 stands: a reach rule that names the address) is handed to
   the listener of *its own room* on that port. With none, the answer is `Failed { REFUSED }` with every handle closed, the answer of no
   listener at all, whether or not another room listens on the port: the same status, the same made-up first port, the same moment.
   The made-up port of a loopback client is a counter *per listener*, so a listener learns nothing of other rooms' connections.
4. **Bounds.** Unchanged and machine-wide: 8 listeners (the sum of the accounts' limits, a partition of the machine), one per carved
   account whose rights have `listen`, 64 pending connections per listener. They are not per room: a room's account is carved
   from the session's and the sum of the accounts' limits is what bounds the machine (INV-0262-3). One room using the session's
   listeners can leave another's `listen` `OVER_QUOTA`; a connect never reveals it (only the program that listens sees its own account's
   answer). Per-room counts need the room-keeper's accounting (ADR-0269) and are not built.
5. **A room's end.** A listener ends with its channel (ADR-0441 item 2): when the programs of a room are gone, their listener channels'
   ends close and `netd` frees the port and the charge. `netd` has no other notion of a room's end, so the connections and listeners
   of a room that outlives its programs (a shell in a room) are just those of its live programs.
6. **Names.** A name that resolves to 127.0.0.0/8 stays `UNREACHABLE` (INV-0441-3): the room key does not change the resolver.
7. **What is not built.** A grant to reach another room's loopback (the architect: later, explicit); the room of an untrusted room's
   launcher (ADR-0269); a per-room listener count.

### Effect of the vision

A program cannot widen its loopback: it holds no connection that may name a room, so it cannot move into another room's; what it
learns of another room is nothing (CP-01, GR-008). The untrusted-room gate of ADR-0268 loses the loopback condition it had from ADR-0441:
a room's `+net 127.0.0.1` reaches that room's own listeners only.

## Consequences

- **Positive.** The gate condition of ADR-0441 is met; `python3 -m http.server` and `curl` of one line still meet (same room); a room
  may use any port; no leak of ports, errors or counters across rooms.
- **Negative.** `rights::ALL` has a new bit (a program given `ALL` can name rooms: none is); the wire has a new request; one room can
  exhaust the machine's 8 listeners for another's `listen` (a denial of service of the grant, not a leak).
- **Follow-up.** The explicit cross-room grant; per-room listener bounds with the room keeper; the untrusted room's connection from
  `init` with its room fixed.

## Invariants

- **INV-0442-1.** A `connect` to 127.0.0.0/8 reaches only a listener of the connecting connection's room; a `bind` or `listen` finds a port
  in use only if a listener of its own room has it. *Check:* `cargo test -p zoraya-net` (`a_connect_reaches_only_the_listeners_of_its_own_room`,
  `accounts_of_one_room_share_its_loopback`) and `make test-net-posix` (`tcptest --serve`/`--probe` in rooms `ra`, `rb`, `rc` and `home`).
- **INV-0442-2.** A connection that does not hold the right `ROOM` cannot name a room, and no connection can give a right it lacks; every
  connection made from one (`Narrow`, `NarrowAccount`, `NarrowRoom` without a name, `Duplicate`) is in its room. *Check:* `cargo test -p zoraya-net`
  (`only_a_connection_with_the_room_right_names_a_room_and_it_is_inherited`).
- **INV-0442-3.** A refusal is indistinguishable from no listener, and nothing a listener sees (its clients' made-up ports) depends on another
  room. *Check:* `cargo test -p zoraya-net` (`a_connect_reaches_only_the_listeners_of_its_own_room` compares the whole answer; `a_room_does_not_see_the_peer_ports_of_another`)
  and `make test-net-posix` (70 refused connects, past the 64 a queue holds).
- **INV-0442-4.** The shell narrows each program's connection into its current room and into no other. *Check:* `make test-net-posix` (the probes of
  each room reach the room's own listener with the same `+net` word); by reading `user/src/bin/zosh.rs` (`net_room`) for the call sites.

## Open questions

- The explicit grant that lets one room reach another's loopback (a word, a room-to-room rule), and whether it is per port.
- Whether the 8 listeners should be counted per room once the room keeper's accounting reaches `netd`.
- A shell that is compromised names any room: in an untrusted room's launcher (ADR-0269) the connection `init` makes must have its room
  fixed and no `ROOM`; check when that launcher exists.

- A room is known to `netd` only by its name: a room removed without `--kill` goes on running, and a new room of the same name would share
  its loopback (both ways). Refuse the name while the old room runs, or add a generation from the keeper.
- The made-up port search of `listen` 0 starts at the range's start for every room (no cursor shared across rooms, so no room sees another's listens).
- A native command's connection (the shell's `narrow_room` with budget 0) joins the session account in every room; its room is not covered by a QEMU test.

## Revisit triggers

- The untrusted room's launcher is built (ADR-0269).
- A part that needs one room to serve another (a browser room and a server room).
- `netd` serves the NIC's address to listeners (ADR-0441 Option B): the room key of a listener on the NIC.

## Architect's decision

(Left empty for the architect.)
