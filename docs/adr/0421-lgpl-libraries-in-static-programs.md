# ADR-0421: LGPL libraries in a ported program: allowed when the program's whole build is in `ports/` and can be relinked from it

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0109, ADR-0410, ADR-0420
- **Evidence:** browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#the-window-text-and-the-network, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WPE requires GLib (LGPL-2.1 or later) and libsoup 3 (LGPL-2); WebKit itself is "a mix of LGPLv2 and BSD licenses"; on
`main` OpenSSL replaces libgcrypt (LGPL-2.1), and glib-networking (ADR-0420) is LGPL
(browser-engine/survey.md#wpe-webkit-in-detail). Zoraya links every program statically and has no shared libraries.
`CLAUDE.md`'s "permissive only" rule is for crates of the kernel and `no_std` crates; ports are listed in
`docs/dependencies.md`, GPL programs and the LGPL TinyCC ship as programs (ADR-0109), and no rule covers an LGPL library
linked into a program (browser-engine/survey.md#the-window-text-and-the-network). The LGPL lets a library be linked
into a work under other terms if the user can relink the work with a modified library `[outside evidence: LGPL-2.1
section 6, not fetched by the survey]`.

## Decision drivers

1. **No code of a different licence in the kernel or the native `no_std` crates** (unchanged).
2. **Ports are built from fetched sources by `ports/Makefile`, never committed** (ADR-0109), so a person who has the
   repository can rebuild any program from source.
3. **The chosen engine needs them**: rejecting LGPL libraries rejects WPE (ADR-0410).

## Considered options

### Option A: Permissive licences only for anything linked into a program

- **Description:** Extend `CLAUDE.md`'s rule to ports' libraries.
- **Evidence for:** The simplest rule.
- **Evidence against:** No WPE (driver 3); NetSurf (GPL-2.0) and its MIT libraries are already a counter-example of the
  rule being for crates only.
- **Cost:** ADR-0410 falls.
- **Precedent:** Projects with permissive-only images (`[outside evidence]`).

### Option B: LGPL libraries allowed in ported programs whose build is in `ports/` and relinks from the fetched sources

- **Description:** An LGPL library may be linked statically into a ported program if `ports/` builds the program from
  pinned sources (ADR-0109) so that the library can be replaced and the program relinked, and `docs/dependencies.md`
  lists it with its licence.
- **Evidence for:** Drivers 1 to 3; the same relinking a distribution offers through shared libraries is offered
  through the build.
- **Evidence against:** A person with only a built image has no object files; they must rebuild from the repository.
- **Cost:** A rule and a column in `docs/dependencies.md`.
- **Precedent:** Genode builds Qt (LGPL) into its Falkon browser from its own port system
  (browser-engine/survey.md#how-other-systems-got-a-browser; Qt's licence `[outside evidence]`).

### Option C: LGPL code only as a separate program

- **Description:** As TinyCC: an LGPL library may run only in a process of its own, talking over IPC.
- **Evidence for:** No combined work.
- **Evidence against:** GLib is linked into every WebKit process; not separable.
- **Cost:** Not feasible for WPE.
- **Precedent:** ADR-0109's GPL programs.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **The rule.** A library under LGPL-2.0, LGPL-2.1 or LGPL-3.0 (any "or later") may be linked statically into a
   program built by `ports/Makefile`, if the library and the program are both built from sources pinned in
   `ports/sources.lock`, the library's patches are in `ports/patches/` under the library's licence, and
   `docs/dependencies.md` lists it with its licence and the programs that link it. Never in the kernel, a native
   program or a `no_std` crate.
2. **Recorded in `CLAUDE.md`** by the architect if accepted, beside the GPL and OFL clauses.
3. **Listed**: GLib, libsoup, glib-networking, WebKit's LGPL files, libgcrypt and libgpg-error (required by WPE 2.54.x;
   on `main` OpenSSL replaces libgcrypt); libtasn1 and libhyphen with their licences once read.

### Effect of the vision

For target 2 none: licences do not change what a program can reach. Target 3: the browser can be built. Networking:
none. Threads: none.

### Examples

`docs/dependencies.md` gains rows such as `| GLib | 2.84.x | LGPL-2.1-or-later | libglib, libgio, libgobject in browser (static) |`.

## Consequences

- **Positive.** WPE and its GNOME libraries are allowed with a written condition.
- **Negative.** The image contains programs that combine BSD, LGPL and permissive code; anyone redistributing an image
  must give the sources and the build (the repository and `ports/`).
- **Follow-up.** The architect's line in `CLAUDE.md`; rows in `docs/dependencies.md`.

## Invariants

- **INV-0421-1.** Every library linked into a ported program has a row in `docs/dependencies.md` with its licence, and
  every LGPL row names programs built by `ports/Makefile` only. *Check:* a `make check-port-licences` script that reads
  `ports/sources.lock` and the table and fails on a missing row or an LGPL library used outside `ports/`.

## Open questions

- Whether the architect wants LGPL-3.0 libraries treated as LGPL-2.1 ones (none is needed now).

## Revisit triggers

- A request to ship a built image without the repository.

## Architect's decision
