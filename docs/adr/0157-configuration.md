# ADR-0157: Configuration: one settings file, a command to edit it, and the shell's start file

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0002, ADR-0053, ADR-0055, ADR-0150, ADR-0153, ADR-0156
- **Evidence:** DM-085, DM-086, DM-087, CP-21, CP-23, shell/survey.md#freebsd-sh-and-base-system, shell/survey.md#fish

## Context

Nothing in Zoraya is configured today: `init` and `zosh` have compiled-in behaviour. The shell now needs a prompt,
history, suggestions, colour and a command path (ADR-0154), and a person needs a place for functions they always want.
Zoraya has no users or uids (ADR-0002 rule 6): there is no `~user`, no `/etc/passwd` of its own, and no per-user
dotfiles. What a process sees is what its launcher gave it (ADR-0053). The task asks for configuration "in the spirit
of FreeBSD's `rc.conf`".

**What is known.** FreeBSD keeps system configuration in one file of `name=value` lines that only overrides a
defaults file, and edits it with `sysrc`, which can list non-defaults and check without changing
(`shell/survey.md#freebsd-sh-and-base-system`). Per-program syntaxes in `/etc` make errors late and editing by tools
unsafe; most misconfigurations are values a checker could catch (DM-087). A central typed store fixes some of that
and brings opaque shared state (DM-086). Environment variables as configuration are a mistake (DM-085, CP-21).
Configuration should be typed and owned (CP-23). fish keeps shared settings in a file it says not to edit by hand
(`shell/survey.md#fish`).

## Decision drivers

1. One readable file of simple settings, editable by hand and by a command, with the same result
   (`shell/survey.md#freebsd-sh-and-base-system`).
2. Every setting declared with a type, a default and a description, so mistakes are caught when made (DM-087, CP-23).
3. No configuration through the environment, and no setting a library reads behind a program's back (CP-21, DM-085).
4. No user identity: where configuration lives is decided by what the launcher gives (ADR-0002, ADR-0053).

## Considered options

### Option A: Shell dotfiles and `/etc` files (Unix baseline)
- **Description:** `~/.profile`-style start files that set environment variables; each service its own file in
  `/etc` with its own syntax.
- **Evidence for:** Familiar; flexible.
- **Evidence against:** DM-085 and DM-087; no `~` without users.
- **Cost:** Small now, growing.
- **Precedent:** Unix.

### Option B: `rc.conf` and `sysrc` (FreeBSD)
- **Description:** One file of `name=value` in shell syntax, a defaults file, a command that edits safely.
- **Evidence for:** Drivers 1 and 4; proven in FreeBSD for decades (`shell/survey.md#freebsd-sh-and-base-system`).
- **Evidence against:** Values are untyped strings; an unknown name is silently kept (driver 2).
- **Cost:** *(estimate)* Small.
- **Precedent:** FreeBSD.

### Option C: B with declared, typed settings
- **Description:** As B, but every setting is declared (name, type, default, description) by the component that owns
  it, in its description (ADR-0156); the command refuses unknown names and bad values; hand edits are checked when
  read, with file and line.
- **Evidence for:** Drivers 1 to 4; DM-087's finding that most misconfigurations are checkable values.
- **Evidence against:** A setting must be declared before it can be used; the schema is spread over components'
  descriptions.
- **Cost:** *(estimate)* A parser and a writer that keeps comments; about 500 lines.
- **Precedent:** typed stores (DM-086) without their binary format. The postmortem records no configuration system
  of a non-Unix system; the registry (Option D, DM-086) is the non-POSIX precedent.

### Option D: A typed key-value store (registry-like)
- **Description:** A service holds typed settings; programs query it over a channel.
- **Evidence for:** Typed, concurrent-safe (DM-086).
- **Evidence against:** Opaque, needs tools to read, a service to trust (DM-086); not a file a person edits.
- **Cost:** *(estimate)* A service and a protocol.
- **Precedent:** Windows registry, dconf.

## Recommendation

Recommended: **Option C**. It is a recommendation, not a decision.

1. **Where.** `/config` on the volume holds the configuration of whoever is given it: `/config/settings`,
   `/config/start.zosh` and `/config/history`. There are no users: the launcher of a session decides which directory
   is that session's `/config` (`init` gives the volume's own; a sandbox launcher can give another). Defaults are
   never in `/config`; they are in the descriptions of the components that declare the settings (ADR-0156), so a
   missing or empty `settings` file means "all defaults".
2. **`/config/settings`.** UTF-8 lines `NAME = VALUE`; `#` starts a comment line; blank lines are kept. `VALUE` is
   one word of ADR-0150's quoting (`'...'` for spaces) with no expansion. Names are `GROUP.NAME` in lower case with
   `-`. A name appears at most once.
