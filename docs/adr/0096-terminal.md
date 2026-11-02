# ADR-0096: The terminal client and its protocol with the shell

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0004, ADR-0006, ADR-0050, ADR-0052, ADR-0060, ADR-0093, ADR-0094, ADR-0095
- **Evidence:** DM-069, DM-070, DM-071, DM-072, DM-073, CP-25, alternatives-postmortem.md#plan-9--inferno

## Context
Stage 5's criterion is a window running `zosh`. `zosh` reads raw console bytes and writes text with `\n`, `\x08 \x08` for erasing and nothing else (TASK-11). Stage 6 brings dash and TinyCC through `libposix`, which expect a VT byte stream on a terminal, and a POSIX broker that owns sessions and the controlling terminal (ADR-0002 rule 1). Byte streams are stage 6 (ADR-0050). So the terminal speaks to the shell over a channel, in messages. DM-071 records that in-band escape sequences let output forge input through query replies; DM-072 that `TIOCSTI` let anyone with the terminal inject keys; CP-25 asks for separated rights and out-of-band control natively, with a VT translation for POSIX. Measured: nothing on Zoraya.

## Decision drivers
1. The terminal never puts anything into the input direction but what the person typed (DM-071, DM-072).
2. Programs ported in stage 6 must run without changes to their output (DM-073: a typed console API needed ConPTY to serve them).
3. No terminal policy in the kernel (DM-069, ADR-0060).
4. Job control and a controlling terminal must remain possible for the broker (DM-070, ADR-0002).
5. Bounded memory and a total parser: output is hostile input to the terminal.

## Considered options

### Option A: A kernel pseudo-terminal with a line discipline (Linux)
- **Description:** A pty pair; the terminal holds the master, the shell the slave, and the kernel's line discipline edits, echoes and sends signals.
- **Evidence for:** Every Unix program works unchanged.
- **Evidence against:** DM-069 and DM-070: the TTY layer and its signal generation in the kernel, against ADR-0060. Query replies go into input (DM-071).
- **Cost:** *(estimate)* Thousands of lines in the kernel.
- **Precedent:** Unix.

### Option B: VT bytes in channel messages, with control out of band
- **Description:** The shell sends output as bytes the terminal interprets as a VT subset. The terminal sends the person's keys as bytes, and its size in a separate message. It answers no query in band.
- **Evidence for:** Drivers 1 to 5. The VT translation CP-25 asks for lives in one place, the terminal, which stage 6's programs need anyway (driver 2).
- **Evidence against:** Output control stays in band: a program can still move the cursor or clear the screen of what others printed, as on any VT.
- **Cost:** *(estimate)* A 500-line host-tested crate, a 250-line process, a protocol crate.
- **Precedent:** Terminal emulators in general, with the query path removed.

### Option C: A typed native protocol, VT translation later
- **Description:** Output is typed operations (text, move cursor, erase), and input typed keys; a translator turns VT into these for POSIX programs, as CP-25 proposes.
- **Evidence for:** Control out of band natively; a program cannot embed control in data.
- **Evidence against:** Two protocols to build now for one native client, and the translator is imperfect, as ConPTY's own documentation says (DM-073). `zosh`'s editor would need rewriting.
- **Cost:** *(estimate)* Option B plus the typed protocol.
- **Precedent:** The Windows console API (DM-073).

