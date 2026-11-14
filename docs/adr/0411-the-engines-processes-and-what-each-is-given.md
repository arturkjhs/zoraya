# ADR-0411: The engine's processes and what each is given: `posix_spawn` with a grant list, nothing by default

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0003, ADR-0102, ADR-0103, ADR-0104, ADR-0107, ADR-0153, ADR-0172, ADR-0261, ADR-0267, ADR-0287, ADR-0310, ADR-0317, ADR-0324, ADR-0341, ADR-0410
- **Evidence:** CP-01, CP-05, DM-001, DM-002, DM-009, DM-014, GR-005, GR-008, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#genode, browser-engine/survey.md#ladybird, browser-engine/survey.md#processes-handles-and-channels, browser-engine/survey.md#the-posix-library, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WPE WebKit (ADR-0410) runs a UI process that the person starts, a Web process per page group that runs the page's
JavaScript, and a network process that fetches; a GPU process exists but is off (ADR-0410 item 2). On Linux the UI
process starts the helpers with `GSubprocess` and, by default, Bubblewrap: fresh namespaces, read-only system
directories, a seccomp deny list, the network process unsandboxed (browser-engine/survey.md#wpe-webkit-in-detail).
Ladybird splits further, with a UI, Hub, Compositor, WebContent, RequestServer and decoders, and sandboxes each with
Landlock and seccomp (browser-engine/survey.md#ladybird).

Zoraya gives a child what its creator passes and nothing else (CP-05, GR-005), but today a POSIX child that a POSIX
program spawns gets its parent's whole namespace table and none of its network, random or window connections
(ADR-0287 item 3, ADR-0310 item 3; browser-engine/survey.md#the-posix-library). So a WebKit child would get all the
person's files the browser has and no network. The engine needs the opposite: the network process gets the network,
the Web process gets nearly nothing. `posix_spawn` exists with `adddup2` and `addclose` only; `fork` copies every page
(ADR-0102). `spawn` takes at most 64 handles (browser-engine/survey.md#processes-handles-and-channels).

## Decision drivers

1. **The content process holds no authority** (CP-01, DM-009): no file of the person's, no network, no window, no
   chooser. A sandbox built by construction, not by a filter retrofitted over ambient authority (DM-014, GR-008).
2. **No fork** (DM-001): WebKit asks for the `posix_spawn` path itself; a fork of a large UI process would copy every
   page (ADR-0102).
3. **No descriptor leaks** (DM-002): a child gets exactly the descriptors and handles listed.
4. **The person can see who holds what** (ADR-0172, ADR-0267): `top` shows the helpers, `grants` says which process
   holds the network.
5. **Small change to the engine.** WebKit's launcher has one function per platform; a Zoraya one beside the GLib one is
   the size of a file.

## Considered options

### Option A: The Linux way: inherit everything, then restrict (Bubblewrap and seccomp, or Landlock)

- **Description:** Children start with the parent's environment and are confined by namespaces and filters after the
  fact, as WebKit's Bubblewrap launcher and Ladybird's Landlock and seccomp do.
- **Evidence for:** What both engines already do on Linux; no engine change.
- **Evidence against:** Zoraya has no namespaces to unshare or syscall filter to install; the model is the retrofit
  DM-014 records. A child inheriting the table would have the person's files until restricted.
- **Cost:** Not available without a filter mechanism Zoraya does not want.
- **Precedent:** Linux, macOS (Seatbelt), FreeBSD (Capsicum after start).

### Option B: `posix_spawn` with a Zoraya grant list: the child gets only what the attributes name

- **Description:** Three Zoraya extensions of `posix_spawnattr_t` in `libc.a`: a namespace list (the table entries the
  child keeps, each at most as wide as the parent's), a grant mask (which of the parent's network, random and window
  connections the child gets), and a flag that starts from an empty table instead of the parent's. WebKit gets a
  `ProcessLauncherZoraya.cpp` that calls `posix_spawn` with them per process type. The child's handles are the listed
  descriptors (`adddup2`) and the bootstrap of ADR-0107.
- **Evidence for:** Drivers 1 to 5; CP-05's explicit construction; Fuchsia's launcher, where the creator supplies each
  handle and namespace entry (alternatives-postmortem.md#fuchsia--zircon-incl-starnix); Genode's parent that routes each
  session (alternatives-postmortem.md#genode).
- **Evidence against:** Three new non-standard names; the UI process is trusted to pass the network only to the
  network process (it holds the grant itself).
- **Cost:** *(estimate)* 300 lines in `libposix` and its tests, 250 lines of launcher in WebKit.
- **Precedent:** Fuchsia's `fdio_spawn_etc` with its actions; Windows' `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.

### Option C: Helpers described and started by the shell

- **Description:** The browser's description names each helper with its grants; the UI process asks the shell (or a
  launcher service) to start a helper by name, and the shell builds its table from the description.
- **Evidence for:** The UI process never holds the network; the shell's record is the whole truth.
- **Evidence against:** A new request path from a running POSIX program to the shell; the helpers' connections must
  then be joined to the UI process by the shell; more moving parts for a UI process that already holds the window and
  the person's chosen files.
- **Cost:** *(estimate)* 800 lines across the shell, `libposix` and WebKit.
- **Precedent:** SerenityOS's SystemServer, which spawned a WebContent per connection to a portal socket
  (browser-engine/survey.md#ladybird).

### Option D: One process (no helpers)

- **Description:** Build WebKit's single-process mode, as Genode's first Chromium did.
- **Evidence for:** No IPC, no launcher.
- **Evidence against:** WPE offers no single-process mode in its build options `[assumption: none was seen in
  OptionsWPE.cmake]`; the page's JavaScript would run with the window and the network (driver 1).
- **Cost:** Patches against WebKit's architecture.
- **Precedent:** Genode 20.08 (multi-process not in use), Haiku's WebKitLegacy.

## Recommendation

**Recommendation (not a decision): Option B.** It changes three accepted ADRs, for children started with the new
attributes only: ADR-0287 item 3 and ADR-0310 item 3 (a POSIX child gets no network and no random service) become "no
network and no random service unless its parent passes its own with the grant mask"; ADR-0341 item 7 (children get a
program's own directory read-only) becomes "read-only unless the parent lists a subtree of it without `:ro` in the
namespace list". Without the attributes every child is given exactly what it is given today (INV-0411-4). A parent
can only narrow what it holds, never widen it.

1. **The extensions.** In `libc.a`, under a `posix-surface-added` block of the ADR that builds them:
   `posix_spawnattr_setnamespace_np(attr, const char *const *entries)` where each entry is `PATH` or `PATH:ro` and must
   be inside an entry of the parent's table with at least that access (else `EACCES` at `posix_spawn`);
   `posix_spawnattr_setgrants_np(attr, unsigned flags)` with `POSIX_SPAWN_GRANT_NET`, `_RANDOM`, `_WINDOW` (the
   parent's connection is shared to the child, ADR-0310's `ZPBX` flags) and `POSIX_SPAWN_MOVE_NET` (the parent's network
   connection is moved: the parent has none afterwards); a flag for a connection the parent lacks is `EACCES`; and the flag `POSIX_SPAWN_EMPTY_NAMESPACE` for `posix_spawnattr_setflags`. Without them `posix_spawn`
   behaves as today. `posix_spawnp` (a PATH search) and `posix_spawn_file_actions_addopen` join them, because GLib and
   other ports call them (ADR-0342's link probe decides).
2. **What each process is given:**

   | Process | Table | Grants | Descriptors |
   |---|---|---|---|
   | `browser` (UI) | its description's: `/usr/share/wpe` read-only, `/system/fonts`, `/system/tls`, its own directory (ADR-0341, `data browser as ~/.local/share/browser`), files the person gives on the line or through the chooser (ADR-0419) | `net web` (ADR-0324) until it moves it to the network process, random, window | its terminal's |
   | network process | `/system/tls` read-only, `~/.local/share/browser/network` read-write (cookies, cache, HSTS) | net (moved, `POSIX_SPAWN_MOVE_NET`), random | its IPC socket |
   | Web process | `/usr/share/wpe` and `/system/fonts` read-only; nothing else (no `/tmp`, no current directory) | random | its IPC socket, the ICU data descriptor if ADR-0417 item 3 applies |
   | a download | written by the UI process through the chooser's entry (ADR-0419) | | |

3. **The launcher.** `Source/WebKit/UIProcess/Launcher/zoraya/ProcessLauncherZoraya.cpp` (a patch in
   `ports/patches/wpewebkit/`): makes the socket pair (ADR-0412), sets `POSIX_SPAWN_EMPTY_NAMESPACE`, the table and
   grants of item 2 per process type, `adddup2` of the child's socket end to a fixed descriptor and that number on the
   command line, as the GLib launcher does; then closes its copy. The network process is spawned with
   `POSIX_SPAWN_MOVE_NET`, so after it starts the UI process has no network: a network process that dies cannot be
   restarted, and the browser says so and ends (the person starts it again). The helpers are the browser's own
   executable started with a role flag (ADR-0410 item 5). No environment but `LANG` and the engine's own
   variables reaches a helper (the environment policy of ADR-0320).
4. **Jobs.** Each helper is a child process of the UI process in the broker's job tree (ADR-0104), so the room's limits
   cover the whole browser and `top ROOM` lists them under it (ADR-0267). A helper that dies is restarted by WebKit; the
   UI process's death ends the helpers (their IPC peer closes).
5. **What the person sees.** The window's pills are the description's grants (ADR-0172): `web`, `/system/fonts`,
   `/system/tls`, `~browser`. `grants browser` prints them as for any program; it does not claim which process holds
   them, because nothing the shell trusts enforces the split: it is the browser's own code, checked by INV-0411-2 and
   INV-0420-2 and described in `help browser`. `top ROOM` shows the processes (ADR-0267).

### Effect of the vision

For target 2 this is the property that makes a browser fit for untrusted pages: what JavaScript runs in holds a socket
to its UI process and read-only fonts. A supervisor reads it in `top` and `grants`. Target 3: the person's chosen files
reach the UI process only. Networking: one process holds the grant. Threads: unchanged.

### Examples

```
$ top home
ROOM  JOB                    MEM      PROCS
home  browser                412M     3
        browser --network     38M
        browser --web        301M
```

A page's script that tries `fetch("file:///home/me/notes.txt")` fails in the Web process: its table has no such entry,
and the network process refuses `file:` URLs from a page.

## Consequences

- **Positive.** The content process has nothing to steal by construction; no filter, no namespace; the extensions also
  serve other multi-process ports (Python's `multiprocessing` with `spawn`, build tools).
- **Negative.** Four non-standard names; WebKit carries a Zoraya launcher; the UI and network processes are as trusted
  as the browser program, and the split between them is the browser's code, not a boundary the shell enforces; a
  network process that dies ends the browser.
- **Follow-up.** The extensions and their tests; the launcher patch; Option C as a later hardening if a boundary the
  shell enforces is wanted.

## Invariants

- **INV-0411-1.** A child started with `POSIX_SPAWN_EMPTY_NAMESPACE` has exactly the table entries listed (each no wider
  than the parent's), exactly the connections the grant mask names, and exactly the descriptors the file actions name.
  *Check:* `cargo test -p zoraya-posix` (the plan of a spawn from attributes, host test) and an in-QEMU C program that
  spawns itself with each combination and prints its table, `isatty`, and whether `connect` and `getrandom` work.
- **INV-0411-2.** The Web process has no network connection, no window channel, no writable table entry, and no entry
  outside `/usr/share/wpe` and `/system/fonts`. *Check:* a test step of the engine (`make test-browser`) whose page
  makes the Web process print its table and try `socket`/`connect`, and asserts the refusals.
- **INV-0411-3.** No engine process is created by `fork`. *Check:* the launcher test counts `snapshot_process` calls
  (zero) through the broker's statistics in the same step.

- **INV-0411-4.** A child started without the new attributes gets what it gets today: its parent's table with the
  parent's own directory read-only, and no network, random or window connection. *Check:* the existing spawn tests of
  `zoraya-posix` and ADR-0341's tests, kept unchanged and passing.

## Open questions

- Whether WebKit's network process needs a `/tmp` (it may write cache files); if so, a per-browser `tmpd` entry.
- Whether a helper description key is worth having before a second multi-process port exists.

## Revisit triggers

- A second engine (Ladybird) with more process types: the table of item 2 per type, perhaps Option C.
- A content process that must read a file the person chose (an upload): it goes through the UI process today; if the
  engine needs a descriptor in the Web process, the grant list gains one entry per upload.

## Architect's decision

Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request. As recommended. The changes to ADR-0287 item 3, ADR-0310 item 3 and ADR-0341 item 7 are accepted; each is recorded in the changed ADR by the foundation part that implements it (plan F4), with an Opus review.
