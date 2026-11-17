# ADR-0435: A program's connection to the random service may make eight others (plan part F4)

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0227, ADR-0288, ADR-0310, ADR-0411, ADR-0433
- **Evidence:** CP-05, GR-005

## Context

ADR-0288 item 6 lets only `init`'s connection and the shell's make others (64 live children each), so that a program cannot use up
the service's 256 connections. A POSIX program holds a connection one level below the shell's, and `POSIX_SPAWN_GRANT_RANDOM`
(ADR-0411 item 1) needs it to make one for each child. The accepted rule makes that impossible: a browser's UI process could not
give its helpers randomness. **Measured:** `cargo test -p zoraya-rng`.

## Decision drivers

1. **A child gets the connections its parent names** (CP-05, GR-005): a program that holds randomness can pass it on.
2. **A program cannot use up the table** (ADR-0288's reason): the bound has to stay small.

## Considered options

### Option A: a program's connection may make `PROGRAM_CHILDREN` (8) live ones, and those make none

Depth 2 makes up to 8, depth 3 none; the shell's and `init`'s are as before.

### Option B: `GRANT_RANDOM` gives the parent's own connection (a move)

No change to the service; a program with two helpers cannot give both randomness.

### Option C: the broker holds a connection and gives one to every POSIX child (ADR-0453's route)

A change in a second place and a decision about which program gets one; not part of the foundations.

## Recommendation

**Recommendation (not a decision): Option A.** The number is the smallest that serves a launcher with a network process, a few
Web processes and a utility process, and is below the 64 a shell may make by a factor that keeps 256 connections enough for 32
programs that all use their share. It is a constant (`PROGRAM_CHILDREN`) beside `MAX_CHILDREN`.

## Consequences

- **Positive.** `GRANT_RANDOM` works for a program that was given randomness; ADR-0288's protection stands for the table as a
  whole at one-eighth of the old bound per program.
- **Negative.** A program can hold 8 connections that it does not use; the table of 256 can then be filled by 32 programs.

## Invariants

- **INV-0435-1.** A connection at depth 2 makes at most 8 live others and one at depth 3 none; a connection that closes frees
  a place; a refused request counts nothing. *Check:* `cargo test -p zoraya-rng` (`only_the_first_three_levels_share_and_children_are_counted`).

## Open questions

- Whether 8 is enough for the engine's Web processes (WebKit starts one per page group).

## Revisit triggers

- A launcher with more than 8 children that all need randomness.

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

`rngd`'s connection limit of 8 is accepted, superseding ADR-0288 item 6 for that limit.