### Option D: Windows as text servers (Plan 9's rio)
- **Description:** Each window serves `/dev/cons` with plain text; there are no escape sequences and no cursor addressing, and the window, not the program, edits text (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence for:** No escape sequences at all, so nothing to inject.
- **Evidence against:** Programs that address the cursor do not run; stage 6's ports would need changes (driver 2).
- **Cost:** *(estimate)* Small now, large in ports.
- **Precedent:** Plan 9.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Protocol** (`zoraya-proto-term`). Shell to terminal: `Output { bytes }`, up to 4 KiB, no reply. Terminal to shell: `Input { bytes, dropped }`, the bytes keys produce, and `Size { columns, rows }`, sent first and on any change. A full queue makes the shell wait for `WRITABLE`; the terminal never blocks on the shell: input that does not fit is dropped and counted in `dropped`.
- **Keys to bytes.** Printable text as UTF-8; Enter `\r`; Backspace `0x7f`; Tab `\t`; Escape `0x1b`; Ctrl with a letter its control code; arrows `ESC [ A` to `ESC [ D`; Home, End, Delete, Page Up and Page Down as their VT sequences. Shift with Page Up or Page Down scrolls the view and sends nothing.
- **Output subset.** Printable UTF-8 (ADR-0095 draws boxes outside ASCII). `\n` moves to the start of the next line, as the kernel console does, `\r` to the start of the line, `\x08` one column left, `\t` to the next multiple of 8, BEL is ignored. `ESC [` with up to 8 numeric parameters: `A B C D` (move), `H` and `f` (position), `J` and `K` (erase, modes 0 to 2), `m` (reset, bold, reverse, the 8 colours of foreground and background, default colours). `ESC c` resets. Anything else, `OSC` and `DCS` strings included, is consumed and ignored, with a bound on its length. Wrapping at the right edge, scrolling at the bottom.
- **What `zosh` needs** is `\n`, `\x08` and text; dash needs nothing more; TinyCC prints plain text. Later programs (`vi`, `less`) will need scroll regions, insert and delete line, the alternate screen and cursor visibility: each is a new sequence in the parser, not a protocol change.
- **No answers.** The terminal answers no query: not `ESC [ 6 n`, not device attributes, not window titles. It has no path from output to input (driver 1).
- **Scrollback.** 500 lines `[assumption]`, a fixed ring.
- **How `zosh` chooses.** `zosh` uses a channel at slot 6 as its terminal if it has one, and the console (ADR-0060) otherwise. With a terminal it neither reads nor holds the console's `READ` right.
- **Programs `zosh` starts** still get the console for output, as in stage 4: their output appears on the serial line, not in the window. A program has no byte stream to inherit until stage 6.
- **Job control.** The terminal knows no processes. Whoever holds the shell end interprets Ctrl-C, which is the byte 3: `zosh` now, the broker in stage 6, which can sit between the terminal and its processes and send `job_signal` (ADR-0004). A controlling terminal is the broker's mapping from a session to this connection. Nothing in the protocol names a process.

**Tensions with accepted ADRs.** *ADR-0006 rule 1* lists terminals as byte streams. This connection is a channel, as stage 6's byte streams do not exist yet; a POSIX terminal handle is the broker's, over this protocol.

### Effect of the vision
None on the choice. For target 2 an agent's terminal output is hostile: the parser is total and bounded, and no output becomes input, which removes DM-071's attack. Rights separated as CP-25 asks (output only, input only) can be added as narrowed connections, a new method under ADR-0052, without changing these messages. Moving to Option C later is a second protocol beside this one, at Option C's cost. Networking: a remote shell is a relay of these messages.

### Deferred to stage 6
The broker, sessions, the controlling terminal and `isatty`; byte streams for children's output; termios in `libposix` (canonical mode would be the broker's or `libposix`'s, never the kernel's).

## Consequences
- **Positive.** A window that runs `zosh`, a parser stage 6 can grow, and no reply path for injected queries.
- **Negative.** Children of `zosh` print to the serial console in GUI mode until stage 6. Programs that query the terminal get no answer and must use defaults.
- **Follow-up.** Narrowed connections; the VT sequences `vi` and `less` need.

## Invariants
- **INV-0096-1.** The parser is total: any byte sequence leaves the cursor inside the grid and never panics. *Check:* host fuzz test of random and structured byte sequences.
- **INV-0096-2.** Output never produces input: the terminal's input to the shell is a function of keys alone. *Check:* host test feeding every query sequence (`ESC [ 6 n`, `ESC [ c`, `ESC ] ... BEL`, `ESC P ... ESC \`) and asserting no input is produced; the parser has no output path.
- **INV-0096-3.** The terminal's memory is bounded whatever the output: scrollback is a fixed ring and an unterminated string is cut at its bound. *Check:* host test writing far more lines and a megabyte of unterminated `OSC`, asserting the sizes.

## Open questions
- Whether 500 lines of scrollback suit a person.

## Revisit triggers
- A ported program that needs a terminal reply.
- Stage 6's broker design, which may move the shell end.

## Architect's decision
