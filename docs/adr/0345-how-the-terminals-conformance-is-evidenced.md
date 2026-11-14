# ADR-0345: How the terminal's conformance is evidenced: a test calls the entry's capability, recordings are made on the host, and a second emulator is a text check

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0332, ADR-0333
- **Evidence:** alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#terminfo-entries-compared, stage-10/survey.md#testing-a-terminal

## Context

ADR-0332 item 10 and INV-0332-1 say that every capability of the pinned entry has a host test of `zoraya-term`
or a line with a reason in an exceptions file, and that a tool fails on a capability with neither. It does not say how
a test is recognised as covering a capability, nor how the recordings are made, nor how the first grids are
compared with xterm when none is installed on the machine (the Mac mini has no xterm). Those are decisions about the
test method.

**What is known.** The entry has 287 capabilities (`stage-10/survey.md#terminfo-entries-compared`); testing
only a name in a comment would let a comment outlive its test. Terminal emulators test against recorded sessions
(`stage-10/survey.md#testing-a-terminal`).

**What is assumed.** That a recording of a host program made once is a fair input; that tmux, which parses the same
bytes with its own parser, is a useful second opinion on text though it is not xterm.

## Decision drivers

1. **A cover that cannot go stale**: a test that stops calling the capability stops covering it.
2. **Tests spell what the entry says**, not what their author remembers it says.
3. **Offline** (`make check` and `cargo test` need no network and no host terminfo).
4. **A person's comparison with xterm stays**, and a tool's does not replace it (ADR-0332 item 10).

## Considered options

### Option A: A comment or an attribute naming the capability

- **Description:** `// covers: cup home` above a test; the tool greps.
- **Evidence for:** The simplest tool; what many projects do.
- **Evidence against:** A comment is not run: the test may not touch the capability. Driver 1 fails.
- **Cost:** *(estimate)* A script.
- **Precedent:** Linux selftests' lists of covered names.

### Option B: The test calls the capability from a dump of the entry

- **Description:** `entry/xterm-256color.caps` is `infocmp -x -1` of the file the pinned `tic` compiles; a test module
  expands a capability's string with its parameters (`cap("cup", &[4, 9])`), reads a flag (`flag("am")`) or a number
  (`num("colors")`), and fails for a name the dump lacks. The tool counts the calls, and compares the dump with the
  pinned entry in `make check-ports`.
- **Evidence for:** Drivers 1 to 3: the bytes fed to the terminal are the entry's, and a renamed capability breaks the
  test.
- **Evidence against:** A test could call `cap` and assert nothing; review and mutation testing catch that, the
  tool cannot.
- **Cost:** *(estimate)* 250 lines of test support (a terminfo parameter expander) and a 60-line script.
- **Precedent:** none found; ncurses' own `tic` has a check for malformed strings, not for coverage.

### Option C: Generate the tests from the entry

- **Description:** A build step writes a test for every capability from its string and an expected-effect table.
- **Evidence for:** Complete by construction.
- **Evidence against:** The expected effect is still written by a person, in a table; the generated test hides what
  it checks.
- **Cost:** *(estimate)* A generator and a table of 287 rows.
- **Precedent:** Plan 9 tested `rio` by use (`alternatives-postmortem.md#plan-9--inferno`).

## Recommendation

**Recommendation (not a decision): Option B.** For the recordings: `tools/term-record/record.py` runs a program on a
pseudo-terminal on the host with `TERM=xterm-256color`, answers nothing, and writes the bytes with marks and a
session script; `vi` and `less` are the host's, the ncurses programs are built from the pinned archive. The grids
`zoraya-term` produces at the marks are frozen in `.grids` files and replayed offline. As the second emulator,
`compare-tmux.py` compares their text with tmux's capture of the same bytes; it is run by hand and not part of
`make test`, and a difference is read, not assumed to be a fault of either side. A person's comparison with xterm
stays the first-comparison gate of ADR-0332 item 10.

## Consequences

- **Positive.** A capability's cover is an executable call; the recordings are reviewable text.
- **Negative.** Recordings of the host's `vi` and `less` carry that host's versions in their bytes; a re-recording
  changes them (the grids are frozen, so the diff is reviewed).
- **Follow-up.** Part 10b removes the key and mouse capabilities from the exceptions as it tests them.

## Invariants

- **INV-0345-1.** Every capability of the dump is called by a test or listed in the exceptions with a reason, and no
  capability is both. *Check:* `tools/term-conformance/check.sh` in `make check`.
- **INV-0345-2.** The dump is the output of the pinned `infocmp` on the pinned `tic`'s compilation of `xterm-256color`.
  *Check:* `tools/term-conformance/check.sh pin` in `make check-ports`.

## Open questions

- Whether CI should run `compare-tmux.py` (tmux is on the runners; its version is not pinned).
- A reference emulator that is xterm itself, when one is installed on a machine of the architect's.

## Revisit triggers

- A capability that a test covers by calling `cap` and asserting nothing.
- The pin moves (ADR-0333): the dump changes and the tool lists what is new.

## Architect's decision
