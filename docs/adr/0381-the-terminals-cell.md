# ADR-0381: The terminal's cell is 16 bytes: a character, two combining marks of the Basic Multilingual Plane, ten attribute bits and two colours

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0332
- **Evidence:** alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#utf-8-and-character-width, stage-10/survey.md#what-a-small-terminal-implements

## Context

ADR-0332 item 6 says "a cell grows to 16 bytes `[assumption]`" and item 9 that a width-0 character joins the cell
before it, "at most two per cell, more are dropped". It does not say what a cell holds, which is a memory layout
and so a decision (CLAUDE.md). The grid of 80 × 24 and a scrollback of 2,000 lines (the setting of item 11) make
the cell the largest object the terminal owns: at 16 bytes the primary screen, the alternate screen and 2,000 lines are
about 2.7 MB; at 24 bytes about 4 MB.

**What is known.** A cell needs a character (21 bits), the SGR attributes (bold, faint, italic, underline, blink,
reverse, invisible, strike: 8 bits), the two halves of a wide character (2 bits), and two colours each of a palette
index (8 bits), a direct colour (24 bits) or the default (a kind: 2 bits). Combining marks are almost all in the Basic
Multilingual Plane; the nonspacing characters outside it (variation selectors supplement, a few musical symbols) are
rare in text a person types (`stage-10/survey.md#utf-8-and-character-width`).

**What is assumed.** That a dropped mark outside the plane is not noticed in stage 10; no font draws it anyway.

## Decision drivers

1. **One cell, one machine word pair**: sixteen bytes keep a row aligned and a copy cheap.
2. **Direct colour per cell** (ADR-0332 item 6) without a table that a hostile program could fill.
3. **Combining marks bounded** (ADR-0332 item 9).
4. **A change of layout is local**: the marks, the attribute word and the colours are private to `zoraya-term` behind
   accessors (the character is a public field).

## Considered options

### Option A: Character, two 16-bit marks, ten attribute bits, two three-byte colours

- **Description:** `char` (4), two `u16` marks (4), attributes and the two colour kinds in a `u16` (2), two `[u8; 3]`
  colours (6) = 16 bytes. A mark outside the Basic Multilingual Plane, and a third mark, are dropped.
- **Evidence for:** Drivers 1 to 4; nothing is allocated per cell.
- **Evidence against:** The dropped marks; the colour kind shares the attribute word.
- **Cost:** *(estimate)* As built, 150 lines with tests.
- **Precedent:** `st` keeps one rune and no marks; `alacritty` a character, flags and two colours and an optional
  heap list of marks (`stage-10/survey.md#what-a-small-terminal-implements`).

### Option B: A side table for marks

- **Description:** 12-byte cell with a `u16` index into a table of up to two full code points a cell.
- **Evidence for:** No mark is dropped for being outside the plane.
- **Evidence against:** The table needs freeing, and a hostile program can fill it; a bound and an eviction rule are
  more code than the marks are worth.
- **Cost:** *(estimate)* 300 lines and a bound to argue.
- **Precedent:** alacritty's `Zerowidth`.

### Option C: A 24-byte cell with two 32-bit marks

- **Description:** Option A with full code points.
- **Evidence for:** Nothing is dropped but the third mark.
- **Evidence against:** 50 % more memory in the screen and the scrollback for a case that does not occur.
- **Cost:** *(estimate)* One more word a cell.
- **Precedent:** Linux console, one `u16` a cell and no marks; Plan 9's `rio` draws runes only
  (`alternatives-postmortem.md#plan-9--inferno`).

## Recommendation

**Recommendation (not a decision): Option A.** The cell is private; if a program needs a mark outside the plane,
Option B or C is a local change.

## Consequences

- **Positive.** 16 bytes; nothing per cell on the heap; the scrollback of 2,000 lines of 80 columns is 2.5 MB.
- **Negative.** Non-BMP nonspacing characters are not shown; a third mark is not shown.
- **Follow-up.** `docs/design.md` records the limit.

## Invariants

- **INV-0381-1.** `size_of::<Cell>()` is 16, and every colour kind and attribute bit survives a round trip through the
  accessors. *Check:* `cargo test -p zoraya-term` (`cell::tests`, `memory_is_what_the_terminal_holds`).

## Open questions

- Whether a mark outside the plane appears in a text a person types (then Option B).

## Revisit triggers

- Fonts with such marks are embedded (ADR-0162's follow-up).

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

As recommended.
