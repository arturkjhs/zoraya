# ADR-0169: The desktop's settings in the one configuration file

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0002, ADR-0010, ADR-0013, ADR-0050, ADR-0052, ADR-0053, ADR-0055, ADR-0097, ADR-0153, ADR-0155, ADR-0156, ADR-0157, ADR-0160, ADR-0161, ADR-0163, ADR-0164, ADR-0168, ADR-0171
- **Evidence:** CP-21, CP-23, DM-085, DM-086, DM-087, X-15, alternatives-postmortem.md#genode, desktop/survey.md#settings, desktop/survey.md#haiku

## Context
The desktop has settings: light or dark, the scale, the enabled keyboard layouts and the active one, later a wallpaper and shortcuts. ADR-0157 (accepted) decided the system's configuration: one file, `/config/settings`, of UTF-8 lines `NAME = VALUE` with ADR-0150's quoting, names `group.name` in lower case, every setting declared by the component that owns it in its description (ADR-0156) as `setting NAME TYPE DEFAULT TEXT` with the types `yes-no`, `number MIN MAX`, `text`, `path` and `choice A|B|C`, defaults never in the file, the `settings` builtin of the shell (which holds `/config`) as the command that lists, shows, checks and writes atomically, hand edits reported line by line and defaulted, no environment, and "GUI settings join as declared `gui.*` names". It also says "a change takes effect in the shell at once; other components read the file when they start", and leaves open "whether settings changed by hand should reach a running component (a reload message)". The task asks that the desktop's settings and the shell's be one source of truth; this ADR puts the desktop's keys into ADR-0157's file and decides only what ADR-0157 leaves to the desktop: which keys, who writes them from the GUI, and how a running compositor learns of a change.

The survey (`desktop/survey.md#settings`): GSettings describes every key in a schema with a type and a default, notifies on change, and says to write "only ever ... in response to explicit user action"; `rc.conf` is read after a defaults file, with `sysrc` to edit it; Haiku keeps per-panel settings files (`desktop/survey.md#haiku`). The catalog's failure modes: an opaque shared database parsed in the kernel (DM-086) and per-program text with no schema (DM-087); the candidate principles are typed, owned configuration (CP-23) and no implicit process-global configuration (CP-21, DM-085, X-15). Genode's own configuration migration was its "most delicate challenge" (`alternatives-postmortem.md#genode`). Measured: nothing. ADR-0157's follow-up makes its parser and writer a `no_std` crate, which the settings program uses.

## Decision drivers
1. One source of truth: `/config/settings` as ADR-0157 defines it; the `settings` builtin, the settings window and a text editor change the same lines and see each other's changes (the task's requirement, ADR-0157 driver 1).
2. Every key is declared by its owner with a type and a default, and an invalid value is never applied (ADR-0157 rules 3 and 5, CP-23, DM-087).
3. No component reads configuration implicitly; a client receives its appearance as a message, not by reading the file (CP-21, DM-085, X-15, ADR-0157 rule 7).
4. A change from the settings window takes effect without a restart where the owner can apply it (theme, motion, layouts), and the owner says so where it cannot (scale).
5. Small: the keys, their declarations, one message to the compositor; no service and no second file.

## Considered options

### Option A: Per-application text files (Unix `/etc` and dot-files; Haiku's per-panel files)
- **Description:** The desktop keeps its own file (or one per program) beside `/config/settings` (`desktop/survey.md#haiku`).
- **Evidence for:** No coupling to the shell's file; each owner validates its own.
- **Evidence against:** Driver 1 fails outright, against ADR-0157; DM-087's unchecked syntax per program.
- **Cost:** *(estimate)* Smallest now; the integration later.
- **Precedent:** Unix, Haiku.

### Option B: A typed settings service (GSettings and dconf; the registry)
- **Description:** A session service owns the file, serves `Get`, `Set` and `Subscribe` over a channel, validates against a schema crate, writes atomically and notifies subscribers; the shell's `settings` and the settings window both go through it.
- **Evidence for:** Drivers 2, 3 and 4 by construction; GSettings is the mature form (`desktop/survey.md#settings`); one writer, so no two programs race on the file.
- **Evidence against:** ADR-0157 makes the `settings` builtin the writer and gives `/config` to the shell; a service in front of the file would change an accepted ADR for no new capability, and would be DM-086's shared store in a user process. The first draft of this ADR was this option; it is kept as the record.
- **Cost:** *(estimate)* A 400-line service, a protocol, and a change to ADR-0157.
- **Precedent:** dconf, the Windows registry.

