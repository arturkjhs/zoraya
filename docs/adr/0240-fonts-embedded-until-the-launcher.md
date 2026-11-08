# ADR-0240: Fonts embedded in the programs `init` starts, until the launcher exists

- **Status:** proposed
- **Date:** 2026-11-08
- **Depends on:** ADR-0053, ADR-0162, ADR-0168
- **Evidence:** DM-081, desktop/survey.md#the-ofl-next-to-bsd-code, desktop/survey.md#candidate-fonts-and-their-licences

## Context
ADR-0162 (accepted) says that a client loads a font "from a file handle the launcher gives it (ADR-0053: no global
name; the launcher passes `/system/fonts` as a read-only directory handle to every GUI program)", and that "the
compositor embeds the UI font for titles". The launcher is ADR-0168's panel, built in stage 7d (TASK-24). Stage 7a moves
the terminal onto `zoraya-text` (ADR-0162's follow-up: `zoraya-term` and the `term` program first), and `term` is
started by `init`, not by a launcher: today nothing hands it a `/system/fonts` handle, the system image has no
`/system/fonts`, and the GUI also runs where the volume is a test's own (`make test-gui`'s `rootfs`, the POSIX images
of `make test-posix-gui`, the null disk of the `test-qemu` GUI configurations). ADR-0162 is silent on the time before
the launcher. Measured: the four font files (`desktop/survey.md#candidate-fonts-and-their-licences`) are 1.38 MB together (`docs/dependencies.md`, fonts table); `term` needs
two of them and Inter as its fallback. Assumed: nothing else.

## Decision drivers
1. Follow ADR-0162's end state, and change nothing it decided: the fonts, the rasteriser, the licences (the OFL lets
   the unmodified files travel inside a program, `desktop/survey.md#the-ofl-next-to-bsd-code`).
2. A font a program draws with is never hostile input it chose (DM-081): the system's files only.
3. Stage 7a must not wait for 7d, and must not build a font-passing path in `init` that 7d replaces.
4. The terminal must draw in every GUI configuration `make test` runs, whatever volume it has.

## Considered options

### Option A: A system font directory read by path (Linux's fontconfig)
- **Description:** Put the files in `/system/fonts` on the root volume and have `term` open them by path.
- **Evidence for:** No change of format when the launcher arrives; the files are on disk once.
- **Evidence against:** A global name, against ADR-0053; the test volumes would each need the files; the POSIX images
  are not the system image.
- **Cost:** *(estimate)* A copy step in four image builds, and a name lookup in `term` that 7d removes.
- **Precedent:** fontconfig on every Linux desktop.

### Option B: `init` passes a fonts directory handle, as the launcher later will
- **Description:** `init` opens `/system/fonts` and gives `term` the handle at a slot, which is ADR-0162's shape with
  `init` as the launcher.
- **Evidence for:** ADR-0162's end state from the start.
- **Evidence against:** The same volume problem as Option A (driver 4); `init`'s session table shrinks in 7d
  (ADR-0168), so this path is written to be moved.
- **Cost:** *(estimate)* A slot in `term`, a file reader in `term` and code in `init`.
- **Precedent:** ADR-0053's handle passing.

### Option C: The programs `init` starts embed the system fonts until the launcher exists
- **Description:** `zoraya-fonts`, a crate of constants, includes the unmodified files of `fonts/` (`include_bytes!`),
  and the programs that draw text before 7d (`term`; the test runner on the host) take the bytes from it. In 7d the
  launcher hands out `/system/fonts` and programs it starts load from that handle, as ADR-0162 says; the compositor
  keeps embedding the UI font, as ADR-0162 already allows.
- **Evidence for:** Drivers 1 to 4: the fonts are the system's files, checked by hash (INV-0162-5); every
  configuration draws; nothing is built that 7d removes but one constant per program.
- **Evidence against:** `term` grows by about 960 KB in the ramdisk (two JetBrains Mono files and Inter); every
  program that embeds a font carries its own copy until 7d.
- **Cost:** *(estimate)* 40 lines; 0.96 MB of ramdisk.
- **Precedent:** The compositor's embedded UI font in ADR-0162; Haiku's app_server, which links its default fonts.

## Recommendation
Recommended: **Option C**, as a gap ADR-0162 leaves before ADR-0168's launcher exists, not a change of its decision.
It is a recommendation, not a decision. Stage 7d moves the programs the launcher starts to the `/system/fonts`
handle and adds the files to the system image.

## Consequences
- **Positive.** 7a's terminal draws with the real fonts in every configuration; the fonts are checked by hash in one
  place.
- **Negative.** The ramdisk grows by about a megabyte until 7d.
- **Follow-up.** In 7d: `/system/fonts` in the system image, the launcher's handle, `term` loading from it.

## Invariants
- **INV-0240-1.** Only the files of `fonts/`, through `zoraya-fonts`, are embedded as fonts in a program. *Check:* a
  grep in `make check-tokens` for `include_bytes!` of a `.ttf` outside `crates/zoraya-fonts`.

## Open questions
- Whether the compositor's titles (7b) and the toolkit (7d) take Inter Medium from `zoraya-fonts` or from the handle:
  7d decides with the launcher.

## Revisit triggers
- The launcher of ADR-0168 exists: this ADR's interim ends.
- The ramdisk's size matters (a limit of the image or of QEMU's loading).

## Architect's decision
