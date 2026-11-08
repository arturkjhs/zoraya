# ADR-0242: The panel beside `init` in stage 7d

- **Status:** proposed
- **Date:** 2026-11-08
- **Depends on:** ADR-0093, ADR-0097, ADR-0155, ADR-0160, ADR-0168, ADR-0172
- **Evidence:** CP-17, alternatives-postmortem.md#genode, desktop/survey.md#haiku, desktop/survey.md#plan-9s-rio

## Context
ADR-0168 (accepted) makes the panel the program that starts applications: `init` starts the compositor, the chooser
and the panel, and the panel starts every terminal (`term` with `zosh`) and application, so that `init`'s GUI table
shrinks and its restart rules for `term` and `zosh` move to the panel (ADR-0155's new shell after `exit` included).
It gives the panel a second manager endpoint, the *client manager*, carrying `AddClient`, `ApplySettings` and the
layout notification, while `init` keeps the driver manager. ADR-0172 (accepted) has the panel tell the compositor the
room and the launch record with every client. The mockups' README (which TASK-24 says wins where it differs from
`plan.md`) has the compositor draw the whole dock "from the panel's launch records and ADR-0172's room table", as 7b
already draws the top bar.

Stage 5's `init` starts the GUI terminal and its shell together with the storage stack, the POSIX session or the
broker beside the shell, and its restart and power-off rules (ADR-0097, ADR-0104, ADR-0155); the GUI tests and the
POSIX tour in a window (`test-posix-gui`, `test-tour-gui`) depend on that shell being `init`'s. Moving all of it into
the panel in one step moves the session, the broker and the power handle with it. Measured: nothing; the stage 5 and
7a to 7c GUI scripts pass with `init`'s terminal (`make test` on `work/7c`).

## Decision drivers
1. Every step of stage 7 keeps `make test` green, the POSIX session in a window included (CP-17: each piece restartable
   on its own).
2. ADR-0168's model where it costs nothing to follow: the panel is a separate program with its own manager endpoint,
   it launches every application it launches with exactly its launch record (INV-0168-1), and the compositor learns
   the record with the client (ADR-0172 point 4).
3. No new authority: the panel holds only what ADR-0168 gives it (the client manager, the volume's root, the console
   to write, its job, the image directory); the power handle stays with `init`'s shell.

## Considered options

### Option A: Everything at once, as ADR-0168 says
- **Description:** `init` starts the panel; the panel starts the first terminal and shell, the POSIX session or the
  broker beside it, and restarts the shell after `exit`.
- **Evidence for:** ADR-0168 as written; one launcher.
- **Evidence against:** The session, the broker, the power-off rule and the restart counts move into a new program in
  the same step as rooms and the dock; the GUI tests of stages 5 to 7c change with it (driver 1).
- **Cost:** *(estimate)* 600 lines out of `init`, into the panel, and every GUI and POSIX-GUI script retested.
- **Precedent:** Genode's launchers own their children from the start (`alternatives-postmortem.md#genode`).

### Option B: `init` keeps the first terminal; the panel launches the rest
- **Description:** `init` starts the panel once per GUI start, with the client manager (`AttachClientManager` on the
  driver manager), a clone of the root, the console, its job and the image directory. `init` keeps starting the first
  terminal and its shell with its rules, and tells the compositor that terminal's record (`home`, `term`, `+w /`) with
  `AddRoomClient` on the driver manager. The panel tells the compositor the room table and the dock, and launches every
  other terminal into the current room (System+Enter, the dock's icon).
- **Evidence for:** Drivers 1 to 3; the panel's code is the code ADR-0168 asks for, and the first terminal is moved
  into it later without changing the panel's protocol.
- **Evidence against:** Two launchers for one kind of program until the move; `AddRoomClient` is accepted on both
  manager endpoints, so the driver manager is not only the drivers' (a stated difference from ADR-0168's split).
- **Cost:** *(estimate)* 250 lines (the panel, its crate, `init`'s start of it).
- **Precedent:** Haiku's Deskbar launches applications while the system's `launch_daemon` keeps its own
  (`desktop/survey.md#haiku`).

### Option C: The shell launches terminals (rio's way)
- **Description:** A builtin in `zosh` starts another `term` with a shell; no panel until 7e.
- **Evidence for:** Smallest; the window system as launcher (`desktop/survey.md#plan-9s-rio`).
- **Evidence against:** The dock, the rooms' chips and the launch records have no owner; INV-0168-1 has no launcher to
  check; ADR-0168 and ADR-0172 are not followed.
- **Cost:** *(estimate)* 100 lines, thrown away in 7e.
- **Precedent:** Plan 9.

### Option D: Unix's baseline: a desktop session manager forks the panel and the first terminal alike
- **Description:** A session script starts the panel and a terminal as peers, both with the session's full authority.
- **Evidence for:** What X11 sessions do.
- **Evidence against:** Ambient authority for both (driver 3); nothing ties a window to a launch record.
- **Cost:** *(estimate)* Small.
- **Precedent:** `xinit`, GNOME's session manager.

## Recommendation
Recommended: **Option B**, as built in stage 7d. It is a recommendation, not a decision. The dock is drawn by the
compositor as the README says (ADR-0168's "two bar windows" become the compositor's own surfaces, as 7b's bar is), so
INV-0168-3 holds by construction. The first terminal moves into the panel in the task that builds ADR-0155's restart
in the panel; until then a terminal the panel started ends with its shell (the window closes after `exit`), and the
panel's room job per room (ADR-0172 point 2) is a job per launched program under the panel's job.

## Consequences
- **Positive.** Rooms, the dock and launch records are real in stage 7d with every earlier test unchanged.
- **Negative.** Two launchers; the driver manager accepts one client-side message.
- **Follow-up.** Move the first terminal, the session and ADR-0155's restart into the panel; one job per room.

## Invariants
- **INV-0242-1.** `SetRooms` and `SetDock` are accepted only on the client manager, the drivers' messages only on the
  driver manager, `AddRoomClient` on either manager, and none of them on a client's channel. *Check:* host test of the
  compositor sending each manager ordinal on each endpoint.

## Open questions
- Whether the first terminal should move into the panel with the session (Option A's work) or the session stay with
  `init` and only the terminal move.

## Revisit triggers
- The task that builds ADR-0155's restart in the panel, or the chooser (7e) needing the panel to own every client.

## Architect's decision
