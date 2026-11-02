# TASK-14: Stage 5 — graphics, input, compositor, terminal

## Precondition

`main` has stage 4 (tag `stage-4`), the cheap-loops tooling (`make test` summary, `tools/mutants/`, subagents in `.claude/agents/`) and the virtio mutant tests. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`.

Read **in full**: ADR-0002, ADR-0006, ADR-0050 to ADR-0055, ADR-0057, ADR-0060, ADR-0070 to ADR-0072, ADR-0080. Read the research catalog entries in category 13 (graphics, input, window systems) and category 11 (terminals, sessions, consoles) of `docs/research/design-mistakes.md` and `held-up-well.md`, and the Plan 9 (rio), Haiku/BeOS, Fuchsia and Genode sections of `alternatives-postmortem.md`.

Roadmap stage 5:

| Stage | Content | Done when |
|---|---|---|
| 5 | virtio-gpu, virtio-input, compositor, terminal as GUI client | screenshot shows a terminal window |

This task makes the criterion stronger: **the terminal window runs `zosh`, and keys typed into the window run commands, including the file commands of stage 4.**

**Architect's direction (record it in `docs/design.md` under Vision as a dated note):** after stage 4 the architect chose to continue with stages 5 and 6 (general-purpose base). The isolation direction stays open and must not be precluded. Every ADR in this task has a short "Effect of the vision" subsection, as in TASK-10.

## Step 1: proposed ADRs (numbers 0090 to 0099 reserved)

Template-exact, status `proposed`, at least three options each, a Linux baseline (DRM/KMS and Wayland or X11) and a non-Unix precedent. **One decision per ADR.** Short. At least:

1. **GPU driver scope.** virtio-gpu 2D only (resource create, attach backing from a memory object, set scanout, transfer to host, flush), no 3D. Driver as a user process under ADR-0054. How `init` tells which virtio device is which (device ID register) and gives each driver only its device.
2. **Input drivers.** virtio-keyboard and virtio-tablet (absolute pointer). Event format inside Zoraya (raw key codes plus a keymap in the compositor, or translated keys from the driver), and where the keyboard layout lives.
3. **Compositor model and protocol.** Clients draw into memory objects they create (ADR-0051), submit damage over a channel protocol declared with ADR-0052, and the compositor composes into the scanout. Buffer lifecycle (who may write when; ADR-0051's rule that shared memory is hostile input that can change between reads), damage validation, surface sizes and limits, a frame callback or not. The X11 lesson: no client can read another client's pixels or input, or inject input. A client gets a compositor channel only if its launcher passes one.
4. **Input routing and focus.** Focus model, what the compositor keeps for itself (for example a key to switch windows), how a misbehaving or hung client cannot block input to others.
5. **Text rendering.** Bitmap font first, or a TTF rasteriser (for example `fontdue`) now. If a font file is embedded, its license must allow it and be recorded; follow `docs/dependencies.md` for any crate.
6. **Terminal.** The terminal is a GUI client that speaks a terminal protocol to the shell over a channel (no byte streams yet: they are stage 6). Which escape sequences it supports now (the subset `zosh` needs, with a plan for what dash and tcc will need), scrollback, how `zosh` chooses between the kernel console (ADR-0060) and a terminal channel. The design must not preclude job control and a controlling terminal, which the POSIX broker adds in stage 6 (ADR-0002).
7. **`init` and the QEMU command line.** What `init` starts in GUI mode; `tools/run-qemu.sh --gui` attaching `virtio-gpu-device`, `virtio-keyboard-device`, `virtio-tablet-device` (MMIO, matching ADR-0054); headless GUI mode for tests (`-display none` with screendumps). `make run` stays serial; `make run-gui` opens a window on the Mac.

Commit the ADRs before the code they govern. Implement the recommended options; the architect accepts or changes them at review, and merging depends on that.

## Step 2: build

- GPU driver, keyboard and tablet drivers as user processes, logic in host-tested crates with hostile fake devices (as `zoraya-virtio` does for blk).
- The compositor service, its protocol crate, and a small client library.
- The terminal client, with `zosh` running inside it.
- `init` wiring for GUI mode; `zosh` unchanged in serial mode.
- Kernel changes only if an ADR requires them.

## Step 3: tests

- **Roadmap criterion, automated:** boot headless in GUI mode, take a screendump through QMP, and check that a terminal window with the `zosh` prompt is there (compare against a reference image with a stated tolerance, or check pixel regions; say which and why). Then inject key events through QMP, type `write a.txt hi`, `cat a.txt`, and check the rendered output.
- Compositor isolation: a client never receives input meant for another, never sees another's pixels; a client cannot inject input.
- Hostile clients: damage outside the surface, absurd sizes, a memory object smaller than declared, a buffer rewritten during composition, a client that never reads events, a client that floods damage. The compositor and other clients keep working.
- Driver fault paths as in stage 4: a driver killed and restarted by `init`.
- Host tests for decoders, damage arithmetic, the terminal's escape-sequence parser (with fuzzing of arbitrary byte input) and the font renderer.
- Every invariant of the new ADRs gets its check, with status in `invariants.md`.
- Measurements under HVF and TCG: time from key event to updated screendump, frames per second of a full-screen redraw, compositor CPU time per frame.

`make test` passes from a clean clone in all configurations. CI stays green.

## Review and process

Follow `CLAUDE.md`: implementation and the independent review on the main model; test runs and mutant triage through the scripts and subagents. Independent review by a subagent that did not write the code, with mutation testing of the new crates; findings and dispositions in `docs/tasks/TASK-14-review.md`; then one check of the fixes only (changed lines). If that check finds errors, fix and repeat it at most once more, then report.

## Documentation

`docs/design.md`: as-built sections, repository map, a "Using the GUI" section, the Vision note above. Mark stage 5 done in the roadmap when the criterion test passes: the architect instructs you to do this. `index.md`, `invariants.md`, `dependencies.md`, `backlog.md`.

## Git

Branch `stage-5` from the latest `main`. Configured git identity only. Commit after each step. **Never merge, never push.**

## Out of scope

POSIX, byte streams, job control (stage 6); 3D, multiple monitors, clipboard, window decorations beyond what focus needs; ADR-0056 quotas.
