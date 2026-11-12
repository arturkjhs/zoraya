# ADR-0294: Q6b, what the person sees where ADR-0262 and ADR-0267 are silent

- **Status:** accepted
- **Date:** 2026-11-12
- **Depends on:** ADR-0153, ADR-0169, ADR-0172, ADR-0262, ADR-0263, ADR-0266, ADR-0267, ADR-0269, ADR-0293
- **Evidence:** CP-23, DM-034, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#lessons-for-zoraya

## Context
The survey's lesson is to say which limit was hit, not only that one was (`quotas/survey.md#lessons-for-zoraya`).
ADR-0267 says `top` is a native program with a keeper channel in a designated slot, that the desktop shows a card and
pills from the panel's once-a-second `Usage`, that a notice carries a `Raise` action, that the rooms overview has a card
per room, and that the settings window sends `Set` over a `Manage` channel the panel gives it. ADR-0262 says the
compositor keeps an account per untrusted room and refuses at its bound with `OverQuota`. Building these (Q6b) needed
answers to things they leave open. Each answer below is the simplest, least surprising and most reversible one; none
changes an accepted decision except where the last section says so.

## Decision drivers
1. One reading renders everywhere: `top`, `room`, `grants`, the pills and the card must not be able to disagree
   (INV-0266-3, INV-0267-1, CP-23).
2. No new authority: no application gets a `Manage` channel or `LIMIT` it does not hold (ADR-0269, INV-0269-2).
3. Calm by default (ADR-0267 driver 5; DM-034 has no pressure signal): nothing moves until a limit is near.
4. Nothing a client sends reaches what the compositor draws of rooms (INV-0172-1).

## Considered options

### Option A: the smallest additions, built
- **Description.**
  1. **`top` is a builtin of `zosh`**, not a program given a slot. The shell already holds the keeper channel (`Manage`
     on the console, `Room(R)` in a panel-launched room), so a builtin needs no new handle-passing rule (ADR-0153
     rule 8 would need one for a designated keeper slot) and no new `Usage` channel per command. It prints what the ADR's
     `top` prints: a row per room that runs, `USED of LIMIT`, the refusal trailer, and a `session` row that sums the rooms
     (the keeper has no session ceiling to show). `top ROOM` prints the room's row, its `limits` line and the line that
     raises the limit that refused; the job tree of the ADR's example needs a kernel call that lists a job's children,
     which does not exist. `top --session` is not built. `--watch` reads every second until Ctrl-C.
  2. **One reading.** `zoraya_settings::usage::Reading` is the keeper's record in plain numbers; `top_table`, `room_words`,
     `Reading::card_lines`, `Reading::limits_line` (the text of `grants`) and `Reading::pills` are the only places any of the
     texts is made. The panel sends the compositor the readings in `Reading::to_wire` text (`SetUsage`, request 44 of the
     client manager's channel); the compositor checks every line again and takes all or none; it makes the pill and the
     card from them with the same functions.
  3. **Pills at 90%** (`NEAR_PERCENT`) for memory, processes and CPU; they come before the grants in the title bar, so
     a narrow window folds the grants into `+N`, never the warning. `home` and rooms without a limit have none. The dock
     chip and the badge carry no pill yet.
  4. **The hover card** is shown while the pointer is on a framed window's title bar, for a window of a room (not `home`):
     the room, its grants as the record has them, then the reading's lines. It is drawn by the compositor, not a client
     surface, above the windows and the dock and under the bar, and it takes no input (a card you cannot press cannot be
     spoofed into a press). It lies outside the window's outer rectangle, above it or else below it, where no pixel of
     the client's buffer is, so a buffer that paints a copy of a card stays a picture in its own body (ADR-0172 point
     1, INV-0172-1). A window with no free side (one as large as the work area) gets the card starting on its title
     bar, which no client draws, so a copy lacks that part. The card is worked out again after every restack, close and
     new window, from the pointer's last position, and its rectangle is damaged before and after whatever moves it (a drag,
     a tiling, a hide, a reading that changes its lines): the compositor remembers the rectangle it last drew. A hidden
     window has no card, and the card is kept on the screen, also for a window at the left limit. In the no-free-side case
     (every tiled or full-height window, so the usual case for tiled windows) the card covers the right of the title bar,
     close and minimise buttons and the client-chosen title included, until the pointer moves off the title bar or reaches
     a button; that is a limitation of the design, not an oversight.
  5. **The notice has no `Raise` button.** The card and `top ROOM` carry the notice's text: which limit refused, how
     often, and `raise: room set NAME --mem 128M` (twice the limit, the words of the shell's hint). A button needs an
     interactive trusted surface, which ADR-0172 has only for the chooser's sheet; it waits for the rooms overview.
  6. **The rooms overview is not built** (it is not built in stage 7 either: `docs/design.md`, "Not built in 7d"). Its
     usage lines are the card's; they come with it.
  7. **The compositor's accounts** (ADR-0262 point 2): an untrusted room is an account of 3 surfaces and 2 display areas
     of buffer pixels; all untrusted rooms together 12 surfaces and 8 display areas, in at most 4 accounts; the rest
     (`MAX_WINDOWS` 32 less 12) stays for `home` and the trusted rooms. A client is in the account of its record's room if
     the record is untrusted. `CreateToplevel`, `CreatePopup` and `SetBuffer` past a bound answer the new status
     `OVER_QUOTA` (8, `Status` is `Below<9>`), count the refusal for the room and change nothing else. The count is readable
     (`Compositor::accounts`) but does not reach the room keeper yet, and it is forgotten when the room's last client
     is gone. A `SetBuffer` buffer waiting for its first commit is mapped, so the account holds it (a new buffer at the
     size the window has is one more area until the commit; a newer one replaces the pending one). The totals over all
     accounts (12 surfaces, 8 areas) can never bind, since 4 accounts of 3 surfaces and 2 areas reach them only at the
     per-account bounds; they state ADR-0262's promise and guard a change of the three constants, and a mutant of those
     two terms is equivalent. The bound on the number of accounts does bind (four rooms with one surface each).
  8. **The settings window's Rooms section** edits through the panel, not the keeper. The window holds `/config` narrowed
     to `settings` (INV-0169-4) and no `Manage` channel; giving it either would widen a client. It asks the panel on the
     channel it already has (`RoomsQuery`, `SetRoomLimit`), and the panel, which holds `Manage` and the volume, does what
     `room set` does: `rooms::with_limit` (the shell's checks), write the line, `Set` to the keeper (which checks the
     ceilings), put the old line back if refused. A settings window launched in an untrusted room is refused: such a
     room holds no `Set` (ADR-0266 point 6). The page shows a field per word, the use beside it, the `room set` line of
     what is being edited, and one Apply. The launch room of the settings window matters only when it is untrusted:
     ADR-0269 gives the settings app `Manage`, so a window launched in `home` or a trusted room may set limits and one
     launched in an untrusted room may not. The handler is `zoraya_panel::set_room_limit`, with the file and the keeper
     as closures, so that the refusal is tested on the host.
- **Evidence for:** Drivers 1 to 4; each rule has a host test; `top` and `room` show the same numbers as the card.
- **Evidence against:** `top` is not a program (a different program cannot run it); the notice is text, not a button;
  the compositor's refusal count stays in the compositor.
- **Cost:** *(estimate)* as built.
- **Precedent:** `systemd-cgtop` reads the same group counters as the unit files it sits beside
  (`quotas/survey.md#linux-cgroups-v2`); the ADR's own examples.

### Option B: `top` as the ADR says, a program with a keeper slot
- **Description:** `top` is a command file; the shell passes a `Usage` channel from the keeper's `Open` as a designated
  slot, for every run.
- **Evidence for:** Any program can be `top`; ADR-0267 says so.
- **Evidence against:** A new designation rule in the shell, a channel per run, and the shell's own `room` and `grants`
  still need the keeper channel: two readers of the same data.
- **Cost:** *(estimate)* 300 more lines in `zosh` and `user/src`.
- **Precedent:** the ADR.

### Option C: the settings window holds a `Manage` channel
- **Description:** As ADR-0267 point 4 reads: the panel gives the settings window a `Manage` channel and it sends `Set`.
- **Evidence for:** No message between the window and the panel.
- **Evidence against:** An application with launch authority over every room, to change a limit; a bug in the window is
  then a bug in the room keeper's trust (INV-0269-2).
- **Cost:** *(estimate)* less code in the panel, more review.
- **Precedent:** none in the survey.

## Recommendation
**Recommendation (not a decision): Option A.** It keeps every accepted decision's effect and takes the cheapest path to a
visible result. Two points differ from the text of ADR-0267 and are the architect's to accept or send back: `top` as a
builtin (point 1) and the settings window's edit through the panel (point 8).

## Consequences
- **Positive.** A person sees a room's use in the shell and on the desktop in the same words; no application gained
  authority; the compositor bounds an untrusted room's windows.
- **Negative.** The reading's disk and connection use are not reported (no launcher forwards the services' counts yet): the
  columns show the limit only. A card cannot be pressed. The panel reads usage once a second and waits up to 5 s for a
  keeper that does not answer.
