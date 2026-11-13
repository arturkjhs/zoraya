# ADR-0323: The compositor's clients: 32 slots, an untrusted room's clients counted against its account, the panel's reserve from the session's share

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0093, ADR-0160, ADR-0168, ADR-0172, ADR-0260, ADR-0261, ADR-0262, ADR-0263, ADR-0267, ADR-0294, ADR-0317
- **Evidence:** CP-07, quotas/survey.md#genode-resource-trading, quotas/survey.md#zoraya-today

## Context
The compositor takes at most 8 client connections (`MAX_CLIENTS`, an assumption of ADR-0093), 32 windows of all
clients together (`MAX_WINDOWS`) and 8 toplevels a client (`MAX_TOPLEVELS`). ADR-0262 point 2 carved the windows and
pixels into accounts: an untrusted room may have 3 surfaces and 2 display areas of pixels, four untrusted accounts 12
surfaces and 8 display areas, the session (home, the trusted rooms, the system clients) the rest, at least 20 windows
(`zoraya_compositor::usage`, Q6b, ADR-0294; refusals are `OVER_QUOTA`, status 8). Clients were not in the table: an
untrusted room could hold every one of the 8 slots with clients that have no window, and the session would get no new
terminal. Part 9d gave POSIX programs windows (ADR-0317); the panel counts the clients it made (`OpenWindows`, by tag,
freed by `ClientGone`) and refuses a POSIX program's window while fewer than `RESERVED_CLIENTS` (3) of the 8 slots
would stay free (`docs/backlog.md`: 8 slots are too few for a desktop). Read in this session: what a client costs the
compositor without a window is its `Client` record, a launch record of at most 24 + 16 + 1,024 + 256 bytes of text and
about 100 bytes of fields, and one port subscription (ADR-0299 bounds a port at 1,024); a window costs its copy, up to
a display of pixels (`quotas/survey.md#zoraya-today`).

## Decision drivers
1. Every service bounds what an untrusted room can make it hold, per account, and the bounds of all accounts never pass
   the service's global bound (ADR-0262, INV-0262-1, INV-0262-3; CP-07).
2. Memory: the compositor's worst case is its windows' copies; client records are small. A limit that is not memory
   should not be tighter than the windows it can hold.
3. The desktop's own applications (a terminal, the file manager, settings) keep slots that POSIX programs' windows
   never take (ADR-0317 item 2).
4. No change to the protocol's shape: the refusal is the existing `OVER_QUOTA` (ADR-0263), or `NO_MEMORY` past the
   global bound as today.

## Considered options

### Option A: The Linux way, no limit in the server
- **Description:** An X server or a Wayland compositor takes connections until file descriptors run out (the process's
  `RLIMIT_NOFILE`); nothing is per user or per application.
- **Evidence for:** No constant to choose.
- **Evidence against:** Driver 1: one untrusted room could take every connection the process can hold.
- **Cost:** none in code; the bound is the handle table (1,024).
- **Precedent:** X11 and Wayland servers.

### Option B: Raise `MAX_CLIENTS` only (32), no account for clients
- **Description:** As today with 32 slots.
- **Evidence for:** Smallest change.
- **Evidence against:** Driver 1: an untrusted room may still open clients with no window until the slots are gone;
  its surfaces' account does not see them.
- **Cost:** a constant.
- **Precedent:** Zoraya before this ADR.

### Option C: 32 slots; an untrusted room's clients counted in its account (3 a room, 12 for all); the panel's reserve taken from the session's share
- **Description:** `MAX_CLIENTS` = `MAX_WINDOWS` = 32: a client that has no window is the only one that does not use a
  window, and the bound costs about 48 KiB of records at most (driver 2). The untrusted account gains a third column:
  `ROOM_CLIENTS` = 3 (as many as its surfaces: a client of a room is there for a window), `UNTRUSTED_CLIENTS` = 12
  for the four accounts, so the session keeps `SESSION_CLIENTS` = 20 whatever the rooms do (ADR-0262's "the rest"). A
  room at its bound is refused with `OVER_QUOTA`, counted as the room's refusal, and the panel is told `ClientGone` for
  its tag as for every refused record. The numbers live in `zoraya-proto-gui` (beside `MAX_POPUPS`), so that the panel
  and the compositor read the same ones. The panel admits a POSIX program's window only while the session's clients
  with it leave `RESERVED_CLIENTS` (3) of `SESSION_CLIENTS` free; it counts the session's clients (the two untagged
  ones, its launches outside untrusted rooms, the windows of POSIX programs) apart from an untrusted room's launches.
  `MAX_TOPLEVELS` (8 a client) stays for the session; an untrusted room's client is held to its account's 3 surfaces
  before it (one check, `OVER_QUOTA`).
