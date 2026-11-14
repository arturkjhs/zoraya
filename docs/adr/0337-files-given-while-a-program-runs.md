# ADR-0337: Files a program needs while it runs: the refusal is shown at once, and the person gives more on a grant line that only a reserved key opens

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0052, ADR-0055, ADR-0096, ADR-0107, ADR-0150, ADR-0153, ADR-0154, ADR-0158, ADR-0160, ADR-0167, ADR-0172, ADR-0224, ADR-0245, ADR-0248, ADR-0268, ADR-0331, ADR-0332, ADR-0335, ADR-0341
- **Evidence:** CP-01, CP-02, CP-05, CP-24, DM-009, DM-014, DM-039, DM-071, DM-072, GR-008, alternatives-postmortem.md#genode, alternatives-postmortem.md#plan-9--inferno, desktop/survey.md#spoofing-and-the-trusted-path, shell/survey.md#capability-shells-and-grant-syntax, stage-10/survey.md#pattern-a-grants-fixed-at-start, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks, stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy, stage-10/survey.md#pattern-d-the-session-holds-the-grant, stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees, stage-10/survey.md#what-a-posix-program-is-given-today, stage-10/survey.md#write-by-rename-and-derived-files

## Context

The line `vi ~/notes.txt` grants `vi` that file (ADR-0153, ADR-0335). Inside `vi` the person types
`:e ~/other.txt`: a file the shell never saw. `less` has `:e`; a compiler follows an `#include`; git rewrites a
tree. ADR-0153 answers for programs that end: the call fails, the runtime reports the path, and after the job the
shell prints the `+r` line that would allow it (rule 11). For a program a person lives in for an hour that answer
comes an hour late, and what the program shows meanwhile is wrong: `ENOENT` makes `vi` open an empty buffer named
`other.txt`, as if the file did not exist. ADR-0153 left open "whether the shell should offer a grant after a
refused command (Deno's prompt), or only the hint; a prompt trains people to say yes". This ADR decides what happens
while the program runs, and what the person sees.

**What is known.**

- Capability systems, shipped and research, fix the grant at the start and fail the later request with a plain
  error: WASI preopens, CloudABI, Plash's shell, SHILL, Genode's routes, Fuchsia's namespaces. Under each,
  `:e other` fails and nothing says why (`stage-10/survey.md#pattern-a-grants-fixed-at-start`,
  `stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees`). TASK-28 expected a text-mode powerbox in Plash;
  its pages show only a graphical one.
- The one shipped text-mode request is Deno's prompt. It was spoofed twice: a program erased and redrew the prompt
  with escape sequences, and later the prompt showed one path and granted another. Both are flaws of a drawing
  surface shared with the program and of a decision made from other bytes than those shown. A terminal's input
  queue was the attack path of a sandbox escape (`TIOCSTI`)
  (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
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
- In Zoraya (`stage-10/survey.md#what-a-posix-program-is-given-today`, and read in the code by the review):
  - A refused path is reported by the program's own runtime on a channel whose write end is in the program's
    table. The report is whatever the program writes: any text without NUL up to 4096 bytes. It is sent once per
    path and at most 16 times per process, and the hint the shell prints from it does not quote the path.
  - A POSIX program's table holds directories only; a file reaches it through a directory entry narrowed to a
    name (ADR-0153 rule 10's note, ADR-0311 item 5).
  - A line in a room already receives everything the room holds (ADR-0172 point 3), and a room's own terminal
    holds exactly the room's grants (ADR-0172's decision of 2026-11-09). Untrusted rooms are refused until
    ADR-0268's gate.
  - The compositor keeps some chords from every client (INV-0160-3), and guards a sheet against a key already on
    its way (ADR-0245). A graphical chooser exists for native applications (ADR-0167); POSIX programs have none.

**What is assumed.** That a person will press a chord and type a short grant when they meant the file, and will
not when they did not. No number for prompt fatigue was found in a primary source.

## Decision drivers

1. **The line still says what a program gets**: least authority by default, visible, in one syntax (CP-01, CP-05,
   DM-009, ADR-0153 driver 1).
2. **The truth, at once.** A refusal must not look like a missing file for an hour
   (`stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees`).
3. **The person designates; the program does not ask.** A request the program words and a person approves is the
   prompt ADR-0153 distrusted, and DM-014's lesson is that heavy or nagging policy is turned off
   (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
4. **Nothing a program writes or sends can make, word, suggest or confirm a grant** (DM-071, DM-072,
   INV-0153-3). What a program reports is data, and is treated as hostile.
5. **Rights only narrow**: nobody gives more than they hold (CP-02).
6. **Unmodified programs** (GR-008: capabilities hurt exactly where programs open paths late).
7. **Nothing weaker than what a line at the prompt already allows**, and nothing stronger: the grant is the same
   act, moved in time (CP-24: privileged powers explicit).

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
- **Evidence for:** Nothing new to build or to attack; it is what every surveyed capability system does
  (`stage-10/survey.md#pattern-a-grants-fixed-at-start`). The person learns to grant the project, and the hint
  already names the directory to give.
- **Evidence against:** Driver 2 fails for long-running programs, and the person must leave the editor, lose its
  state and start again to add one file. That pressure produces `+w ~` at the start, which is ADR-0153's own revisit
  trigger.
- **Cost:** *(estimate)* None.
- **Precedent:** WASI, CloudABI, Plash's shell, SHILL, Genode, Fuchsia.

### Option C: Option B, with the refusal shown at once

- **Description:** As today, plus one thing: the shell reads the reports while the job runs and shows the latest
  on a status line of the terminal. To act on it the person still ends the program and starts it with a grant.
- **Evidence for:** Drivers 1, 2 and 4 with no new way to grant anything: the smallest change that stops a
  refusal from looking like a missing file. It needs none of the reserved key, the grant line or the new slot.
- **Evidence against:** The person still loses the editor's state to add one file (driver 6's spirit), so the
  pressure toward a wide grant at the start remains. The status line is a new place where program-reported text
  is shown.
- **Cost:** *(estimate)* 150 lines in the shell, 100 in `term`, one message.
- **Precedent:** None surveyed shows a refusal to the person as it happens; Node's audit channel reports it to
  the developer (`stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees`).

### Option D: The system asks on the program's request (Deno's prompt; the Mac's consent alert)

- **Description:** When a program opens a path outside its table, the call waits and the terminal asks "`vi`
  wants to read `~/other.txt`. Allow?"; the answer decides the call. In its best form the question is drawn on a
  surface the program cannot reach and is answered only after a reserved key, which removes both of Deno's flaws.
- **Evidence for:** Driver 6 with no retry: `:e` simply works after "yes", and `vi` never shows a wrong empty
  buffer. Deno ships it; macOS ships it per protected folder
  (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
- **Evidence against:** Driver 3: the program chooses the moment and the path, and the person only confirms; a
  hostile program asks for the key file just after the person asked for something else. A build that probes a
  hundred missing paths raises a hundred questions, or needs a "yes to all", which is Option A by one key. A call
  that waits for a person changes every program's timing, and a program in the background or in a script must be
  given a rule of its own.
- **Cost:** *(estimate)* A blocking path in the runtime, a modal prompt, rules for scripts and for programs in the
  background.
- **Precedent:** Deno (two CVEs, both fixable on a separate surface); macOS TCC (the grant lands on the terminal
  and all it runs).

### Option E: The person opens a grant line with a reserved key and types the grant

- **Description:** Option C's status line, and a chord that no program receives, which opens a line there on which
  the person writes `+r PATH` or `+w PATH`, in the shell's syntax; the shell gives that to the running program. The
  program is not asked and does not wait; the person repeats the command inside it.
- **Evidence for:** Drivers 1 to 7. It is the powerbox's principle, the person's own act of naming is the grant,
  carried to text (`shell/survey.md#capability-shells-and-grant-syntax`, ADR-0167), with the three properties the
  sources ask of a trusted request (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`). It is the
  grant a person may already type before a command, typed later.
- **Evidence against:** Nothing like it has shipped: the survey found no text-mode precedent. The person names the
  file twice (to the program, then to the shell) and repeats the command. A program that treated the first
  refusal as "new file" needs to be told to read again. One more key and one more line to learn. In a room's own
  terminal it can give nothing (item 6).
- **Cost:** *(estimate)* 500 lines in `zoraya-shell` and `zosh`, 200 in `term`, a slot and 150 lines in each
  runtime, a reserved chord in the compositor, a narrowing to several names in the file service.
- **Precedent:** In graphics, CapDesk, the Mac's open panel, the portal (ADR-0167). In text, none.

### Option F: A room for the session, and nothing else (Genode's deployment, the Mac's terminal consent)

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

### Option G: A helper inside each program that opens files for it (Casper; Game of Trees)

- **Description:** Each interactive port is split: a small trusted part holds the grants and opens files on behalf
  of the confined rest.
- **Evidence for:** The program's own structure carries the policy; `got` and FreeBSD's filters are built so
  (`stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy`).
- **Evidence against:** Driver 6: every port is rewritten, and the helper still needs a rule for names the person
  types later, which is this ADR's question again. Casper's set is closed at the start.
- **Cost:** *(estimate)* A redesign per program.
- **Precedent:** FreeBSD's `head`, `wc`, `tail`, `cat`; `got`.

## Recommendation

**Recommendation (not a decision): Option E, on top of Options B and F as they stand.** Before a program starts,
the line and the room decide, as today. While it runs, the person can give more; the program can only be refused.
Items 1 to 3 are Option C and stand alone: if the architect declines the grant line, they are the design.

1. **A refusal stays a refusal.** A path no entry covers fails at once with `ENOENT` (ADR-0107 item 6), a write
   inside a read-only entry with `EACCES`; no call ever waits for a person. Both are reported on the `REPORT`
   channel (ADR-0153 rule 11), now once per path *and* access, and the limit of 16 counts the reports since the
   program last received a grant, so a long session does not fall silent.
2. **A report is hostile data.** The shell shows a reported path only if, after it normalises the path itself,
   the whole of it is made of the characters ADR-0311 allows in a name (ASCII letters and digits and
   `. - _ + ~ @ , =`) and `/`, and it fits the row whole. No other script is shown: letters that look like
   others, or that are drawn right to left, could make a shown path read as a different one. Anything else is shown as
   `a path that cannot be shown`, which also happens to honest names in other scripts
   (open question). The same rule and
   ADR-0150's quoting apply to the hint ADR-0153 rule 11 prints after a job, which today prints the path as it
   came. What is shown is what the program *reported*; a program can report a path it never opened, and cannot
   make the line say anything but a path.
3. **It is shown at once.** For the programs of the line in front, the shell shows the latest report on the
   terminal's **status line**: one row below the grid that belongs to the window and that no output sequence can
   reach (`Status { text }`, a new ordinal of `zoraya-proto-term`; `term` draws it outside the grid). The words
   are the hint's, with the number of other refusals waiting:

   ```
   vi was not given ~/other.txt (to read)  [2 more]                System+G: give
   ```

   The shell looks the path up in its own table, without opening it and without following a link object, and
   says which of three things is true: `was not given` (the shell holds it and the program does not), `asked for
   ~/othr.txt, which does not exist` (so a typing mistake is told apart from a missing grant), or `asked for
   /x, which this shell does not hold` (nothing can be given from here). The status line is empty when nothing is
   pending. After the job ends the hint of rule 11 is printed as today.
4. **The attention key.** The chord System+G is the compositor's: it reaches no client as a key (INV-0160-3) and
   makes the compositor send the focused terminal `Attention`, as System+C sends `Copy` (ADR-0170). `term` passes
   it to the shell in a message of its own, never as input bytes. Nothing a program writes, and nothing typed to
   a program, produces it. The shell ignores Enter for 500 ms after the line opens, the guard ADR-0245 put on
   sheets, so that a key already on its way cannot confirm anything.
5. **The grant line.** With a line in front, the attention key makes the status line an edit field:

   ```
   give vi:
   ```

   While it is open, keys and pastes go to the shell's own editor and not to the program; a pasted newline is
   text, not Enter. The person types one or more grants in ADR-0153's and ADR-0224's words, `+r PATH`,
   `+w PATH`, `+net HOST[:PORT]`, as literal words (no variable, no substitution), with the shell's completion of
   paths against its own table. **The line opens empty and nothing is suggested**: the status line above it
   shows what was refused, and the person types what they mean. Enter gives; Escape or the key again closes the
   line. When the line in front is a pipeline, the field names the element whose refusal was on the status line when
   the key was pressed, or the last element if there was none; it does not change while the line is open, except
   by the person's Tab on the empty field, which moves to the next element. The grant goes to that one element
   (ADR-0153 rule 3). With no line in front the key does nothing: the prompt is already where grants are
   typed.
6. **What is given, and from what.** The shell parses the line's text with its own parser and resolves each word
   against its own table and network scope, exactly as for a grant before a command (ADR-0153 rule 3, ADR-0224
   item 1). It can give only what it holds. In the first terminal that is the person's files. In a shell that
   entered a room with `room NAME`, it is what that shell holds, which may be more than the room: the same as
   typing `+r PATH` before a command in that room. **In a room's own terminal the line can give nothing new**,
   because every line there already has all the shell holds; the status line then says `which this shell does not
   hold`, and the person adds to the room from a `home` shell (`room add`, ADR-0172), which reaches programs
   started afterwards. The grant lasts until that program's line ends. It is written nowhere. `jobs` shows it with
   the line's other grants, marked `(given while running)`.
7. **How it arrives.** The bootstrap record gains a fixed slot and the native start-up record a header index,
   `GRANTS`: the read end of a channel whose write end the shell keeps for each program it starts. A message
   carries entries in ADR-0107's encoding with their connections, or a network connection whose scope is the old
   one's and the new rule's union. The runtime (`libposix`, `zoraya-rt`) reads the channel only when a path
   resolves to no entry or to an entry without the right, or `netd` answers `NotGranted`; it then resolves once
   more before it fails and reports. A path that resolves costs nothing new. An entry that arrives for a prefix
   the table already has is joined with it, the rights being the union (ADR-0153 rule 2); the prefix of a
   program's own directory is never touched (ADR-0341 item 3). The table's limit of 64 entries stands; at the limit
   the grant is refused
   on the status line. Children started afterwards inherit the entries as they inherit the table; a child already
   running does not receive them.
8. **A file, for a program whose table holds directories.** A file given to a POSIX program is its directory
   narrowed to that name (ADR-0311 item 5). A second file of the same directory, at start (`vi a.txt b.txt`) or
   later, needs one connection that admits both names: `narrow` in the file protocol takes a list of names, each
   with its rights, where ADR-0153 gave it one name. The shell keeps the list per directory and sends the
   replaced entry.
9. **The program is not told.** The person repeats what they asked: in `vi`, `:e ~/other.txt` again, or `:e!` to
   read the file into the buffer the refusal left empty; in `less`, `:e` again; a build is run again. `help` says
   so in one sentence per program.
10. **Untrusted rooms and the serial console: not in stage 10.** For a program of an untrusted room the grant
    line is refused (`not for an untrusted room`): ADR-0167 asks that a grant typed for such a room be confirmed in
    the compositor's own card, no path for that exists, and untrusted rooms themselves wait for ADR-0268's gate.
    On the serial console there is no status line and no reserved chord, and full-screen programs are not carried
    there (ADR-0331 item 13); the refusal is printed when the program ends, as today.
11. **Not built here.** The graphical chooser for POSIX programs (ADR-0167's deferred item) and a request a
    program makes on purpose (a native "ask for a file" call) are separate decisions; the `GRANTS` slot is what
    either would deliver through.

**What the person sees.** In a terminal window:

```
~/proj> vi notes.txt
```

`vi` fills the grid. The person types `:e ~/other.txt`. `vi` shows an empty buffer and says, in its own words on its
own last line, that the file is new. Below the grid the window's status line says:

```
vi was not given ~/other.txt (to read)                          System+G: give
```

The person presses System+G. The status line becomes `give vi:`; they type `+r ~/oth`, Tab completes the name,
Enter gives it; the line says `given until vi ends: +r ~/other.txt`. The person types `:e!` and the file is there.
`:w` on it is refused and the status line says `vi was not given ~/other.txt (to change)`. If they had typed
`:e ~/othr.txt`, the line would have said `vi asked for ~/othr.txt, which does not exist`.

If a program reports something that is not a plain path, the line says `vi was refused a path that cannot be
shown`, and there is nothing to copy from it.

To avoid all of it, the person says so before starting: `+w ~/proj vi notes.txt`, or works in a room:

```
~> room new work +w ~/proj
~> room work
work ~/proj> vi notes.txt
```

where `:e` and `:w` reach everything in `~/proj` and nothing else. In the room's own terminal window, a refusal for
a path outside `~/proj` reads `vi asked for ~/other.txt, which this shell does not hold`; the person switches to a
`home` terminal and types `room add work +r ~/other.txt`, and the next `vi` in the room has it.

**Tensions with accepted ADRs.**

- *ADR-0153 rule 3* ("a grant is recognised only as a literal `+r` or `+w` token before the command name, written
  in the source ... a grant applies to that one command") and *INV-0153-3*. The grant line is a third place where
  literal grants are written, after the line and `/config/rooms` (ADR-0172 point 3). The words are typed by the
  person in the shell's own editor and parsed by the shell; no value, file, report or program output makes or
  suggests one, so INV-0153-3 holds as worded. What changes is *when* a grant can be written.
- *ADR-0153 rule 11 and INV-0153-7*: reports are read during the run as well as after it; a write refused inside
  a read-only entry is reported; the report is once per path and access, not per path; the limit of 16 restarts
  after a grant; the path is filtered and quoted before it is shown (item 2), also in the existing hint.
- *ADR-0153's open question* on a prompt is answered: no prompt; a line the person opens.
- *ADR-0055 and ADR-0153 rule 2* (`narrow(name, rights, server_end)`: one component): item 8 needs a list of
  names. Two operands in one directory at start need it as well.
- *ADR-0107 item 5*: one more fixed slot, `GRANTS`, in the list INV-0002-5's check enumerates; in
  `zoraya_abi::native::Startup` (ADR-0153 rule 8) a header index, as `REPORT` is there.
- *ADR-0096*: `Status` and `Attention` join `zoraya-proto-term`; the status row is outside the grid, so
  INV-0096-1 to INV-0096-3 are untouched.
- *ADR-0160 (INV-0160-3) and ADR-0248 item 1*: System+G joins the chords that reach no client, so one chord
  fewer is passed on to the focused client; like System+C it produces a message to the focused window.
- *ADR-0172's concept rule* ("glow means trust": the trusted material is used for "the handing out of grants") and
  *INV-0172-1*: the grant line hands out a grant in the terminal's own text, not in the compositor's material. So
  does a `+r` typed at the prompt today, and `grants` and the hints state grants in the same text. The frame's
  pills stay the compositor's and show the room, not a job's run-time grants. This is a stated exception for the
  shell's text, and the open questions ask whether the line should be drawn by the compositor instead.
- *ADR-0167's Rooms bullet* (a `+r` line for an untrusted room is confirmed in the compositor's card): not
  carried out here; item 10 refuses the case.
- *ADR-0224's revisit trigger* ("a program that follows redirects across hosts ... the question becomes a prompt or
  a proxy policy") is met by `+net` on the grant line, the person's act, not a prompt. *INV-0224-1* holds: the
  word is literal and typed.
- *ADR-0245*'s guard for sheets is taken over for the grant line (item 4). *ADR-0154*: Tab completes; on the
  empty field of a pipeline's grant line it moves to the next element instead (item 5).
- *ADR-0332 item 8*: a paste while the grant line is open goes into the line, and its newlines do not confirm.

### Effect of the vision

For target 2 this ADR is mostly about what a hostile program can do to the person's view. It cannot open the line,
type into it or confirm it; it cannot make the status line say anything but a path it reported; nothing it sends
is ever offered as a grant. What it can do is report paths it never opened, which the line shows as reported. A
supervisor giving a running agent one more file is *not* delivered by stage 10: untrusted rooms are refused here
(item 10), and a room's own terminal has nothing to give. The `GRANTS` slot is the mechanism such a supervisor
would later hold (CP-24's shape). Target 3: a person works in `vi` without planning their grants. Networking: the
same line gives a host. Threads: the runtime's re-read is under the library's lock like any table change.

## Consequences

- **Positive.** A refusal is visible when it happens and tells a missing file from a missing grant. A person adds
  one file to a running program in the syntax they already use, without a prompt existing for a program to
  trigger, time, word or imitate, and without anything a program sends ever becoming a suggestion. Nothing is
  stored; rooms stay the way to keep. The existing hint stops printing unfiltered paths.
- **Negative.** No shipped precedent in text. The person types the path a second time and repeats a command, and
  `vi`'s "new file" on a refused path is still misleading inside `vi`. In a room's own terminal the line gives
  nothing. Not on the serial console, not for untrusted rooms. A running child does not receive a later grant.
  `narrow` grows a list. The status line states grants in a client's text.
- **Follow-up.** `GRANTS` in both records and runtimes; reports read live, filtered and quoted; `Status` and
  `Attention`; the status row in `term`; the chord in the compositor; the grant line in the shell's editor;
  `narrow` with a list of names; `jobs`; the hint texts and `help grants`; a step in `docs/tour.md`.

## Invariants

- **INV-0337-1.** A running program's table or network scope grows only by a grant line that the attention
  message of its own terminal opened and the person ended with Enter. No program output, no `REPORT`, no byte typed
  to a program and no message a client can send opens the line, puts text into it or gives anything. *Check:* a
  host test of the shell's input path feeding program output, reports and keys of every kind without the
  attention message and asserting no grant and an empty field; the compositor's host test of INV-0160-3 extended
  with System+G; a fuzz test of `term`'s input.
- **INV-0337-2.** While the grant line is open, no key and no pasted text reaches a program, the attention key
  never reaches one as bytes, and an Enter within 500 ms of the line's opening gives nothing. *Check:* a host test
  of the shell with a line in front in raw mode, keys and a paste before, during and after the grant line, and an
  Enter at 100 ms.
- **INV-0337-3.** What is given is exactly what the line's text parses to, as literal words, each resolved against
  the shell's own table and scope and refused if it reaches outside them. *Check:* the host tests of INV-0153-2,
  INV-0153-3, INV-0172-4 and INV-0224-1, run through the grant line's entry point, with a variable and a
  substitution refused, in a first terminal, in a room's own terminal and in a shell that entered a room.
- **INV-0337-4.** A grant made on the grant line goes to one element of the line, ends with that line and is
  stored nowhere. *Check:* a host test that the shell closes the write end and drops the record at the line's
  end; an in-QEMU script that gives a file to one `vi`, ends it, starts another and sees the refusal again, with
  `/config/rooms` unchanged; a pipeline of two programs in which only the named one can open the file, and in which a
  report
  written by the other while the line is open does not change the element.
- **INV-0337-5.** The status line's cells change only by the shell's `Status` message, and every path in a
  `Status` text or a printed hint passed item 2's filter; a report that does not is shown as `a path that cannot
  be shown`. *Check:* the fuzz test of INV-0096-1 with the status row's contents compared before and after; a
  host test of the shell with hand-written reports holding `ESC`, spaces, U+202E, a newline, a `;`, a Hebrew name, a
  name mixing Latin and
  Cyrillic letters, and 4096 bytes, asserting the text shown for each and the quoting of the hint.
- **INV-0337-6.** A refused path of a program of the line in front appears on the status line while it runs,
  worded by which of item 3's three cases the shell's own lookup finds, and the lookup opens nothing and follows
  no link object. *Check:* INV-0153-7's host test of the reporter extended to live reads and to (path, access);
  a host test of the lookup over a held path, a missing one, one outside the shell's table and a link object; a
  GUI script with `vi`.
- **INV-0337-7.** For a program of an untrusted room, and on the serial console, the attention key gives
  nothing. *Check:* a host test of the shell in a room marked untrusted and with the console as its terminal.

## Open questions

- Whether the grant line should be drawn by the compositor in the trusted material (a strip on the terminal's
  frame, as the chooser's sheet has), which would end the exception to ADR-0172's concept rule.
- Whether a suggestion should ever be offered. The first draft offered the refused path as a faint suggestion;
  the review showed that the path, the access and the moment are then the program's, accepted by three habitual
  keys. If typing proves too slow, a suggestion would need at least: frozen when the line opens, never a
  directory, never a path of another program's own directory, and a pause before it can be taken.
- A wider set of characters for shown paths: names in other scripts are ordinary on this system (ADR-0013,
  ADR-0163) and are hidden by item 2's filter; a set that admits one script per component and no right-to-left
  letters would show most of them.
- A word that gives one name to create (`+w` on a file that does not exist yet has to name its directory today);
  the chooser's `Save` already gives one name.
- Whether the key should open the graphical chooser as a sheet on the terminal window, and give what is picked.
- A way to give a running program of a room more from the room's own terminal, and the card for untrusted rooms
  (the path ADR-0172 point 4 uses for `Room { name }`, through the panel), once ADR-0268's gate has passed.
- Whether a running child should receive later grants through its parent's runtime.

## Revisit triggers

- People grant `+w ~` at the start to avoid the grant line: the line is too slow, or the defaults too narrow.
- A way is found for output, input or a report to open or fill the grant line, or to put words on the status
  line.
- A ported program in scope cannot recover from a first refusal without being restarted.
- The agent-isolation direction is chosen: a supervisor's programmatic holder of `GRANTS`.

## Architect's decision
