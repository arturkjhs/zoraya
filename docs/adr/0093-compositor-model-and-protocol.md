# ADR-0093: Compositor model and protocol

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0006, ADR-0050, ADR-0051, ADR-0052, ADR-0053, ADR-0056, ADR-0072, ADR-0090, ADR-0092, ADR-0094
- **Evidence:** DM-079, DM-080, DM-081, DM-082, DM-083, DM-084, DM-160, GR-016, CP-24, CP-14, X-11, X-5, alternatives-postmortem.md#plan-9--inferno

## Context
Several programs share one screen and one keyboard. Something must decide what is drawn where and who gets which key. DM-079 is what happens when that something trusts its clients: any X client reads every other client's pixels and keys and can send them input. GR-016 is the answer Wayland gave, and CP-24 makes it a candidate principle. ADR-0051 says shared memory is hostile input that can change between reads (DM-160). Zoraya has channels (ADR-0050), memory objects (ADR-0051) and typed protocols (ADR-0052), so the question is the shape on top of them. Measured: nothing on Zoraya.

## Decision drivers
1. No client can read another's pixels or input, or make input (DM-079, DM-082, CP-24).
2. Shared memory is read once and never trusted (DM-160, ADR-0051).
3. A hostile or hung client cannot stop the compositor or other clients (CP-14).
4. What is shown of a client is what it committed: the compositor never shows pixels a client has not committed (DM-083). A client that writes the rectangle it is committing while the compositor copies it can still tear its own window.
5. Small and local: no remote display at this stage (DM-080).

## Considered options

### Option A: X11 (Linux baseline, older)
- **Description:** A server that clients ask to draw, in one global window namespace, with properties, grabs and `SendEvent`.
- **Evidence for:** Network transparency (DM-080), and every window manager or screenshot tool is an ordinary client.
- **Evidence against:** DM-079 and DM-082: every client reads and injects. Server-side rendering froze policy in the protocol (DM-080, X-11).
- **Cost:** *(estimate)* Large, and its security model cannot be retrofitted.
- **Precedent:** X11.

### Option B: Wayland on DRM/KMS (Linux baseline, current)
- **Description:** Clients render into buffers they share with the compositor (shared memory or DMA-buf), attach and commit damage, and get a frame callback; the compositor owns input and routes it (GR-016).
- **Evidence for:** Drivers 1, 3, 4 (GR-016), proven across Linux desktops.
- **Evidence against:** The compositor composes from the client's buffer whenever it recomposes, so a client that writes outside a commit can change what is shown; Wayland relies on a release protocol and client discipline for that. A client writing the rectangle being read tears its window in both models. Its minimal core fragmented into extensions (DM-084).
- **Cost:** *(estimate)* As Option C, plus a buffer release protocol.
- **Precedent:** Weston, Mutter, KWin.

### Option C: Wayland's shape on Zoraya's objects, with a copy at commit
- **Description:** A client creates a memory object for its surface and sends it once. To show a frame it commits a damage rectangle; the compositor copies those pixels into a copy of its own, composes from its copies into the scanout, flushes, and replies. The reply is the frame callback.
- **Evidence for:** Drivers 1 to 5. The copy is ADR-0051's rule applied to pixels, so the client may draw at any time and the compositor never shows pixels the client did not commit.
- **Evidence against:** One extra copy of each damaged pixel (X-5), and the compositor's memory grows with every surface: up to one copy per surface.
- **Cost:** *(estimate)* 900 lines of host-tested logic, a 300-line process, a protocol crate.
- **Precedent:** Wayland's commit model (GR-016), with a copy instead of a release.

