# ADR-0317: The window of a POSIX program: a `window` line in the description, a client channel from the panel, a raw-surface client in a static library

- **Status:** accepted
- **Date:** 2026-11-13
- **Depends on:** ADR-0052, ADR-0093, ADR-0100, ADR-0104, ADR-0153, ADR-0160, ADR-0165, ADR-0168, ADR-0172, ADR-0310
- **Evidence:** CP-01, CP-17, CP-25, alternatives-postmortem.md#genode, alternatives-postmortem.md#haiku--beos, desktop/survey.md#haiku

## Context
TASK-27 part 9d ports NetSurf, a C program whose framebuffer front end draws into a pixel buffer through `libnsfb`. Today
no POSIX program has a window: a graphical application is a native Rust program the panel starts with a launch record
(ADR-0168 INV-0168-1), holding the compositor's channel and, with `zoraya-app` (ADR-0165), turning events into widget
events. A POSIX program runs from `zosh` through the broker; its handles are the ones the shell's plan gives it
(ADR-0153, ADR-0310's `ZPBX` bootstrap). Read in this session: `AddClient` and `AddRoomClient` are manager requests only
the panel and `init` may send (`zoraya-proto-gui`, `MANAGER_REQUESTS`); the shell holds a channel to the panel
(`Manage`/`Room(R)`) and the compositor learns a client with its record (the room, the grants as pills). NetSurf needs
exactly: one toplevel, a buffer of the window's size it paints into, damage rectangles, the compositor's key, text,
pointer and resize events, a title, a cursor shape, and a clipboard later.

## Decision drivers
1. The compositor must keep drawing, from its records only, what the program holds (INV-0172-1): the window of a program
   that reaches the Internet shows the grants of its line, as the terminal of a room does.
2. A person grants; a program never asks for focus, position or stacking (INV-0165-2): the client requests of
   `zoraya-proto-gui` are all NetSurf may send.
3. The least change to trusted code: no new manager request in the compositor, no change to the chooser's path.
4. C code must not re-implement the wire protocol (a second encoder is a second place for a parser bug).
5. Reversible: a program that does not say `window` is unchanged (CP-17: a failing program is restarted alone). Scope held (CP-25): one window, no clipboard yet.

## Considered options

### Option A: The Linux/POSIX way, a display socket (X11, Wayland) at a well-known path
- **Description:** The program connects to a compositor socket in the namespace and speaks the protocol with a client
  library (libwayland).
- **Evidence for:** What NetSurf's other front ends do.
- **Evidence against:** An ambient authority: any program could open windows and no record would show the grant
  (CP-01); the namespace has no such path (ADR-0107); the compositor identifies a client by the channel it was added with.
- **Cost:** *(estimate)* a name service in the compositor and a way to name a room and a record; large.
- **Precedent:** X11, Wayland (`desktop/survey.md#haiku` compares the servers).

### Option B: A native Rust host window beside the C program (Haiku's app_server proxy)
- **Description:** A Rust program `webwin`, started by the panel, owns the window with `zoraya-app`; NetSurf runs as its
  POSIX child and sends pixels over a byte stream; events come back as records.
- **Evidence for:** Reuses the whole existing client library; no C code for the protocol.
- **Evidence against:** Needs a Rust program that starts POSIX children with grants (the shell's job today, ADR-0153),
  copies every frame through a pipe (3 MiB for a full 1024 x 768 frame) and gives the person two processes to understand.
- **Cost:** *(estimate)* a second launcher for POSIX programs; copy cost of damage rectangles.
- **Precedent:** Haiku's `BWindow` proxy (`alternatives-postmortem.md#haiku--beos` describes the server model).

### Option C: A `window` line in the description; the shell asks the panel for a client channel; a static library speaks the protocol
- **Description:** The description of `netsurf` has a key `window TITLE`. Running the line, `zosh` asks the panel on its
  existing channel for a client channel for that application in the shell's room (a new request on the panel channel
  that does what `Launch` does for `AddRoomClient`, minus starting a process); the panel adds the client to the
  compositor with the room's record and the line's grants, and returns the client end; `zosh` moves it into the child at
  a slot named in the `ZPBX` bootstrap; `libposix` exposes it as `zoraya_window_channel()`. A new static library
  `libzgui.a`, built from a new crate `zoraya-surface` (no_std, host-tested with a fake compositor, the `Platform` trait of
  `zoraya-app` reused for the channel and the memory objects), implements the client side of `zoraya-proto-gui`:
  `open`, `buffer`, `damage`/`commit`, `poll_event`, `close`. NetSurf's `libnsfb` surface calls it through five C functions.
- **Evidence for:** Drivers 1 to 5: the compositor sees a client like any other, with a record; one Rust encoder; no
  second launcher.
- **Evidence against:** The shell and the panel grow one request each; a Rust static library inside a C program has the
  allocator question (below).
- **Cost:** *(estimate)* 600 to 1,000 lines of Rust with tests, 150 lines of C, 100 lines each in the shell and panel.
- **Precedent:** Genode's session requests (`alternatives-postmortem.md#genode`): a parent creates a session for a child
  and hands it the capability.

### Option D: Make the program a native application (NetSurf behind the Rust ABI)
- **Description:** Cross-build NetSurf against Zoraya's native runtime instead of libposix.
- **Evidence against:** NetSurf, libcurl and OpenSSL are POSIX code; ADR-0109 chose the POSIX path for ports.
- **Cost:** *(estimate)* a second libc.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision. The details a session may decide:

1. **The line.** `window TITLE` is a new key of `zoraya-describe`; only a trusted room's shell honours it, as for the
   chooser (an untrusted room gets no window of a POSIX program while the gate is closed).
2. **The panel's request** carries the application name and the room; the reply is `ManagerDone`-style: the client end of
   a channel, with no right to move, the compositor's end added with `AddRoomClient` and the line's entries. The panel
   refuses a second window request from the same shell for the same program while the first is open (a quota of one
   window per run in this ADR; popups use the protocol's popup surfaces).
3. **The buffer** is a memory object of the window's size in `XRGB8888`-compatible 32 bit words, as `zoraya-app` makes
   its buffers; a resize event allocates a new one before the next commit. `libnsfb`'s surface paints into it and calls
   `damage`; `commit` is sent when the compositor's frame callback allows (ADR-0093), at most one frame in flight.
4. **Events** map: key down/up with the compositor's key code and modifiers to `NSFB_KEY_*` (a table in the C surface),
   text input to `NSFB_EVENT_KEY_DOWN` with the code point (NetSurf's framebuffer front end takes text from key events),
   pointer move/press/release to `NSFB_EVENT_MOVE_ABSOLUTE`/`KEY_DOWN` of `NSFB_KEY_MOUSE_n` (no wheel: the protocol has no wheel event, see item 7),
   close request to `NSFB_EVENT_CONTROL` quit, resize to a new surface geometry.
5. **The allocator.** `libzgui.a` uses the C program's `malloc` through the standard Rust `GlobalAlloc` shim that `libposix`
   already has for itself if one exists, otherwise a fixed-capacity arena of 8 MiB in the library (no growth).
6. The library lives in `crates/zoraya-surface` and is linked only by programs that name it (`-lzgui`); `libc.a` is unchanged.

7. **As built by step 2 of 9d** (amendment; the recommendation stands, this records what reading the code decided).
   `crates/zoraya-surface` is an rlib with `Surface<P: Platform>` (open, `pixels`, `damage`, `commit`, `event`,
   `set_title`, `close`), tested on the host with a fake compositor (15 tests: clipping of every edge case, one frame in
   flight, pointer states into moves and button changes, resize, refusal). It is **not** a second static library: a
   second Rust staticlib would carry its own `core`, `compiler_builtins` and `zoraya-rt`'s `svc` (INV-0100-2 allows one
   member), so the allocator question of item 5 is answered by linking the rlib into `libzoraya_posix.a`, which has
   `ProcessHeap` as its `#[global_allocator]`; the five C names (`zoraya_window_open`, `_buffer`, `_damage`, `_poll`,
   `_close`) are then names of `libc.a`, listed in a `posix-surface-added` block by the ADR that lands them. The
   `Platform` trait moves to a tiny crate (`zoraya-platform`, re-exported by `zoraya-app`) so that libposix does not
   link `zoraya-ui`. **The second open question:** `ZPBX` takes a new flag bit, `EXT_WINDOW = 4`, with the handle after
   the random connection's (`ext_handles` and its test change). The protocol has no wheel event, so a wheel is not
   offered (page keys, arrows and the scrollbar work); a blocking wait with a time limit is a poll with short sleeps
   in the C surface until the library can wait on the channel with a deadline.

The names `libc.a` adds for the window (the C surface's five calls; this is not a boundary: C code can issue raw calls on the channel, and what protects the compositor is that it takes only client requests, checks every buffer against the display and the budget, and draws the pills from the record), with `struct zoraya_window_event` in
`ports/netsurf/zoraya_window.h`; the C surface calls them, but C code can issue raw calls on the channel; the boundary is the compositor's client rules, not these names):

```posix-surface-added
zoraya_window_buffer
zoraya_window_close
zoraya_window_damage
zoraya_window_open
zoraya_window_poll
```

## Consequences
- **Positive.** A POSIX program can have a window with a record the person sees; `netsurf` is the first; later ports (a
  PDF viewer, an editor) reuse it.
- **Negative.** The panel and the shell gain a request; `zoraya-describe` gains a key; a Rust library is linked into C.
- **Follow-up.** The clipboard (ADR-0170) and drag and drop for POSIX programs; a second window; downloads through the chooser
  (the chooser channel would be a second `ZPBX` slot).

## Invariants
- **INV-0317-1.** A POSIX program is given a window channel only when its description has `window`, its shell is in a
  trusted room and the panel added it to the compositor with that room's record. *Check:* host tests of `zoraya-shell`
  (the plan) and `zoraya-panel` (the request); a GUI step that reads the pills of the window.
- **INV-0317-2.** `zoraya-surface` sends only the client requests of `zoraya-proto-gui` (`CLIENT_REQUESTS`) and every
  damage rectangle is clipped to the buffer before a pixel is read. *Check:* `cargo test -p zoraya-surface` (`only_client_requests_are_sent`, `damage_is_clipped_to_the_buffer_before_anything_is_sent`).
- **INV-0317-3.** A `libnsfb` call with a rectangle or a coordinate outside the surface changes no memory outside the
  buffer. *Check:* `cargo test -p zoraya-surface` checks the surface's clipping and `fits` over every edge rectangle with a fake `Platform`; what `libnsfb` itself writes is checked only by the GUI step of `make test-netsurf` (the screen), and the buffer's bounds by `Gui::buffer`'s checked size.

## Open questions
- Does `libposix`'s startup already give Rust code a `GlobalAlloc`? *(read `crates/zoraya-posix/src/lib.rs` and the
  `zcc` link line before step 2; the fixed arena of item 5 is the fallback.)*
- Is `ZPBX` extensible by one more handle, or does it need a new flag bit (`zoraya-abi` `posix.rs` line 136)?

## Revisit triggers
- A second POSIX graphical program with different needs (video, GL), or a measured frame cost above 16 ms for a full
  window under HVF.

## Architect's decision
Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.

Option C with item 7's amendment. Item 4's "wheel to mouse 4 and 5" is removed because the protocol has no wheel event; a wheel/scroll event in the compositor protocol is a small follow-up part after 9d (docs/backlog.md). Part 9d merges only after the review's must-fix findings are fixed and checked.