### Option C: ADR-0157's file and crate, the desktop's keys declared by their owners, the settings window as a second writer, and one message to the compositor
- **Description:** The compositor declares the `gui.*` settings in its description (ADR-0157 rule 3). The compositor reads `/config/settings` at start through a read-only connection to `/config` that `init` gives it (ADR-0153's vocabulary: a `read` entry), with ADR-0157's parser crate, and applies its keys. The settings window (ADR-0168) receives, from the panel's own table (the launcher decides, ADR-0157 rule 1) and not from a description (ADR-0153 rule 2), `/config` narrowed to the name `settings` with the rights an atomic replacement needs, `read`, `write`, `create`, `publish`, `sync`, `clone`, worded `replace settings`: a right set outside ADR-0153's seven kinds, stated as such; uses ADR-0157's writer crate (the same code the `settings` builtin uses: parse, replace one line, publish atomically over the old name, ADR-0010), and then tells the panel `SettingsChanged` over a channel to the panel that the settings program alone receives (ADR-0167 point 2); the panel re-reads the file through a `read` entry of its own and sends the compositor `ApplySettings` over its client-manager channel (ADR-0168); the compositor applies what it can live and sends every client a `Configure` with the new appearance (ADR-0160). The `settings` builtin changes the same lines; a running compositor learns of them the next time the settings window is used or the session starts, which is ADR-0157's rule.
- **Evidence for:** Drivers 1 to 5: nothing but ADR-0157's own file, syntax, declarations, crate and command, plus two messages. Two writers of one file are safe because each write is an atomic publish of a whole new file (ADR-0010, ADR-0157 rule 4): the last wins, and neither sees a torn file.
- **Evidence against:** A hand edit or a `settings` command while the session runs reaches the compositor only through the settings window or a restart (driver 4 is met for the GUI path only); the settings window holds `/config` with write rights, a second holder beside the shell; a list-valued setting (the enabled layouts) needs ADR-0171's list type, a change to ADR-0157.
- **Cost:** *(estimate)* The declarations, 100 lines in the compositor and the panel, the settings window's use of the crate.
- **Precedent:** `rc.conf` edited by both `sysrc` and hand (`desktop/survey.md#settings`); GSettings' "write only on user action".

### Option D: Configuration passed only at launch (Genode's static configuration, CP-21 taken literally)
- **Description:** Every setting is an argument or handle at spawn; to change one, restart the session (`alternatives-postmortem.md#genode`).
- **Evidence for:** No run-time state, no second writer; driver 3 by construction.
- **Evidence against:** Driver 4: switching the theme or the layout restarts everything; Genode's own migration shows static configuration is not simpler to evolve.
- **Cost:** *(estimate)* None beyond the launcher.
- **Precedent:** Genode's static `init` configuration; Hubris.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Keys**, declared as ADR-0157 rule 3 prescribes, in the compositor's description (`gui.*`); names follow ADR-0157's `group.name` with `-`:

  | Name | Type | Default | Owner | Applied |
  |---|---|---|---|---|
  | `gui.theme` | choice `light\|dark` | the chosen direction's `appearance` | compositor; clients through `Configure` | live |
  | `gui.scale` | choice `auto\|1\|2` | `auto` (ADR-0164) | compositor | at the next session, or live with every window re-created (ADR-0164) |
  | `gui.motion` | yes-no | `yes` | compositor; clients through `Configure` | live |
  | `gui.wallpaper` | text | `''` (none) | compositor | live |
  | `gui.keyboard-layouts` | list of choice `us\|ua\|ru\|cz` (ADR-0171's list type), 1 to 4 items, unique, in switching order | `'us'` | compositor | live |
  | `gui.keyboard-layout` | choice `us\|ua\|ru\|cz`, which must be a member of the list (the compositor checks and reports; the first member is used otherwise) | `us` | compositor | live |

  There is no font setting: one pair of font families ships with the chosen direction (ADR-0162). The list type is ADR-0171's proposed change to ADR-0157; until it is accepted the key cannot be declared, since ADR-0157's five types have no list.
- **Defaults** are in the declarations, never in the file (ADR-0157 rule 1); an absent key is its default.
- **Who reads.** The compositor holds a `read` entry on `/config` (from `init`, ADR-0097) and reads the file once at start with ADR-0157's crate; it applies its keys and reports bad lines as ADR-0157 rule 5 says (`compd: /config/settings line N: PROBLEM` on the console). Clients never read the file: theme and motion reach them in the `appearance` record of ADR-0160's `Configure`, scale in its `scale` field; the panel holds a `read` entry too and reads the file only when the settings window tells it to.
- **Who writes.** The `settings` builtin (ADR-0157 rule 4) and the settings window (ADR-0168), both with ADR-0157's writer crate and both atomically (a new file published over the old, ADR-0010); nothing else by launch record; a hand edit, including a text editor saving the file through the chooser (a `create or replace` write, not an atomic publish), is what ADR-0157 rule 5 provides for: reported and defaulted line by line. The settings window's launch record (ADR-0167, ADR-0153's `Startup`) carries `/config` narrowed to `settings` with `read`, `write`, `create`, `publish`, `sync`, `clone` from the panel's own table; no other record carries `/config` with any writing right.
- **The settings window** has one page per group and a control per key, bound to its declaration (type, default, text); every change is written at once (GSettings' rule, `desktop/survey.md#settings`), then `SettingsChanged` goes to the panel. The compositor applies theme, motion, wallpaper and layouts live and announces "takes effect when the session restarts" for scale, unless the architect accepts ADR-0164's live re-creation.
- **Messages.** `SettingsChanged` (application to panel, no payload) and `ApplySettings { theme, motion, wallpaper, layouts, layout, scale }` (panel to compositor, on the client-manager channel of ADR-0168, ADR-0052 ordinals). The compositor validates the values again against its declarations: the panel is trusted, the file is not.

**Tensions with accepted ADRs.** *ADR-0157*: none in its rules; its open question on reaching a running component is answered for the GUI path only (the settings window), and its "other components read the file when they start" stays true for every other change; two writers of one file is new and relies on rule 4's atomic publish. *ADR-0153 rules 1 and 2*: the settings window receives `/config` with writing rights from the panel's own table, not from a description (rule 2 forbids a description naming a fixed path), with a right set outside the seven kinds; the launch record shows it (ADR-0167 point 2); `settings` is a base-system program of the image. *ADR-0155 rule 8* ("the shell owns `/config`"): the settings program is a second holder. *ADR-0097*: the compositor and the panel gain a `read` entry on `/config`; no new service. *ADR-0002 rule 7*: nothing feeds the environment from the file (ADR-0157 rule 7).

### Effect of the vision
For target 2 an agent's session has whatever `/config` its launcher gives (ADR-0157); its applications hold no `/config` entry and see their appearance in `Configure`. Target 3 wants per-user settings, which is a `/config` per session, no change here. Networking: `net.*` names, ADR-0157's. Threads: none.

### Deferred to stage 6
Nothing here depends on POSIX.

## Consequences
- **Positive.** One file, one syntax, one crate, one command; the shell and the desktop agree by construction; clients need no configuration access; no service.
- **Negative.** A `settings` command or a hand edit during a session reaches the compositor only through the settings window or a restart; the settings window is a second holder of `/config` with write rights; the layout list depends on ADR-0171.
- **Follow-up.** The `gui.*` declarations in the compositor's description; `SettingsChanged` and `ApplySettings`; the settings window.

## Invariants
- **INV-0169-1.** Every key the compositor or the panel reads from `/config/settings` is declared in the compositor's description with a type, a default and a text, and the parser accepts exactly the values the declaration allows; `gui.keyboard-layouts` is ADR-0171's list of the choice of ADR-0163's names and `gui.keyboard-layout` one of them. *Check:* a host test over the declarations and ADR-0157's parser with valid and invalid values per key, and a host test that every key read in the compositor's and the panel's code names a declared setting.
- **INV-0169-2.** An invalid value never reaches an owner: at start the compositor applies the default and reports the line; an `ApplySettings` with an invalid value is refused with `InvalidArgument` and changes nothing. *Check:* a host fuzz of the compositor's settings reader over arbitrary files and of `ApplySettings` over random records, asserting the applied state.
- **INV-0169-3.** A write from the settings window is atomic and keeps every other line: after it, the file holds the new value whole or the old value whole, and every line the window did not change, comments included, is unchanged (ADR-0157's INV-0157-1 applied to the second writer). *Check:* the host property test of INV-0157-1 run over the settings window's writer, and a host test that interleaves a `settings` write and a window write and asserts one of the two files, never a mixture.
- **INV-0169-4.** No launch record the panel builds carries `/config` with a writing right except the settings window's, and no client holds a channel on which `ApplySettings` is accepted. *Check:* a host test over the panel's launch records (INV-0168-1) and a host test sending `ApplySettings` on a client channel and asserting `Unsupported`.

## Open questions
- Whether the compositor should re-read the file on a `settings` command from the shell (a reload message from `zosh` to the panel would need a channel `zosh` does not hold); ADR-0157's open question, left to it.
- Whether `gui.scale` should be applied live (ADR-0164's re-creation) or at the next session.

## Revisit triggers
- A second user or session.
- A third writer of the file: Option B's service becomes worth its cost.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
