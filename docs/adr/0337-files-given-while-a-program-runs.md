# ADR-0337: Files a program needs while it runs: the refusal is shown at once, and the person gives more on a grant line that only a reserved key opens

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0052, ADR-0096, ADR-0107, ADR-0150, ADR-0153, ADR-0154, ADR-0158, ADR-0160, ADR-0167, ADR-0172, ADR-0224, ADR-0331, ADR-0332
- **Evidence:** CP-01, CP-02, CP-05, CP-24, DM-009, DM-014, DM-039, DM-071, DM-072, GR-008, alternatives-postmortem.md#genode, alternatives-postmortem.md#plan-9--inferno, desktop/survey.md#spoofing-and-the-trusted-path, shell/survey.md#capability-shells-and-grant-syntax, stage-10/survey.md#pattern-a-grants-fixed-at-start, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks, stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy, stage-10/survey.md#pattern-d-the-session-holds-the-grant, stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees, stage-10/survey.md#what-a-posix-program-is-given-today, stage-10/survey.md#write-by-rename-and-derived-files

## Context

The line `vi ~/notes.txt` grants `vi` that file (ADR-0153). Inside `vi` the person types `:e ~/other.txt`: a file the
shell never saw. `less` has `:e`; a compiler follows an `#include`; git rewrites a tree. ADR-0153 answers for
programs that end: the call fails, the runtime reports the path, and after the job the shell prints the `+r` line
that would allow it (rule 11). For a program a person lives in for an hour that answer comes an hour late, and what
the program shows meanwhile is wrong: `ENOENT` makes `vi` open an empty buffer named `other.txt`, as if the file did
not exist. ADR-0153 left open "whether the shell should offer a grant after a refused command (Deno's prompt), or
only the hint; a prompt trains people to say yes". This ADR decides what happens while the program runs, and what
the person sees.

**What is known.**

- Capability systems that shipped fix the grant at the start and fail the later request with a plain error: WASI
  preopens, CloudABI, Plash's shell, SHILL, Genode's routes, Fuchsia's namespaces. Under each, `:e other` fails and
  nothing says why (`stage-10/survey.md#pattern-a-grants-fixed-at-start`,
  `stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees`). TASK-28 expected a text-mode powerbox in Plash;
  its pages show only a graphical one.