- **Limitations.** (a) Apply sends one `SetRoomLimit` per changed word, up to 32 rooms by 6 words, each a call of its
  own that may wait up to 5 s for the keeper; the settings window stops sending after the first answer that says the
  panel or the keeper does not answer, but a transaction (all words of a room in one request, with a check) is not
  built. (b) The panel's read-modify-write of `/config/rooms` has no lock: it is the file's one writer while it handles
  a request, but a person editing the file by hand at the same moment can lose an edit (the rollback re-reads the file
  to keep other edits). (c) `cpu_share` is the share of one CPU while `cpu_permille` is the share of the machine; they
  agree only while user threads run on one CPU, which is true today and must be revisited with SMP.
- **Follow-up.** The rooms overview with `Raise`; a kernel call listing a job's children for `top ROOM`; the compositor
  forwarding its refusal count to the keeper.

## Invariants
- **INV-0294-1.** A window request of an untrusted room's account that passes a bound is refused with `OverQuota`, and
  nothing of any other account changes; the untrusted accounts together leave `home` and the trusted rooms at least 20
  of the compositor's 32 windows. *Check:* `cargo test -p zoraya-compositor usage` (the three account tests).
- **INV-0294-2.** The compositor takes the rooms' use only from the client manager's channel, whole or not at all, and a
  pill shows only at `NEAR_PERCENT` or more. *Check:* `cargo test -p zoraya-compositor usage` and
  `tests/gui/limits.script` in `make test-gui`.
- **INV-0294-3.** The settings window's `SetRoomLimit` is refused when the window was launched in an untrusted room, and
  otherwise does what `room set` does. *Check:* `cargo test -p zoraya-panel` (`an_untrusted_origin_sets_no_limit` and
  the handler's other tests) and host tests of `zoraya_settings::rooms::with_limit` for the words and values.

## Open questions
- Whether the pill should also mark the dock chip and the badge (ADR-0263 point 7 says the badge and dock chip).
- Whether disk use should come from `fsd`'s account through the same `Report`.
- Whether `top` as a builtin is acceptable beside ADR-0267's "native program".

## Revisit triggers
- A second reader of the keeper's usage that is not the shell or the panel (an agent supervisor).
- The rooms overview.

## Architect's decision

Accepted 2026-11-12. Decided by the architect; recorded by an agent at the architect's request.
