# ADR-0168: The first applications

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0003, ADR-0043, ADR-0053, ADR-0055, ADR-0096, ADR-0097, ADR-0108, ADR-0160, ADR-0161, ADR-0162, ADR-0163, ADR-0165, ADR-0167, ADR-0169
- **Evidence:** CP-01, CP-17, CP-25, CP-27, DM-079, GR-005, alternatives-postmortem.md#haiku--beos, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#genode, desktop/survey.md#haiku, desktop/survey.md#plan-9s-rio, desktop/survey.md#gnomes-hig-and-libadwaita

## Context
Stage 7's criterion is a desktop a person enjoys using. Stage 5 has one terminal started by `init`. The question is which applications stage 7 builds, in what order, and which wait. Haiku ships a file manager (Tracker), a panel (Deskbar), Terminal, StyledEdit, ShowImage and about thirty more, grown over two decades (`desktop/survey.md#haiku`, `alternatives-postmortem.md#haiku--beos`). rio has terminals only, and the window system is the program launcher (`desktop/survey.md#plan-9s-rio`, `alternatives-postmortem.md#plan-9--inferno`). GNOME's core applications are each "one thing done well" (`desktop/survey.md#gnomes-hig-and-libadwaita`). Genode's parent tree launches components with explicit resources (`alternatives-postmortem.md#genode`). The architect's scope rule is CP-27: hold the list by decision. Measured: nothing. Assumed: the ordering below by what each application needs from the others.