- **Evidence for:** Drivers 1 to 4; the same shape as ADR-0262's surfaces and pixels.
- **Evidence against:** The session's 20 are not carved: a session that opens 32 clients leaves an untrusted room none,
  as for windows today (ADR-0262 bounds the rooms, not the session).
- **Cost:** *(estimate)* 100 lines with tests in the compositor and the panel.
- **Precedent:** ADR-0262's accounts; Genode's session quotas, where a client pays for its session state (`quotas/survey.md#genode-resource-trading`).

### Option D: A slot table per room, carved in advance
- **Description:** Each room (trusted or not) gets a fixed number of slots at creation.
- **Evidence for:** No room can starve another, the session included.
- **Evidence against:** Rooms are created at run time and their number is not bounded by the compositor; a static
  carve wastes slots of rooms that are idle; the session's rooms are one account by ADR-0262.
- **Cost:** *(estimate)* a table keyed by room in the compositor and the panel.
- **Precedent:** none in Zoraya.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision. As built in part 9e: `zoraya_proto_gui::MAX_CLIENTS`
(32), `ROOM_CLIENTS` (3), `UNTRUSTED_CLIENTS` (12), `SESSION_CLIENTS` (20); the compositor's `MAX_CLIENTS` is the
protocol's; `AddRoomClient` of an untrusted room past its account is `OVER_QUOTA` with `ClientGone` for its tag;
`zoraya_panel::OpenWindows::admit` takes `SESSION_CLIENTS`; a GUI step opens many windows (`windows.script`).

## Consequences
- **Positive.** A desktop of up to 20 session clients (from 6 usable before); an untrusted room's clients are bounded
  like its surfaces; the hover card's and `top`'s surface column is unchanged.
- **Negative.** The compositor's round may read 32 client channels (one message each, as before); the records'
  memory grows from about 12 KiB to about 48 KiB at most.
- **Follow-up.** Show an untrusted room's clients in the readings (`SetUsage`) if a person needs them; carve the
  session's share if a measured workload starves a room.

## Invariants
- **INV-0323-1.** An untrusted room's `AddRoomClient` past `ROOM_CLIENTS` for that room or past `UNTRUSTED_CLIENTS`
  for all is refused `OVER_QUOTA` (its end closed, `ClientGone` for its tag) and changes no other client; the session's
  clients are not refused for the rooms'; the clients of all accounts together never pass `MAX_CLIENTS`. *Check:* `cargo test -p zoraya-compositor` (`tests_usage.rs`).
- **INV-0323-2.** The panel never admits a POSIX program's window that would leave fewer than `RESERVED_CLIENTS` of
  `SESSION_CLIENTS` free, counting the session's clients only. *Check:* `cargo test -p zoraya-panel`.

## Open questions
- **A deviation from ADR-0262's "four accounts", for the architect.** Client accounts never count rooms: the bound is
  `ROOM_CLIENTS` a room and `UNTRUSTED_CLIENTS` (12) for all, so up to 12 untrusted rooms of one client each are
  admitted, while surfaces are held to four accounts (ADR-0262, counted by the rooms that have surfaces, as before).
  The clients of a fifth to twelfth room hold slots of the untrusted share but get no surface until one of the four
  accounts frees (`OVER_QUOTA` at `Create`). Counting accounts from a room's first client (ADR-0262's "when it first
  launches into the room") would refuse the fifth room's terminal itself at `AddRoomClient`; not changed here.
- The compositor keeps a refusal count per room for the readings. A refusal for a room that has no client (one past
  `UNTRUSTED_CLIENTS`) is listed until the next refusal of a room not yet listed or the next client goes; the list is
  never longer than the untrusted clients plus one (as built in part 9e, `count_refusal`). The panel shows such a
  room's refusal only for that while.
- Whether a client with no window should hold its slot for ever (a stuck program): the panel cannot see the program's
  end; the person's close button needs a window. A timeout would be a new decision.

## Revisit triggers
- A measured desktop that needs more than 20 session clients; a compositor heap measured near its limit.

## Architect's decision