- The one shipped text-mode request is Deno's prompt. It was spoofed twice: a program erased and redrew the prompt
  with escape sequences, and later the prompt showed one path and granted another. A terminal's input queue was the
  attack path of a sandbox escape (`TIOCSTI`) (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
- What a trusted request needs, by the sources that have one: a surface the program cannot draw on, input the
  program cannot produce, and a decision made from the very bytes shown; Windows reserves a key sequence and a
  separate desktop for it (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`,
  `desktop/survey.md#spoofing-and-the-trusted-path`).
- A helper that opens files for a confined program serves only the names fixed at the start (Casper's
  `fileargs`); OpenBSD does not confine the files of `vi`, `mg` or `less` at all, and its `got` gives the editor
  everything (`stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy`).
- When the terminal or the session holds the grant, one consent covers everything run there and the prompt names
  the terminal, not the program (macOS) (`stage-10/survey.md#pattern-d-the-session-holds-the-grant`).
- An editor that saves through a sibling file needs the directory; no surveyed system grants a file with its
  derived names (`stage-10/survey.md#write-by-rename-and-derived-files`). ADR-0335 chooses programs that save in
  place and derive nothing.
- Zoraya has the pieces: literal grants in one syntax (ADR-0153, ADR-0224), rooms that hold them for a project
  (ADR-0172), a report channel for refused paths, a shell that sits on every byte of a terminal (ADR-0331), a
  compositor that keeps some chords from every client (INV-0160-3), and a graphical chooser for native
  applications (ADR-0167), which POSIX programs do not have (`stage-10/survey.md#what-a-posix-program-is-given-today`).

**What is assumed.** That a person will press a chord and type or accept a short grant when they meant the file,
and will not when they did not. No number for prompt fatigue was found in a primary source.

## Decision drivers

1. **The line still says what a program gets**: least authority by default, visible, in one syntax (CP-01, CP-05,
   DM-009, ADR-0153 driver 1).
2. **The truth, at once.** A refusal must not look like a missing file for an hour
   (`stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees`).
3. **The person designates; the program does not ask.** A request the program words and a person approves is the
   prompt ADR-0153 distrusted, and DM-014's lesson is that heavy or nagging policy is turned off
   (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
4. **Nothing a program writes or sends can make, word or confirm a grant** (DM-071, DM-072, INV-0153-3).
5. **Rights only narrow**: nobody gives more than they hold (CP-02).
6. **Unmodified programs** (GR-008: capabilities hurt exactly where programs open paths late).
7. **The same on the serial console and in the window**, with the window allowed to show it better (CP-24).

## Considered options

### Option A: The program may open what the person may (Unix)

- **Description:** An interactive program gets the shell's whole table, as a Unix editor gets its user's files.
- **Evidence for:** `:e` works; nothing to learn. It is what OpenBSD does for `vi`, `mg` and `less`, after trying
  hard to confine everything else (`stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy`).
- **Evidence against:** Ambient authority (DM-009, CP-01), and path races with it (DM-039). "Interactive" is not a
  property the shell can check, so every program would claim it. Target 2 is precluded.
- **Cost:** *(estimate)* None now.
- **Precedent:** Unix; OpenBSD's pledge-only editors.

### Option B: Only what was fixed at the start, with the hint afterwards (today)

- **Description:** Directories are granted on the line (`+w ~/proj vi notes.txt`) or held by the room. Anything
  else fails; the shell prints ADR-0153's hint when the program ends.
- **Evidence for:** Nothing new to build or to attack; it is what every shipped capability system does
  (`stage-10/survey.md#pattern-a-grants-fixed-at-start`). The person learns to grant the project.
- **Evidence against:** Driver 2 fails for long-running programs, and the person must leave the editor, lose its
  state and start again to add one file. That pressure produces `+w ~` at the start, which is ADR-0153's own revisit
  trigger.
- **Cost:** *(estimate)* None.
- **Precedent:** WASI, CloudABI, Plash's shell, SHILL, Genode, Fuchsia.

### Option C: The system asks on the program's request (Deno's prompt; the Mac's consent alert)

- **Description:** When a program opens a path outside its table, the call waits and the terminal shows "`vi`
  wants to read `~/other.txt`. Allow?"; the answer decides the call.
- **Evidence for:** Driver 6 with no retry: `:e` simply works after "yes". Deno ships it; macOS ships it per
  protected folder (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
- **Evidence against:** Driver 3: the program chooses the moment and the path, and the person only confirms. Both
  of Deno's advisories are this prompt's. A build that probes a hundred missing paths raises a hundred questions,
  or needs a "yes to all", which is Option A by one key. A call that waits for a person changes every program's
  timing.
- **Cost:** *(estimate)* A blocking path in the runtime, a modal prompt, rules for scripts and for programs in the
  background.
- **Precedent:** Deno (two CVEs); macOS TCC (the grant lands on the terminal and all it runs).

### Option D: The person opens a grant line with a reserved key and types the grant

- **Description:** The refusal is shown at once on a line of the terminal that no program can draw on. A chord that
  no program receives opens a line there on which the person writes `+r PATH` or `+w PATH`, in the shell's syntax;
  the shell gives that to the running job. The program is not asked and does not wait; the person repeats the
  command inside it.
- **Evidence for:** Drivers 1 to 7. It is the powerbox's principle, the person's own act of naming is the grant,
  carried to text (`shell/survey.md#capability-shells-and-grant-syntax`, ADR-0167), with the three properties the
  sources ask of a trusted request (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
- **Evidence against:** Nothing like it has shipped: the survey found no text-mode precedent. The person acts twice
  (grant, then repeat the command). A program that treated the first refusal as "new file" needs to be told to read
  again. One more key and one more line to learn.
- **Cost:** *(estimate)* 500 lines in `zoraya-shell` and `zosh`, 200 in `term`, a slot and 150 lines in each
  runtime, a reserved chord in the compositor.
- **Precedent:** In graphics, CapDesk, the Mac's open panel, the portal (ADR-0167). In text, none.

### Option E: A room for the session, and nothing else (Genode's deployment, the Mac's terminal consent)

- **Description:** Work that needs many files is done in a room: `room new work +w ~/proj`, `room work`. A program
  outside a room gets its line; there is no way to add to a running program.
- **Evidence for:** It exists (ADR-0172), is shown by the compositor, and matches how people think of a project.
  Genode decides a component's file system at deployment, by the person, outside the component
  (`alternatives-postmortem.md#genode`, `stage-10/survey.md#pattern-d-the-session-holds-the-grant`).
- **Evidence against:** The file a person wants next is by definition the one the room does not hold; widening the
  room for it widens every later program in the room. As the only answer it is Option B with a longer memory.
- **Cost:** *(estimate)* None.
- **Precedent:** Genode's Sculpt; Qubes; Plan 9's namespaces, assembled before a program starts
  (`alternatives-postmortem.md#plan-9--inferno`).

### Option F: A helper inside each program that opens files for it (Casper; Game of Trees)

- **Description:** Each interactive port is split: a small trusted part holds the grants and opens files on behalf
  of the confined rest.
- **Evidence for:** The program's own structure carries the policy; `got` and FreeBSD's filters are built so
  (`stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy`).
- **Evidence against:** Driver 6: every port is rewritten, and the helper still needs a rule for names the person
  types later, which is this ADR's question again. Casper's set is closed at the start.
- **Cost:** *(estimate)* A redesign per program.
- **Precedent:** FreeBSD's `head`, `wc`, `tail`, `cat`; `got`.

## Recommendation

**Recommendation (not a decision): Option D, on top of Options B and E as they stand.** Before a program starts,
the line and the room decide, as today. While it runs, the person can give more; the program can only be refused.

1. **A refusal stays a refusal.** A path no entry covers fails at once with `ENOENT` (ADR-0107 item 6), a write
   inside a read-only entry with `EACCES`; no call ever waits for a person. Both are reported on the `REPORT`
   channel (ADR-0153 rule 11, extended to the second case, with `access: write`).
2. **It is shown at once.** The shell reads the reports while the job runs. For the job in front it shows the
   latest one on the terminal's **status line**: one row below the grid that belongs to the window and that no
   output sequence can reach (`Status { text }`, a new ordinal of `zoraya-proto-term`; `term` draws it outside the
   grid). The words are the hint's:

   ```
   vi was not given ~/other.txt (to read)                         System+G gives it
   ```

   The shell, which may hold more than the program, checks the path first and says `which does not exist` when
   that is the truth, so a typing mistake is told apart from a missing grant. The status line is empty when
   nothing is pending. After the job ends the hint of rule 11 is printed as today.
3. **The attention key.** On the desktop the chord System+G is the compositor's: it reaches no client as a key
   (INV-0160-3) and makes the compositor send the focused terminal `Attention`, as System+C sends `Copy`
   (ADR-0170). `term` passes it to the shell in a message of its own, never as input bytes. On the serial console
   the shell takes one control character before the discipline sees it, `shell.attention`, default Ctrl-]
   `[assumption]`. Nothing a program writes, and nothing typed to a program, produces either.
4. **The grant line.** With a job in front, the attention key makes the status line an edit field:

   ```
   give vi:  +r ~/other.txt
   ```

   While it is open, keys go to the shell's own editor and not to the program. The line takes one or more grants
   in ADR-0153's and ADR-0224's words, `+r PATH`, `+w PATH`, `+net HOST[:PORT]`, with the shell's completion of
   paths. If a refusal is pending, the grant that would cover it is offered as the faint suggestion ADR-0154's
   editor already draws for history; Right or Tab takes it, typing replaces it. For a path that exists it is `+r`
   or `+w` on that path; for a name to be created, `+w` on its directory, as rule 11 offers. A reported path that
   cannot be written as one printable word of at most 240 bytes is never suggested. Enter gives; Escape or the key
   again closes the line. With no job in front the key does nothing: the prompt is already where grants are typed.
5. **What is given.** The shell parses the line's text with its own parser and resolves each word against its own
   table and network scope, exactly as for a grant before a command (ADR-0153 rule 3, ADR-0224 item 1): it can give
   only what it holds, so in a room no more than the room. The grant goes to the job in front and lasts until that
   job ends. It is written nowhere. `jobs` shows it with the job's other grants, marked `(given while running)`.
   To keep a grant, the person adds it to the room (`room add`, ADR-0172).
6. **How it arrives.** The start-up record and the bootstrap record gain a fixed slot, `GRANTS`: the read end of
   a channel whose write end the shell keeps for each program it starts. A message carries entries in ADR-0107's
   encoding with their connections, or a network connection whose scope is the old one's and the new rule's union.
   The runtime (`libposix`, `zoraya-rt`) reads the channel only when a path resolves to no entry or to an entry
   without the right, or `netd` answers `NotGranted`; it then resolves once more before it fails and reports. A
   path that resolves costs nothing new. Children started afterwards inherit the entries as they inherit the table;
   a child already running does not receive them.
7. **The program is not told.** The person repeats what they asked: in `vi`, `:e ~/other.txt` again, or `:e!` to
   read the file into the buffer the refusal left empty; in `less`, `:e` again; a build is run again. `help` says
   so in one sentence per program.
8. **Untrusted rooms.** No suggestion is ever offered there, and Enter does not give: it asks the compositor for
   the confirmation card ADR-0172 point 6 already requires for a grant to an untrusted room, which shows the
   grant's text; only its confirmation gives. On the serial console an untrusted room's grant line is refused with
   `not from this terminal` (no card can be shown there).
9. **Not built here.** The graphical chooser for POSIX programs (ADR-0167's deferred item) and a request a program
   makes on purpose (a native "ask for a file" call) are separate decisions; the `GRANTS` slot is what either
   would deliver through.

**What the person sees.** In a terminal window:

```
~/proj> vi notes.txt
```

`vi` fills the grid. The person types `:e ~/other.txt`. `vi` shows an empty buffer and says, in its own words on its
own last line, that the file is new. Below the grid the window's status line says:

```
vi was not given ~/other.txt (to read)                         System+G gives it
```

The person presses System+G. The status line becomes `give vi:` with `+r ~/other.txt` in faint text; Right takes
it, Enter gives it; the line says `given until vi ends: +r ~/other.txt`. The person types `:e!` and the file is
there. `:w` on it is refused, the status line says `(to change)` and offers `+w ~/other.txt`. If they had typed
`:e ~/othr.txt`, the line would have said `vi asked for ~/othr.txt, which does not exist`, and nothing would be
offered.

On the serial console there is no status line. The same refusal is printed when `vi` ends, as today. During the
run, Ctrl-] makes the shell hold the program's output and write `give vi:` on the bottom row; after Enter or Escape
the shell sends the program `SIGWINCH` so that it repaints.

To avoid all of it, the person says so before starting: `+w ~/proj vi notes.txt`, or works in a room:

```
~> room new work +w ~/proj
~> room work
work ~/proj> vi notes.txt
```

where `:e` and `:w` reach everything in `~/proj` and nothing else. In an untrusted room the status line shows the
refusal, the grant line opens empty, and a grant typed there brings up the compositor's card with the
words `+r ~/sandbox/agent-1/notes.txt for vi in agent-1`.

**Tensions with accepted ADRs.**

- *ADR-0153 rule 3* ("a grant is recognised only as a literal `+r` or `+w` token before the command name, written
  in the source ... a grant applies to that one command") and *INV-0153-3*. The grant line is a third place where
  literal grants are written, after the line and `/config/rooms` (ADR-0172 point 3). The token is still typed or
  accepted by the person in the shell's own editor and parsed by the shell; no value, file or program output makes
  one. What is new, and must be accepted knowingly: the *suggestion* is derived from a path the program asked for.
  It is offered as history suggestions are, becomes a grant only by the person's keys, is never offered in an
  untrusted room, and can be removed altogether (open question) at the cost of typing the path twice.
- *ADR-0153 rule 11*: reports are read during the run as well as after it, and a write refused inside a read-only
  entry is reported. INV-0153-7 gains that case.
- *ADR-0153's open question* on a prompt is answered: no prompt; a line the person opens.
- *ADR-0107 item 5*: one more fixed slot, `GRANTS`, in the list INV-0002-5's check enumerates, and in
  `zoraya_abi::native::Startup` (ADR-0153 rule 8).
- *ADR-0096*: `Status` and `Attention` join `zoraya-proto-term`; the status row is outside the grid, so
  INV-0096-1 to INV-0096-3 are untouched.
- *ADR-0160, INV-0160-3*: System+G joins the chords that reach no client; like System+C it produces a message to
  the focused window.
- *ADR-0172*: a job's run-time grants are not pills on the frame (the pills are the room's, INV-0172-1); they are
  on the status line and in `jobs`. Point 6's card is used as written.
- *ADR-0224's revisit trigger* ("a program that follows redirects across hosts ... the question becomes a prompt or
  a proxy policy") is met by `+net` on the grant line, the person's act, not a prompt.
- *ADR-0167*: unchanged; the chooser stays the desktop's way for native applications.

### Effect of the vision

This is target 2's missing half. An agent in a room can be given one more file by its supervisor without ending
it, the agent cannot ask in any way that reaches a person as a question, and what it tried is on the status line
and in the hint: an audit trail the agent cannot write. The grant line is bounded by the room's shell, so a person
at an untrusted room's terminal cannot be talked into giving what the room does not hold. A supervising program
would later hold the write end of `GRANTS` instead of a person (CP-24's shape). Target 3: a person works in `vi`
without planning their grants. Networking: the same line gives a host. Threads: the runtime's re-read is under the
library's lock like any table change.

## Consequences

- **Positive.** A refusal is visible when it happens and tells a missing file from a missing grant. A person adds
  one file to a running program in three keys, in the syntax they already use, without a prompt existing for a
  program to trigger, time or imitate. Nothing is stored; rooms stay the way to keep.
- **Negative.** No shipped precedent in text. The program is not told, so the person repeats a command, and `vi`'s
  "new file" on a refused path is still misleading inside `vi`. The suggestion comes from the program's request. A
  running child does not receive a later grant. The serial console has the line but not the protected row.
- **Follow-up.** `GRANTS` in both records and runtimes; reports read live; `Status` and `Attention`; the status
  row in `term`; the chord in the compositor; the grant line in the shell's editor; `jobs`; the hint texts and
  `help grants`; a step in `docs/tour.md`.

## Invariants

- **INV-0337-1.** A running program's table or network scope grows only by a grant line that the attention key of
  its own terminal opened and the person ended with Enter. No program output, no `REPORT`, no byte typed to a
  program and no message a client can send opens the line or gives anything. *Check:* a host test of the shell's
  input path feeding program output, reports and keys of every kind without the attention message and asserting
  no grant; the compositor's host test of INV-0160-3 extended with System+G; a fuzz test of `term`'s input.
- **INV-0337-2.** While the grant line is open, no key reaches a program, and the attention key never reaches one
  as bytes. *Check:* a host test of the shell with a job in front in raw mode, keys before, during and after the
  line.
- **INV-0337-3.** What is given is exactly what the line's text parses to, each word resolved against the shell's
  own table and scope and refused if it reaches outside them. *Check:* the host tests of INV-0153-2, INV-0153-3,
  INV-0172-4 and INV-0224-1, run through the grant line's entry point.
- **INV-0337-4.** A grant made on the grant line ends with its job and is stored nowhere. *Check:* a host test
  that the shell closes the write end and drops the record at the job's end; an in-QEMU script that gives a file to
  one `vi`, ends it, starts another and sees the refusal again, with `/config/rooms` unchanged.
- **INV-0337-5.** The status line's cells change only by the shell's `Status` message. *Check:* the fuzz test of
  INV-0096-1 with the status row's contents compared before and after; the GUI script reads the row after a
  program prints text shaped like it.
- **INV-0337-6.** A refused path of the job in front appears on the status line while the job runs, in the hint's
  words; a missing path inside an entry never does. *Check:* INV-0153-7's host test of the reporter extended to
  live reads; a GUI script with `vi` opening one path outside its entries and one missing inside.
- **INV-0337-7.** In an untrusted room the grant line offers no suggestion and gives nothing without the
  compositor's confirmation. *Check:* a host test of the shell in a room marked untrusted, with a fake compositor
  that never confirms and one that does.

## Open questions

- Whether the suggestion should exist at all, or the person should always type the path; a small study of how
  often it is taken without reading would inform it. Removing it changes one item and no mechanism.
- A word that gives one name to create (`+w` on a file that does not exist yet gives its whole directory today,
  as ADR-0153's rule 11 offers); the chooser's `Save` already gives one name.
- Whether the key should instead open the graphical chooser as a sheet on the terminal window when there is one,
  and type the chosen path's grant into the line.
- Whether a running child should receive later grants through its parent's runtime.
- Whether the status line needs the compositor's trusted material in a trusted room, or `term`'s own row is enough:
  a program can imitate its look inside the grid, and gains nothing by it, since the key and the line are not its.

## Revisit triggers

- People grant `+w ~` at the start to avoid the grant line: the line is too slow, or the defaults too narrow.
- A way is found for output or input to open or fill the grant line.
- A ported program in scope cannot recover from a first refusal without being restarted.
- The agent-isolation direction is chosen: a supervisor's programmatic holder of `GRANTS`.

## Architect's decision