3. **Declared settings.** A component declares each setting in its description: `setting NAME TYPE DEFAULT TEXT`,
   with types `yes-no`, `number MIN MAX`, `text`, `path`, `choice A|B|C`. The first ones:

   | Name | Type | Default | Meaning |
   |---|---|---|---|
   | `shell.prompt` | text | `'{cwd}> '` | the prompt; `{cwd}` is the current directory with `~` for home |
   | `shell.home` | path | `/home` | where an interactive shell starts, and what `~` and `cd` alone mean (ADR-0150) |
   | `shell.path` | text | `'/zoraya/bin /bin'` | where commands are looked for, in order |
   | `shell.history` | yes-no | `yes` | keep history in `/config/history` |
   | `shell.history-size` | number 0 100000 | `1000` | lines kept |
   | `shell.suggestions` | yes-no | `yes` | suggestions from history (ADR-0154) |
   | `shell.colour` | yes-no | `yes` | any colour from the shell |
   | `console.columns` | number 20 500 | `80` | width of the serial console |

4. **`settings`** (a builtin, since the shell holds `/config`, ADR-0155):
   - `settings` lists the settings that differ from their defaults;
   - `settings --all` lists every declared setting with its value;
   - `settings NAME` shows its value, default, type and description;
   - `settings NAME VALUE` checks and writes it, keeping the file's other lines and comments, atomically (a new file
     published over the old, ADR-0010's unnamed create and publish);
   - `settings --reset NAME` removes the line;
   - `settings --check` reads the file and reports every problem without changing anything.
   A change takes effect in the shell at once; other components read the file when they start.
5. **Hand edits.** When a component reads the file, a line it cannot parse, an unknown name of its group or a bad
   value is reported once as `NAME: /config/settings line N: PROBLEM` (`zosh:`, `init:`, ADR-0155 rule 5) and the
   default is used; nothing stops.
6. **The start file.** `/config/start.zosh` is a `zosh` script run by an interactive shell before its first prompt,
   for functions and variables; a failure in it is reported with its line and the prompt still comes (ADR-0151's stop
   rule ends the script, not the shell). Scripts and `zosh -c` do not run it.
7. **No environment.** Settings are not environment variables, and no program reads `/config` unless given it
   (ADR-0153). A program that needs a value gets it as an option or an exported variable from the line.

**Examples.**

```
~> settings
~> settings shell.prompt
shell.prompt = '{cwd}> '   (default)
  text: the prompt; {cwd} is the current directory with ~ for home
~> settings shell.prompt 'zoraya {cwd}$ '
zoraya ~$ settings
shell.prompt = 'zoraya {cwd}$ '
zoraya ~$ settings shell.colour maybe
settings: shell.colour: maybe is not yes or no
zoraya ~$ settings shell.colur no
settings: shell.colur: no such setting
  hint: did you mean shell.colour?
zoraya ~$ settings --reset shell.prompt
~> cat /config/start.zosh
# Functions for every session.
fn build {
    +w . make $args
}
```

### Effect of the vision

For target 2 a sandbox's configuration is whatever `/config` its launcher hands it, so an agent's session cannot read
or change the person's; the settings file is plain text an auditor reads. Target 3: GUI settings join as declared
`gui.*` names. Networking adds `net.*` names, nothing else.

**Tensions with accepted ADRs.** None. ADR-0104 item 12's `/etc/zoraya/session` is the POSIX session's own file and
stays.

## Consequences
- **Positive.** One file, one command, typed and checked, with defaults out of the file; no identity needed.
- **Negative.** A setting must be declared to exist; other components see a change only when they restart.
- **Follow-up.** The settings parser and writer in a `no_std` crate; `settings`; the declarations in `zosh`'s and
  `init`'s descriptions.

## Invariants
- **INV-0157-1.** `settings NAME VALUE` followed by reading the file gives back exactly the value, and every other
  line of the file, comments included, is unchanged. *Check:* a host property test of the writer over random files
  and settings.
- **INV-0157-2.** `settings` refuses an undeclared name or a value outside its type and leaves the file unchanged.
  *Check:* host tests over every type with boundary values.
- **INV-0157-3.** A file with any content never stops a component: every bad line is reported once and the defaults
  apply. *Check:* a deterministic random test of the reader over arbitrary bytes.

## Open questions
- Whether `/config/history` belongs with configuration or in a separate state directory.
- Whether settings changed by hand should reach a running component (a reload message), or only at start.

- *Note, 2026-11-03 (TASK-18 Part 2, step 6):* item 4's atomic write (a new file published over the old) needs
  ADR-0010's unnamed create and publish in the file client `zosh` uses, which has neither yet: `settings` writes
  the whole file in place, so a crash during the write can leave it cut short, and the reader then reports the
  damaged line and applies the default (INV-0157-3). To do with the client's next change.

## Revisit triggers
- A setting that needs a structure (a list of records): a typed list syntax, not a second file format.
- Several people sharing one machine: per-session `/config` directories become the launcher's business, as above.

## Architect's decision

Accepted as recommended, 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
