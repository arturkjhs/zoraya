# ADR-0327: Font files under the SIL OFL 1.1 as data next to the system's code and its GPL ports

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0109, ADR-0162, ADR-0326
- **Evidence:** CP-27, desktop/survey.md#candidate-fonts-and-their-licences, desktop/survey.md#the-ofl-next-to-bsd-code

## Context
`CLAUDE.md`'s licence rule lists the code licences a dependency may have (MIT, Apache-2.0, BSD, ISC, Zlib) and one
licence for a data file that is not code (`CDLA-Permissive-2.0`, for the TLS root store of ADR-0228). The system's
fonts are data under the SIL Open Font License 1.1: Inter 4.1 and JetBrains Mono 2.304, unmodified, in `fonts/` with
their licence texts, accepted for the desktop by ADR-0162 and listed with their hashes in `docs/dependencies.md`. Part
9f-text (ADR-0326) makes NetSurf, a GPL-2.0 program (ADR-0318), read the same files from `/system/fonts` at run time.
The architect asked (2026-11-14) that a font that is not permissively licensed have a rule of the architect's before it
is used, and that the least committing option be taken until then.

What is known (`desktop/survey.md#the-ofl-next-to-bsd-code`): the OFL lets the fonts be bundled with any software,
does not change the software's licence, forbids selling the fonts by themselves, and makes a modified font (a subset
included) a Modified Version that stays under the OFL; none of the system's fonts declares a reserved name. Of the
fonts measured for ADR-0162 (`desktop/survey.md#candidate-fonts-and-their-licences`), every one with the Cyrillic
the architect requires is under the OFL or the Bitstream Vera licence, which has the same "not sold by itself" clause:
no font there is under one of the code licences of the rule.

## Decision drivers
1. Cyrillic, including the Ukrainian letters, is required in NetSurf (ADR-0326).
2. The rule must stay narrow: a licence named for data must not open a path for code (CLAUDE.md, CP-27).
3. Least committing: nothing new enters the repository or the image's licences.

## Considered options

### Option A: name OFL-1.1 in the rule for unmodified font files
- **Description:** `CLAUDE.md`'s data clause gains "an unmodified font file may be OFL-1.1, with its licence text";
  code never; a subset or a changed font is not allowed.
- **Evidence for:** matches what ADR-0162 already accepted and `docs/dependencies.md` lists; the OFL's terms fit an
  unmodified data file next to BSD and GPL code.
- **Evidence against:** a second data licence in the rule.
- **Cost:** a line of `CLAUDE.md` (the architect's).
- **Precedent:** Debian ships OFL fonts in main as free; Fedora likewise `[outside evidence: their licence lists]`.

### Option B: replace the fonts by ones under a code licence
- **Description:** find fonts under MIT or Apache-2.0 with the required Cyrillic.
- **Evidence for:** no new licence.
- **Evidence against:** none of the measured candidates qualifies; a new font changes the desktop's look, which
  ADR-0161 and ADR-0162 fixed.
- **Cost:** *(estimate)* a survey and a redesign.
- **Precedent:** none found.

### Option C: keep the fonts out of NetSurf (ADR-0318's bitmap font)
- **Description:** no OFL data reaches the GPL program.
- **Evidence for:** no licence question.
- **Evidence against:** contradicts the architect's decision in ADR-0326.
- **Cost:** none.
- **Precedent:** ADR-0318 as accepted.

## Recommendation
Recommended: **Option A**. It is a recommendation, not a decision.

Until the architect rules, part 9f-text takes the least committing path: it adds no font file and no licence; NetSurf
reads the files the system already ships under ADR-0162, unmodified, at run time from `/system/fonts`; nothing of a
font is compiled or linked into `netsurf` or into `libc.a`; the image's copy carries the licence texts beside the
fonts.

## Consequences
- **Positive.** The rule says what is already the case for the desktop's fonts.
- **Negative.** None beyond a longer rule.
- **Follow-up.** The architect's line in `CLAUDE.md` if accepted.

## Invariants
- **INV-0327-1.** No font file is compiled or linked into a program built from a port; fonts reach POSIX programs only as files of `/system/fonts`. *Check:* `make check-tokens` (no `include_bytes!` of a `.ttf` outside `zoraya-fonts`, INV-0240-1) and a check at the end of `make netsurf` that no `.ttf` file is an input of its link (to be written if this ADR is accepted).

## Open questions
- Whether the architect wants the rule to cover the Bitstream Vera licence too (DejaVu), for a later direction.

## Revisit triggers
- A font that is not unmodified, or a font compiled into a program, is proposed.

## Architect's decision