## Decision drivers
1. Each application exercises one new ADR and is usable alone, so stage 7 has a visible result at every step (CP-17's isolation applies: each is a process `init` or the panel can restart).
2. The launcher is where a person grants things to applications (ADR-0167, CP-01, GR-005); it comes first because every other application is started by it.
3. The terminal and `zosh` remain the heart (TASK-18); the desktop must not replace them with something weaker.
4. Scope held by decision (CP-27): what waits is written down.
5. No application can observe another (DM-079); the panel is the one program that sees every window, through the compositor's manager side.

## Considered options

### Option A: A full suite (GNOME core applications, Haiku's set)
- **Description:** Launcher, file manager, terminal, editor, image viewer, settings, calculator, clock, media player, browser, mail.
- **Evidence for:** A desktop people recognise as complete (`desktop/survey.md#haiku`).
- **Evidence against:** Haiku took decades with volunteers (`alternatives-postmortem.md#haiku--beos`); a browser or mail needs networking (out of scope); CP-27.
- **Cost:** *(estimate)* Years.
- **Precedent:** Haiku, GNOME.

### Option B: Terminals only, the window system as launcher (Plan 9's rio)
- **Description:** New windows are terminals; everything else is a command (`desktop/survey.md#plan-9s-rio`).
- **Evidence for:** Smallest; `zosh` is already the way to grant things (TASK-18).
- **Evidence against:** The architect asked for a desktop with a file chooser and settings; a person cannot grant a file to a program by pointing at it; nothing shows Ukrainian file names outside a terminal.
- **Cost:** *(estimate)* The terminal's move to the new text crate and nothing else.
- **Precedent:** Plan 9.

### Option C: Six applications in dependency order, each small
- **Description:** (1) the panel, which is launcher, window list, layout indicator and session supervisor; (2) the terminal brought to ADR-0162's text and ADR-0165's loop; (3) the chooser (ADR-0167); (4) a file manager; (5) a text editor; (6) settings (ADR-0169). An image viewer waits for an image decoder.
- **Evidence for:** Drivers 1 to 5. Each step needs only the ones before it; each exercises one ADR; together they are the mockups' five screens.
- **Evidence against:** Six programs is six times the toolkit's rough edges found late; the file manager holds the root and is the second trusted program beside the chooser.
- **Cost:** *(estimate)* 800 to 2,000 lines each with tests.
- **Precedent:** Haiku's Deskbar, Tracker, Terminal, StyledEdit as a subset (`desktop/survey.md#haiku`).

### Option D: One application that does everything (acme on Plan 9)
- **Description:** A single text-and-command environment with windows inside it.
- **Evidence for:** One program to write; a coherent model.
- **Evidence against:** It is a different design from the one the architect described (windows, a chooser, settings), and it does not exercise the compositor's windows.
- **Cost:** *(estimate)* As large as C and less reusable.
- **Precedent:** acme (`alternatives-postmortem.md#plan-9--inferno` describes Plan 9's model, not acme in particular).

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

1. **`panel`.** Its shape is the direction's, by ADR-0161's `desktop.layout`. With `taskbar` (the first mist, quiet and night): a bar along the bottom of the display with a launcher menu listing the installed applications from a directory the launcher holds (`/system/apps`, images loaded with ADR-0108's `image_create` or ADR-0043's ramdisk lookup), a window list with hidden windows (restores on click, ADR-0160), the keyboard layout indicator (ADR-0163) and a clock. With `dock` (the second mist): two bar windows, a thin bar at the top (`bar.height`) carrying the system menu at the left, the keyboard layout and the clock at the right, and in its middle the trusted mark of an open sheet (ADR-0167), and a floating dock centred in a reserved strip at the bottom, translucent over the blurred wallpaper (ADR-0161), with the installed applications as `dock.icon`-pixel icons on tiles and a dot under each that is running; the dock has no text window list: windows are listed by the switcher (ADR-0160, hidden ones included), and a press on a running application's icon raises its windows, or restores them when hidden `[assumption]`. In both shapes the panel is the same program with the same channels; only its windows differ. Under ADR-0172 (the `clear-field` mockups, the architect's choice of 2026-11-03) the panel launches every application into a *room*: the dock is ADR-0161's dock B, with every application that has a window as an icon on a tile of neutral material (the agent included), one dot per room under a running application in the room's colour, the untrusted application ringed, and after a divider the rooms as chips, the current one filled, with `+` for a new room; a press on a chip enters its room (`room NAME`), `+` opens the overview's form; the hovered icon's name and `zosh` line stand in a tooltip over the dock, and in the show-commands mode every icon and chip has its line under it (`edit FILE`, `room project`), the only state in which the dock grows; the bar then carries no room badge and keeps the system menu, the trusted mark in the middle while a sheet or the show-commands mode is open, the keyboard layout and the clock; the dock's strip is reserved in the dock's current height, and ADR-0172's fields end above it in every mode (they are recomputed when the dock grows for the show-commands mode and when it shrinks back); the dock's only hue is a room's, and it is drawn from the panel's launch records and ADR-0172's room table, never from an application; the rooms overview (System+R, in the trusted material) lists the rooms with their windows and grants, enters one, and makes one from a form whose `zosh` line is shown beside it; `AddClient` carries the room, its `untrusted` flag, its colour slot and the launch record's entries in ADR-0153's words, so that the compositor can draw them (ADR-0172 point 4); the tooltip of the launch record becomes the hover card of the window's pills, drawn by the compositor from the same record. The window list of the `taskbar` shape groups by room too. It holds a *client manager* channel to the compositor, a second manager endpoint carrying `AddClient`, `ApplySettings` (ADR-0169) and the layout notification (ADR-0163), which `init` creates and hands over (a change to ADR-0093's one manager channel, listed below); `init` keeps the driver manager (`AttachDisplay`, `AttachInput`, ADR-0097). So the panel is the program that starts applications and gives each its compositor channel, its chooser channel (ADR-0167), its data directory and what was dropped on it, in a sub-job of its own (ADR-0003). Its window is ADR-0160's *bar* kind: frameless, in a reserved strip, never covered, moved or closed by clients. Every launch is explicit: the panel's launch record for an application lists its handles, so the panel can show "this program has: its data folder, the file X" in a tooltip, which is the GUI's form of the shell's "what a program gets" (TASK-18).
2. **`term`** keeps its protocol and grid (ADR-0096) and moves to `zoraya-text` (ADR-0162) and `zoraya-app` (ADR-0165); System+Enter in the panel starts another. **The terminal's title**, reconciled with ADR-0154 and ADR-0096: ADR-0096 ignores `OSC`, so the title cannot travel in the byte stream (and ADR-0154 rule 8 keeps the editor's output to its subset). Instead `zoraya-proto-term` gains one message out of band, `Title { text }` (a new ordinal, ADR-0052), which `zosh` sends at start and after each `cd` with its current directory as the prompt's `{cwd}` shows it (ADR-0157's `~` for home); the terminal sets its window title to `zosh — <text>` with `SetTitle` and never from output. Under ADR-0172 the same channel carries `Room { name }` when the shell enters a room, which the terminal passes to the panel for the frame's badge (ADR-0172 point 4). Control stays out of band (CP-25); nothing a program prints can set the title, so this is a smaller claim than on every Unix terminal. When `zosh` exits, the panel starts a new shell in the same terminal (ADR-0155's Decision had `init` do it in the window's terminal; the panel is the launcher now, listed below).
3. **`chooser`** (ADR-0167).
4. **`files`.** A file manager: a window per directory, list view with name, size and time (ADR-0014), icon per kind from the name's extension `[assumption: no file types in stage 7]`, open on double click (which launches the application registered for the extension through the panel with the file as its grant), new folder, rename, delete with confirmation, and drag of a file onto an application window as a grant (ADR-0170 once decided; until then, "Open with" through the panel). It holds the root like the chooser and is a system program; it shows Ukrainian and Czech names (ADR-0162). It runs in the `home` room (ADR-0172), and a drag from it into a window of another room is the grant the drop card shows in words (`+r ~/Документи/звіт-2026.txt edit`).
5. **`edit`.** A text editor for UTF-8 files: multi-line text view with selection, undo, find; open and save through the chooser; "unsaved changes" on `CloseRequested`. The first program that holds nothing but what it was given.
6. **`settings`** (ADR-0169): appearance (theme, scale, motion), keyboard (the enabled layouts and which is active, switched with System+Space), and about; each change written with ADR-0157's crate and announced to the panel (ADR-0169). The mockup shows the appearance page with the keyboard section below it. Its layout follows ADR-0161's `settings.style`: `plain` is a page of controls under headings; `cards` (the second mist) groups the controls in rounded cards (`bg.window` on the grey `bg.panel` ground, `radius.control` doubled) of rows with the label at the left and the control at the right, thin `border` separators between rows, a `text.title` page title and `text.heading` section headings with `text.caption` descriptions, which `zoraya-ui` (ADR-0165) provides as one group widget so that every settings page looks the same. Under ADR-0172 each page holds only its own parameters (the `clear-*` mockups' appearance page: theme, scale, motion, and "wallpaper follows the time of day", a `gui.wallpaper-by-time` yes-no key ADR-0169 would gain, which changes the wallpaper alone, never the interface), a `Rooms` page lists the rooms as the overview does, and the page's keys are shown as the `settings` line under the cards, so that the setting has its text.

**What waits**, with the reason: an image viewer (needs a PNG decoder, which is a total parser of its own: a follow-up task once the mockup tool's encoder has a decoder beside it); a calculator and a clock application (nothing new, when there is time); a clipboard manager (ADR-0170); a browser, mail, media (networking and codecs, out of scope); "open with" for POSIX programs started from the desktop (stage 6's namespace table from the launcher, ADR-0167's deferred section).

**Tensions with accepted ADRs.** *ADR-0093*: its one manager channel becomes two endpoints, the driver manager in `init` and the client manager in the panel; the compositor accepts `AddClient`, `ApplySettings` and the layout notification on the second and the driver methods only on the first (a change to ADR-0093's manager side, by new ordinals under ADR-0052). *ADR-0097*: `init` starts `term` and `zosh` today; in stage 7 `init` starts the compositor, the chooser and the panel, and the panel starts terminals (`term` with `zosh`, which keeps the volume at slot 5 and the console to write only, so INV-0097-3 holds through the panel's launch record) and applications, so `init`'s GUI table shrinks and its restart rules for `term` and `zosh` move to the panel: a dead terminal is reported in the window list and not restarted; a dead panel is restarted alone by `init`, three times, as a driver is, and its launched applications keep running under their own jobs. *ADR-0096*: unchanged in its rules; `Title` is a new ordinal of its protocol, and `OSC` stays ignored. *ADR-0154*: unchanged; the shell sends `Title` as a message, not as output bytes, so INV-0154-2 holds. *ADR-0155*'s Decision, "in GUI mode `init` does the same in the window's terminal" (a new shell after `exit`), moves to the panel with the rest of terminal launching: the panel starts the new shell with the same launch record and the same terminal; INV-0155-5's wording ("`init` starts a new shell in the same terminal ... and the same in the GUI script") reads "the panel starts" in GUI mode, and its GUI check moves with it. *ADR-0156*: every application of this ADR ships a `.cmd` description, which the panel reads for its name and operand kinds (ADR-0167 point 5). *ADR-0053*: the panel is a launcher in the parent-routed model, as Genode's child launchers are (`alternatives-postmortem.md#genode`).

### Effect of the vision
For target 2 the panel is the sandbox launcher ADR-0053 foresaw: an agent is an application with a launch record listing what it holds. Target 3 adds applications by adding directories, not by changing the panel. Networking and threads: none.

### Deferred to stage 6
A POSIX program launched from the desktop runs in the broker's session with a terminal window (ADR-0105); the panel starts it as `zosh` would, through the broker. Not in stage 7's first six.

## Consequences
- **Positive.** Every step is a usable program; the panel is the one place grants are made and shown in the GUI.
- **Negative.** Two trusted programs hold the root (chooser, files); no image viewer in stage 7.
- **Follow-up.** The six programs; the PNG decoder; ADR-0170.

## Invariants
- **INV-0168-1.** Every application the panel starts receives exactly the handles its launch record lists, the record names the room it was launched in (ADR-0172), and the record is shown to the person. *Check:* host test of the panel's launch logic against a fake `spawn` recording the handle list; the GUI test opens the tooltip of a running application and reads it.
- **INV-0168-2.** A connection to the volume's root with its full rights is held only by `init`, the panel, the chooser, `files` and the shell in a terminal (ADR-0097's slot 5); every other program holds only what its launch record lists: its data directory, `/system/fonts` read-only, what the person gave it, and for the settings program `/config` narrowed to `settings` (ADR-0169). *Check:* host test enumerating the launch records of every application in `/system/apps` against this list; in-QEMU test that `edit` cannot reach a name outside its data directory (it has no other directory connection to try).
- **INV-0168-3.** The panel's window cannot be covered, moved or closed by any client. *Check:* host test of the compositor's stacking with the panel as ADR-0160's bar kind (INV-0160-7).

## Open questions
- Whether the panel and `init`'s session table should be one program (ADR-0097 Option C's session launcher): the panel is that launcher for applications; `init` keeps drivers.
- File types by extension is an assumption; a type registry is a later ADR.
- *(Check note, stage 7d, ADR-0001 rule 6.)* INV-0168-2's check waits for `/system/apps` and its launch records and for
  `edit` (7e, 7f): in 7d the panel launches only terminals, whose shell holds, by ADR-0242's accepted condition, its
  room's grants (`home`'s for the first terminal and for `home`) and not the volume's root. INV-0168-1's GUI
  half reads the launched shell's `slots` (exactly its record's handles) and shows the dock's tooltip in a screendump
  (`tests/gui/rooms.script`); the hover card of the window's pills waits for the hover card (ADR-0172 point 5).

## Revisit triggers
- An image decoder: add the viewer.
- Networking: the browser question.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
