# ADR-0419: The engine's window: a WPEPlatform backend over the window channel, chrome drawn by the browser, the clipboard and files only by gesture

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0093, ADR-0164, ADR-0167, ADR-0170, ADR-0172, ADR-0245, ADR-0317, ADR-0322, ADR-0326, ADR-0410, ADR-0411, ADR-0413
- **Evidence:** CP-24, DM-079, X-11, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#ladybird, browser-engine/survey.md#the-window-text-and-the-network, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WPE's UI process hands each page frame to a `WPEView`, whose `render_buffer` must report it rendered and then
released; a platform implements `WPEDisplay` (`connect`, `create_view`), usually `WPEToplevel`, and optionally a
keymap, clipboard, screens and input methods; buffers are DMA-BUF or SHM (`ARGB8888` in a `GBytes`), and
`wpe_buffer_import_to_pixels` gives the pixels of either; events are pointer, scroll, key and touch with keyvals;
`WPEClipboard` reads synchronously (browser-engine/survey.md#wpe-webkit-in-detail). WPE has no browser chrome: an
application draws its own. Ladybird's front end implements `ViewImplementation` with clipboard hooks and
`FileDownloader` (browser-engine/survey.md#ladybird).

Zoraya's window channel for POSIX programs (ADR-0317) gives one window per process with a shared 32-bit pixel buffer,
damage and commit with one frame in flight, key (Linux code, code point, modifiers), pointer, wheel (ADR-0322), focus,
resize and close events, integer scale (ADR-0164); no cursor shape, no clipboard, no chooser, no second window
(browser-engine/survey.md#the-window-text-and-the-network). The clipboard is the compositor's and moves only by gesture
(ADR-0170); the chooser gives a file without its path (ADR-0167, ADR-0245). Neither is offered to POSIX programs yet.

## Decision drivers

1. **The compositor owns input and output** (CP-24): no client reads another's input or the clipboard at will
   (DM-079's X11 model avoided).
2. **The platform interface upstream supports**: implement WPEPlatform, not patches in WebKit's UI process.
3. **A page cannot take what the person did not give**: the clipboard on paste only; files on a chooser pick only.
4. **A small protocol core** (X-11): new events for the clipboard and the chooser, not new policy in the compositor.

## Considered options

### Option A: A Wayland compositor socket for the engine (the Linux way)

- **Description:** Give WPE its built-in Wayland platform by a Wayland server in or beside Zoraya's compositor.
- **Evidence for:** No WPEPlatform code to write.
- **Evidence against:** ADR-0317 rejected a display socket at a well-known path (ambient authority, no record); a
  Wayland server is a large new component.
- **Cost:** *(estimate)* A Wayland server: many thousands of lines.
- **Precedent:** Linux desktops.

### Option B: A Zoraya WPEPlatform backend over ADR-0317's window, the chrome drawn by the browser, the clipboard and chooser added to the window channel

- **Description:** `WPEDisplayZoraya`, `WPEViewZoraya`, `WPEToplevelZoraya`, `WPEScreenZoraya`, `WPEKeymapZoraya` and
  `WPEClipboardZoraya` in a patch beside the headless backend; the browser program draws its toolbar with
  `zoraya-text` above the page; three event kinds and two calls are added to the C window interface.
- **Evidence for:** Drivers 1 to 4; WPEPlatform's mandatory surface is three functions; the headless backend shows the
  shape (any buffer, frames paced by a timer).
- **Evidence against:** One copy of each frame from the Web process's buffer into the window's; no GPU; one window, so
  `window.open` opens a tab, not a window; no cursor shapes.
- **Cost:** *(estimate)* 1,500 lines of backend, 1,500 of chrome, 300 in `libposix` and the compositor's client rules.
- **Precedent:** Haiku's WebKit, drawing into a `BBitmap` in a native window with Haiku's own run loop
  (browser-engine/survey.md#how-other-systems-got-a-browser).

### Option C: A native Rust UI process embedding the engine

- **Description:** The UI process is a Zoraya native application (`zoraya-ui`) that links WPE's C API.
- **Evidence for:** The chrome looks like every other native window.
- **Evidence against:** A native program linking a POSIX C++ library of millions of lines breaks the personality split
  (ADR-0002); WPE's UI process needs GLib and the POSIX layer.
- **Cost:** *(estimate)* A bridge of its own.
- **Precedent:** none here.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **The backend.** `WPEDisplayZoraya::connect` opens the window (`zoraya_window_open`, title from the description);
   `create_view` gives the one view. `WPEViewZoraya::render_buffer` copies the buffer's pixels
   (`wpe_buffer_import_to_pixels`; `ARGB8888` to the window's `XRGB8888` words, alpha dropped) below the chrome,
   damages the changed rows, commits when the compositor's frame callback allows (one frame in flight, ADR-0093), and
   reports the buffer rendered on the callback and released at once after the copy. Registered at WPE's extension
   point with priority 100 and linked in.
2. **Input.** Keys: the compositor's Linux code and modifiers become the hardware keycode and modifiers; the code point
   becomes the keyval (`wpe_unicode_to_keyval`), named keys by a table; key repeat comes from the compositor's repeated
   events. Pointer, buttons and the wheel (`ZW_WHEEL`, in 120ths of a notch) become `WPEEvent`s in logical pixels;
   focus and close map to the view's focus and the toplevel's close.
3. **Scale.** `WPEScreenZoraya` reports `zoraya_window_scale()`; a scale change re-creates the view's size; the page
   is rendered at physical pixels.
4. **The chrome.** The browser program draws a 40-logical-pixel toolbar (back, forward, reload, the address field, a
   status line) with `zoraya-text` (ADR-0326's calls), so the window's frame and pills stay the compositor's and the
   chrome uses the desktop's fonts. The address field takes typed text and paste.
5. **Clipboard, by gesture.** Three window events are added: `ZW_COPY` and `ZW_CUT` (System+C, System+X) make the
   browser run the page's copy or cut command and send `zoraya_window_offer(text, len)` within ADR-0170's 500 ms;
   `ZW_PASTE` (System+V) carries the text, which the browser inserts through the page's paste command.
   `WPEClipboardZoraya` holds only the text of the current paste and is empty otherwise, so a page's
   `navigator.clipboard.readText()` outside a paste gets nothing.
6. **Files, by gesture.** A second bootstrap slot gives the browser a chooser channel (ADR-0317's follow-up, ADR-0167):
   a download asks `Save` with the suggested name and writes through the returned entry; `<input type=file>` asks
   `Open` and passes the chosen file's bytes to the page through the UI process. The window's pills gain the chosen
   entries (ADR-0172 point 4). No download is written without a chooser answer; there is no downloads directory.
7. **Not offered.** In an untrusted room, downloads and uploads (its programs get no chooser channel, INV-0172-2);
   cursor shapes (the protocol has none), a second window (`window.open` and `target=_blank` open in
   the same window, the previous page in history), pointer lock, gamepads, input methods.

### Effect of the vision

For target 2 an agent's page cannot read what the person copied elsewhere, cannot write a file the person did not pick,
and cannot open a second window to look like a system dialog; the pills show what was picked. Target 3: copy, paste,
downloads and uploads work the way the native applications' do. Networking: none. Threads: the backend runs on the UI
process's main thread with GLib's loop polling the window channel (ADR-0414).

### Examples

The person selects a paragraph and presses System+C; System+V into the text editor pastes it. They click a PDF link:
the chooser sheet asks where to save `report.pdf`; they pick `~/Downloads`; a pill `+w ~/Downloads/report.pdf` appears on
the browser's frame and the file is written. A page that calls `navigator.clipboard.readText()` on load gets an empty
string.

## Consequences

- **Positive.** The engine's own platform interface; the desktop's gestures for the clipboard and files; POSIX programs
  gain a clipboard and the chooser, which later ports reuse.
- **Negative.** One copy per frame; one window; no cursor shapes; a chrome to maintain.
- **Follow-up.** The backend patch; the three events and `zoraya_window_offer` in `zoraya-surface` and `libc.a` (a
  `posix-surface-added` block); the chooser slot; GUI tests (copy, paste, download, upload, scale 2).

## Invariants

- **INV-0419-1.** The browser's clipboard holds text only between a `ZW_PASTE` event and the end of the paste command it
  causes; at every other time a read of it is empty. *Check:* a host test of the backend's clipboard state machine and
  a GUI script whose page polls `readText()` before, during and after a paste.
- **INV-0419-2.** The browser writes a downloaded file only through an entry the chooser returned in this run.
  *Check:* a GUI script that starts a download and cancels the chooser, then checks that no file was written and no
  refused path was reported.
- **INV-0419-3.** Every buffer `render_buffer` receives is reported released exactly once. *Check:* a host test of the
  view with a fake window.

## Open questions

- The frame copy's cost at 2560×1600 under HVF (ADR-0317's revisit trigger is 16 ms for a full window).
- Whether WebKit's `<select>` popups and context menus need a popup surface or can be drawn inside the view.

## Revisit triggers

- A GPU path in the compositor: DMA-BUF-like buffers instead of a copy.
- A second POSIX program with several windows.

## Architect's decision

Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request. As recommended.