### Option D: Windows as servers of draw requests (Plan 9's rio and devdraw)
- **Description:** Clients send drawing operations to a server that owns all images; `rio` multiplexes the display and gives each window its own devices (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence for:** Each window sees only its own files, so isolation is by construction; no shared memory.
- **Evidence against:** Every drawing operation is a message and server code (DM-081's lesson is about where that code runs, and here it would run in the compositor). A terminal draws glyphs, which this makes a protocol of its own.
- **Cost:** *(estimate)* A drawing library in the server.
- **Precedent:** Plan 9.

## Recommendation

### Decision
The architect accepted Option C with one change (2026-11-02): sending a frame to the display driver does not block
the compositor. One flush is in flight at a time. While it waits, the compositor goes on taking commits and input, and
replies to commits once they are copied and composed. When the driver answers, the compositor sends the newest frame:
the union of everything composed meanwhile, so the frames in between are skipped. A driver that misses the deadline,
2 s `[assumption]`, is dropped, and the compositor tells `init` with a `DisplayHung` message on the manager channel;
`init` restarts the driver as if it had died (ADR-0097). This replaces "Commit" and "Driver calls block" below where
they differ. Attaching a new display driver (`Info`, `Attach`) still waits for it, with the same deadline.

### Recommendation as written
Recommended: **Option C**. It is a recommendation, not a decision.

- **Connections.** A client holds one channel end to the compositor, which only its launcher can give (ADR-0053): the compositor's manager, `init`, adds a client by sending the server end over its manager channel. There is no listening name.
- **Messages** (`zoraya-proto-gui`). Client to compositor: `Display` (reply: width, height), `Create { width, height }` with one memory object, `Commit { x, y, width, height }`. Compositor to client: replies with a status, and events: `Key`, `Pointer`, `Focus` (ADR-0094). Each event carries the count of events dropped for this client before it. Manager to compositor: `AddClient` with one channel end, `AttachDisplay` and `AttachInput` with one driver channel each (ADR-0097).
- **Surfaces.** One per connection. Width and height from 1 to the display's. The object's type is checked (ADR-0072), and it is mapped read-only for `width × height × 4` bytes rounded up to pages: a smaller object fails the mapping and is refused with `BadBuffer`. The compositor allocates its own copy with a fallible allocation; failure is `NoMemory`. At most 8 connections `[assumption]`: a ninth is closed at once.
- **Commit.** A rectangle inside the surface and not empty, or `BadRange` and nothing changes. The compositor copies those rows once into its copy, composes the screen rectangles that changed, sends one `Flush` to the display driver for the union of a round's damage, and then replies to every commit of the round. One commit at a time per connection: the compositor does not read a connection again until it has replied, so a client that floods commits waits for its own replies, and each round reads at most one message per connection.
- **Buffer lifecycle.** The client owns its memory and may write it at any time. What it wrote before `Commit` is what the copy can hold; what it writes during the copy can tear only its own window. The compositor composes only from its copies, never from client memory.
- **Placement.** The compositor places windows: the first at a fixed origin, later ones cascaded. A client cannot position, raise or resize its window, so it cannot cover another to spoof it (DM-082). Windows have a one-pixel frame, coloured for the focused one, and no other decoration.
- **Pointer.** Drawn by the compositor on top of the composition, from the tablet's coordinates.
- **Events.** Sent with `channel_write`; `WouldBlock` drops the event and counts it for that client (ADR-0094). The compositor never waits for a client.
- **What no client can do.** There is no message that returns pixels, names another client, creates input, or reads the display. A client's memory is mapped read-only by the compositor, so the compositor cannot write into it either.
- **Without a display.** If the display driver goes, the compositor goes on accepting commits into its copies and replies to them; when a new driver is attached it attaches the scanout again and flushes the whole screen.

**Driver calls block.** Each round's `Flush`, and the `Info` and `Attach` of a new display driver, wait for the driver's reply, with a 2 s deadline. A hung driver therefore stops input routing and every client for up to 2 s before the compositor drops it, against driver 3 for the driver (not for clients); and a driver slower than 2 s to come up is dropped and costs a restart. A flush that does not block, with replies matched by transaction, would remove this; it is follow-up work.

**Tensions with accepted ADRs.** *ADR-0056* is proposed, so the compositor's memory per client is bounded only by the limits above.

### Effect of the vision
For target 2 this is the model that keeps an agent's GUI from reading or driving another's, provided the agent gets a client channel and nothing else. Capture, automation and accessibility, which an agent supervisor will want, are left to separate capabilities the compositor would grant (CP-24, GR-016's open problem), and nothing here precludes adding them as new manager-side methods. Target 3 wants zero-copy buffers and a release protocol for large windows; moving later adds a method and changes when the compositor reads client memory, at a modest cost `[estimate: 200 lines]`, because clients already commit damage. Networking: a remote display would be a client that relays, not a protocol property (DM-080). Threads: one client may share its connection among its threads only by its own locking.

### Deferred to stage 6
Nothing here depends on POSIX.

## Consequences
- **Positive.** Isolation by construction, and composition from memory no client can change.
- **Negative.** A copy per damaged pixel; memory per surface in the compositor; no client-side positioning, resizing or decorations.
- **Follow-up.** Resizing, several surfaces per client, capture as a granted capability.

## Invariants
- **INV-0093-1.** A client never receives a key while another surface has focus, nor a pointer event outside its own surface. *Check:* host tests of the routing logic, and an in-QEMU test with two clients.
- **INV-0093-2.** The client protocol has no message that carries pixels or another client's events to a client, or creates input. *Check:* host test over the protocol crate's ordinals, and an in-QEMU test sending the input and manager ordinals on a client channel and seeing `Unsupported`, with no event at the other client.
- **INV-0093-3.** A commit outside its surface, a surface larger than the display, and a memory object smaller than its surface are refused, change nothing, and leave the compositor serving others. *Check:* host tests, and the same cases in-QEMU against the real compositor.
- **INV-0093-4.** The compositor reads client memory only while copying a commit, once per pixel, and composes only from its own copies. *Check:* host test with a client buffer that changes on every read, asserting that the screen equals one consistent copy; and review.
- **INV-0093-5.** A client that never reads its channel, or floods commits, does not stop the compositor serving others. *Check:* host test of the loop, and an in-QEMU test with such clients beside a well-behaved one.
- **INV-0093-6.** A display driver that does not answer a flush stops neither input nor replies to commits; at most one flush is in flight; at the deadline the driver is dropped and the manager told. *Check:* host tests with a driver that never answers and with frames composed during a flush, and an in-QEMU test with a display driver that never answers a flush.

## Open questions
- Copy cost per frame under TCG: measured by this task (compositor CPU time per frame).
- Whether one commit in flight per client limits a fast client: the frame rate is measured.

## Revisit triggers
- A client whose frames are dominated by the copy.
- A request for capture or synthetic input.

## Architect's decision
