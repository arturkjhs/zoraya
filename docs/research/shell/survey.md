# Shell survey for TASK-18

What a person typing commands gets from the shells people know, what confuses them, and the evidence. It is the
research behind proposed ADR-0150 to ADR-0158. Every source in [Sources](#sources) was fetched in this session
(2026-11-03); the note there says what was read. Text is paraphrased; short quotations are marked. Where a page could
not be fetched, the claim rests on a catalog entry (`DM-`, `GR-` in `../design-mistakes.md` and `../held-up-well.md`)
or is left out. Three primary pages were refused to this session (Oils' own site with 403, the Monad Manifesto PDF
was unreadable here, the bash manual with 429); the Oils documents were read from the project's GitHub mirror, and
Snover's argument is cited through DM-020, which quotes it.

## POSIX sh

**Good at.** One language that runs everywhere a script lands; a small grammar; pipelines and redirection as the
composition model (GR-002). Exit status 127 for "not found" and 126 for "found but not executable", and a status
above 128 for a command ended by a signal, are written down [SH1, 2.8.2].

**Confusing.**
- *Field splitting.* The result of an unquoted parameter expansion, command substitution or arithmetic expansion is
  split on `IFS`, and an empty or all-blank field disappears [SH1, 2.6.5]. Then every field is a pathname pattern
  [SH1, 2.6.6]. So `$f` is not one argument unless quoted.
- *Globs that match nothing.* "If the pattern does not match any existing filenames or pathnames, the pattern string
  shall be left unchanged" [SH1, 2.14.3]: `rm *.tmp` in a directory with none runs `rm` on a file literally named
  `*.tmp`.
- *Exit status of a pipeline* is the last command's unless `pipefail` is set [SH1, 2.9.2]; a failing producer is
  invisible.
- *`set -e`.* It is ignored in the condition of `if`, `while`, `until`, after `!`, in every command of an `&&`/`||`
  list but the last, and a compound command whose failure happened while it was ignored does not trigger it either
  [SH1, `set`]. A function called from an `if` therefore runs with error checking off for its whole body.

**Evidence of confusion.** The Wooledge BashPitfalls page, the community's reference list, opens with fifteen
pitfalls of which five are word splitting, three are globbing and four involve file names that start with `-`
[SH4]. Wheeler documents that names with spaces, newlines, tabs and leading dashes break ordinary scripts, that "all
simple line-at-a-time processing of pathnames is wrong", and recommends quoting every expansion, `./*` instead of
`*`, and NUL-separated lists as defences [SH5]. DM-095 and DM-008 record the same class from the security side.

## FreeBSD sh and base system

FreeBSD makes POSIX `sh` usable interactively rather than replacing it. Its `sh` is "close to" the POSIX shell, with
only POSIX features plus a few Berkeley extensions [SH2]. On top of that: emacs (`-E`) and vi (`-V`) editing through
the editline library, emacs mode switched on by itself for an interactive terminal, `fc` and a history file
(`HISTFILE`, `HISTSIZE`), `bind` for keys, `set -o pipefail`, and an `ENV` file sourced only by interactive shells
[SH2]. FreeBSD 14.0 made it root's default shell: "The default shell for the root user is now sh(1), which has many
new features for interactive use" [SH3].

The base-system conventions around the shell are what make FreeBSD feel coherent:
- **POLA.** "As FreeBSD evolves, changes visible to the user should be kept as unsurprising as possible";
  rearranging startup variables in `/etc/defaults/rc.conf` is given as a violation [SH6].
- **One configuration file.** `rc.conf` holds the host name, interfaces and which services start, as `name=value`
  assignments in `sh` syntax; `/etc/defaults/rc.conf` holds every default, and `rc.conf` only overrides [SH7].
- **A command to edit it.** `sysrc` changes `rc.conf` variables "in a safe and effective manner": `sysrc name`
  shows, `sysrc name=value` sets, `+=`/`-=` edit lists, `-x` removes, `-a` lists what is not default, `-c` checks
  without changing [SH8].
- **One front end for services.** `service name start|stop|status`, run in the same restricted environment as at
  boot; `-e` lists enabled services [SH9].
- **One style for utilities.** style(9): options parsed by `getopt(3)`, sorted; a usage line in the manual's
  SYNOPSIS form (`usage: f [-aDde] [-b b_arg] req1 [opt1]`) printed to standard error; exit 0 on success and 1 on
  failure; errors through `err(3)`/`warn(3)`, "do not roll your own" [SH10]. `err(3)` prints the program's name, a
  colon and a space, the message, and for `err`/`warn` a colon and the system's error text [SH11].
- **One manual format.** mdoc(7) is semantic markup with fixed section order: NAME, SYNOPSIS, DESCRIPTION, ...,
  EXIT STATUS, EXAMPLES, DIAGNOSTICS, ..., SEE ALSO [SH12].

What confuses: the shell itself is still POSIX (every pitfall above applies); the conventions are kept by review,
not by a mechanism, so a utility's usage line, manual and behaviour can disagree.

## bash

**Good at.** It is the baseline people know: Ctrl-R history search, Tab completion, `$(...)`, `[[ ]]`, arrays,
`pipefail`, job control with `%n`, `fg`, `bg` and the `[1] 25647` line [SH13].

**Confusing.** Everything in POSIX sh, plus the exceptions of `set -e`. BashFAQ 105 lists failures of errexit that
surprise experienced users: `let i++` when `i` is 0 ends the script; a function called from `if` or `&&` runs with
errexit off; `local x=$(cmd)` hides the failure; command substitution ignores it without `inherit_errexit`;
`pipefail` can fail intermittently depending on pipe buffer sizes. Two of its three maintainers' recommendations are
not to use it and to check errors explicitly [SH14].

## zsh

zsh broke with POSIX on the most common pitfall: it does not split unquoted parameter expansions. Its FAQ says the
variable "remains intact", that splitting causes surprises with innocuous spaces, and that "the natural way to produce
word-splitting behaviour in zsh is via arrays"; `SH_WORD_SPLIT` restores the old rule for sh and ksh emulation
[SH15, SH16]. `NOMATCH`, on by default, makes a glob with no match an error "instead of leaving it unchanged" [SH16].
`CORRECT` offers spelling correction of commands, with a documented risk of false reports [SH16]. What confuses:
scripts written for zsh and run by `sh` (or the reverse) behave differently on the same text; the option set is large
enough that two zsh users may not run the same language.

## Plan 9 rc

Duff's `rc` is the earliest deliberate break with the Bourne shell and the clearest statement of why [SH17]:
- "Values of variables, including command line arguments, are not re-read when substituted into a command": there is
  no rescanning, so no splitting or injection through values. Variables are therefore lists, `(a b c)`, and `()` is
  not `''`.
- One quoting rule: quotes are used only when a syntax character must appear in an argument or for the empty string;
  inside quotes a quote is written twice.
- `^` concatenates, pairwise over lists; `$status` is a string, empty on success, and a pipeline's status joins its
  parts with `|`; `if not`, `switch`, `fn`, and `~` for pattern matching without file name expansion.
- Duff's criticism of Bourne: arguments containing spaces make scripts "run wild"; nested command substitution needs a
  number of backslashes "exponential in the nesting depth"; `IFS` is a security hole in set-uid programs; "nobody
  really knows what the Bourne shell's grammar is". He concedes that Bourne's `if ... else ... fi` is better than
  rc's `if not`.

Plan 9 also removed the need for job control: windows replace it, and the window system gives each window its own
`/dev/cons` (`alternatives-postmortem.md`, Plan 9; DM-070).

## fish

fish is a deliberately non-POSIX shell for people at a keyboard. Its design document states five laws: a small set
of orthogonal features; stay responsive and block only on what the user asked for; "configurability is the root of
all evil", each option being a failure to decide; design the user interface first; and discoverability, so that
features are learnt from the program itself [SH18].

**Language** [SH19]. Variables are lists; a value "stays as it is" with no word splitting; command substitution
splits only on newlines; `set name value` instead of `name=value` (`NAME=value cmd` remains for one command's
environment); `$status`, `$argv`; blocks end with `end`; no heredocs and no subshells; only `*` and `**` as globs,
not applied to the values of variables, and a glob that matches nothing fails the command except in `for`, `set` and
`count`; `string` and `math` builtins instead of parameter-expansion syntax. Combiners `and`/`or`/`not` and `&&`/`||`
exist side by side; `$pipestatus` holds every status of the last pipeline [SH21].

**Interactive** [SH20]. Autosuggestions from history, completions and valid paths as greyed text, accepted with the
right arrow or Ctrl-F, never run unless accepted; syntax highlighting that marks a non-existent command red; Tab
inserts the unambiguous part and opens a navigable list otherwise; Up and Down search history for lines containing
what was typed; Ctrl-R opens a searchable history list; `help` and `-h` on builtins; universal variables shared by all
sessions; abbreviations that expand visibly before the line runs. Completions are declared with
`complete -c cmd -s o -l output -d description`, loaded per command from a search path, and can be generated from the
system's manual pages [SH22]. History substitution (`!!`) is left out as "an awkward interface that was invented
before interactive line editing was even possible" [SH23].

**Confusing.** Everything a bash user has in muscle memory: `x=1`, `$(...)`-heavy idioms of older versions, heredocs,
`export X=1`, and scripts found on the web. The FAQ's own warning is that a `config.fish` that prints output breaks
`scp` and `rsync` when fish is the login shell [SH23].

## Nushell

Nushell is a shell and a language whose commands pass structured values (tables, records, lists) [SH24].
- The whole source is parsed before it runs; there is no runtime `eval`, so "sourced files need to be available
  before your script is run"; variables are immutable by default; the last value of a block is its result [SH24].
- External programs' output arrives as bytes, turned into text when it is valid UTF-8; an external command that exits
  non-zero makes "the whole pipeline" fail, like `pipefail`, unless caught with `try` or `complete` [SH25].
- Lists are not spread into arguments of an external command implicitly; `...$list` spreads them; `^name` forces the
  external program over a builtin of the same name [SH26].
- Comments above `def` become the command's help text, shown by `help` and in completion menus; parameter comments
  describe parameters; every command gets `-h`/`--help`; a `directory` parameter completes only directories [SH27].
- For bash users: `>` is a comparison, so redirection is `o> file` or `| save file`; `$env.PATH` is a list; `;`
  instead of `&&`, `try`/`catch` instead of `||` [SH28].

**Confusing.** The data model is powerful but large; text tools need explicit conversions (`lines`, `from json`);
structure exists only inside Nu, so a POSIX program in the middle of a pipeline sees text (DM-020's point that object
pipelines bind to one runtime and that Nushell gets structure largely by reimplementing tools as builtins).

## PowerShell

PowerShell pipes .NET objects; parameters accept pipeline input by value or by property name, one object at a time,
and a pipeline whose objects cannot be bound fails with a binding error [SH29]. Snover's case for it (DM-020) is that
text tools bind fetching, processing and rendering together, so the next tool does "prayer-based parsing".
- *Consistency by mechanism.* Command names are verb-noun with an approved verb list: use `Remove`, never `Delete`;
  verbs come in pairs (`Get`/`Set`, `Start`/`Stop`) [SH30]. Help is generated: `Get-Help` builds the name, syntax and
  parameter list from the declaration and takes descriptions from comment-based help [SH31].
- *Interactive.* PSReadLine gives syntax colouring, a visual mark of syntax errors, history search and predictive
  suggestions from history, off by default [SH32].
- *Confusing.* Two parsing modes (expression and argument) decide whether `2+2` is 4 or the string `2+2` [SH33].
  Passing arguments to native programs has been wrong for years: arguments are joined into one string, quotes were
  lost until 7.3 changed the default (a "breaking change"), and `--%` exists to stop parsing altogether [SH33].
  Errors: `$ErrorActionPreference` defaults to `Continue`, so most errors do not stop a script, and a native
  program's non-zero exit is not an error unless `$PSNativeCommandUseErrorActionPreference` is set, which defaults to
  false [SH34]. Until 7.4, binary output piped from a native program was converted to strings and corrupted [SH29].

## Oils and YSH

Oils is an "upgrade path from bash": OSH "runs your existing shell scripts" and YSH is a new language for people who
avoid shell [SH35].
- *Simple word evaluation.* In YSH an unquoted `$x`, `$(cmd)` or `$((...))` is exactly one argument: never split,
  globbed or dropped when empty; `@array` splices; splitting and globbing of values are explicit functions; globs
  matching nothing produce nothing (`nullglob`); `dashglob` is off, so a glob never yields a name starting with `-`
  [SH36].
- *Errors.* The goal is to "never silently ignore an error, and never lose an exit code": errexit is on, failures in
  command substitutions count, `try { } ` with `_error.code` handles a failure, `strict_errexit` forbids calling
  functions or substitutions in a condition (the bash trap), `boolstatus` separates "false" (1) from an error,
  `sigpipe_status_ok` ignores the `yes | head` SIGPIPE [SH37].

**Confusing.** Two languages in one project, chosen by options; the documentation calls shell error handling "damned
if you do and damned if you don't" [SH37].

## Capability shells and grant syntax

No mainstream shell decides what a program may reach; the user's identity does (DM-009, DM-010). Four precedents do:
- **Plash** (Seaborn) ran Linux programs with less authority than the caller. Its shell wrote grants into the
  command line, as in `gcc -c foo.c => -o foo.o + .` (the post does not define `=>` and `+`), and its programmatic
  form `plash-exec` grants read-only access unless a `w` flag asks for read-write, with read-only `/usr`,
  `/bin`, `/lib` and `/etc` given to every program unless `--no-default` [SH39].
- **SHILL** (OSDI 2014) is a scripting language where contracts declare what a script, and the programs it runs, may
  access, enforced by language and sandbox; the authors call contracts "declarative security policies" that document
  a script for its users [SH40].
- **Deno** is "secure by default": no files, network, environment or subprocesses unless granted by flags such as
  `--allow-read=./data`, `--allow-write`, `--allow-net=host`, `--allow-run=git`; `--deny-*` overrides; a missing grant
  is an error the program can catch, or, at a terminal, a prompt [SH41].
- **OpenBSD `unveil`**: the first call hides the whole filesystem except the named path, with permissions `r`, `w`,
  `x`, `c` (create and remove); later calls add paths; then the list should be locked [SH42].

Plan 9's per-process namespaces (GR-004) and Fuchsia's table of path prefixes to handles are the same idea at the
process level; Capsicum showed that programs opening paths late are where a capability model hurts (GR-008).

## Command-line conventions

- **POSIX Utility Syntax Guidelines** [SH43]: one-character options after `-` (guidelines 3, 4), grouping of options
  without arguments (5), options before operands (9), `--` ends options (10).
- **style(9) and err(3)**: above, under FreeBSD [SH10, SH11].
- **Command Line Interface Guidelines** (clig.dev) [SH44]: show help for `-h` and `--help`, "lead with examples";
  human-readable output first, machine formats as options; errors rewritten for humans with what to do next, the most
  important information last; colour "with intention", off when not a terminal, when `NO_COLOR` is set or `TERM` is
  `dumb`; every short flag has a long form; zero for success, non-zero for failure, distinct codes for the important
  failures; "if you change state, tell the user".

## Classic pitfalls

| Pitfall | What happens | Source | Fix others chose |
|---|---|---|---|
| Word splitting | An unquoted value with a space becomes two arguments; an empty one disappears | SH1 2.6.5, SH4 #1, #2, #14, SH5 | No splitting of values: rc, zsh, fish, YSH (SH17, SH15, SH19, SH36) |
| Quoting | Several quoting forms with different rules; nested substitution needs exponential backslashes | SH17 | rc's one rule; fish's two quotes |
| Unmatched glob | Pattern passed on literally | SH1 2.14.3, SH5 | Error: zsh `NOMATCH`, fish; empty: YSH `nullglob` |
| Leading dash | A file named `-n` becomes an option | SH4 #3, SH5, DM-095 | `./*`, `--`; YSH `dashglob` off |
| `set -e` | Off inside conditions and functions called from them; lost in `local x=$(...)` | SH1 `set`, SH14 | YSH: on everywhere, `try`, `strict_errexit` (SH37); Nushell: failing external fails the pipeline (SH25) |
| Exit statuses | One integer mixes "false", "error" and "killed by signal N" (128+N) | SH1 2.8.2 | rc's string `$status`; YSH `boolstatus`; Zoraya's exit records (ADR-0004) |
| Pipeline status | Last command only | SH1 2.9.2 | `pipefail`, fish `$pipestatus`, rc's joined status |
| Text-only pipelines | Structure re-parsed by every consumer | DM-020, GR-002 | PowerShell objects, Nushell values (SH29, SH24), each bound to its own runtime |

## What this suggests for zosh

Not decisions; inputs to ADR-0150 to ADR-0158.
1. Every shell that broke with POSIX for people fixed the same three things first: no splitting of values, a glob
   that matches nothing is not passed on silently, and errors are not silently lost. None of them regretted it in the
   pages read; the cost is always the same: bash habits and scripts from the web do not work.
2. Discoverability comes from one description per command that feeds help and completion (Nushell, PowerShell,
   fish's completions); conventions that are only reviewed drift (FreeBSD's style(9) is written but not enforced).
3. Structured pipelines buy precision at the price of a runtime that every tool must share; at the boundary with
   byte programs both PowerShell and Nushell had to add escape hatches.
4. Capability shells exist but are research or niche (Plash, SHILL); the shapes that a person can read are Deno's
   flags and unveil's letters. Nothing in this survey shows a capability shell used daily by many people: Zoraya would
   be early here, which is a risk to state.

## Sources

Each was fetched on 2026-11-03 (this session). "Primary" means the project's own documentation or the authors' paper.

- [SH1] The Open Group, POSIX.1-2024, Shell Command Language, https://pubs.opengroup.org/onlinepubs/9799919799/utilities/V3_chap02.html, primary. Read 2.6.5 field splitting, 2.6.6 pathname expansion, 2.8.2 exit status, 2.9.2 pipeline exit status and `pipefail`, 2.14.3 unmatched patterns, the `set -e` exceptions.
- [SH2] FreeBSD, `sh(1)`, https://man.freebsd.org/cgi/man.cgi?query=sh&sektion=1&format=ascii, primary. Editing modes, history, `bind`, `ENV`, `pipefail`, conformance statement.
- [SH3] FreeBSD 14.0-RELEASE release notes, https://www.freebsd.org/releases/14.0R/relnotes/, primary. `sh` as root's default shell.
- [SH4] Wooledge wiki, BashPitfalls, https://mywiki.wooledge.org/BashPitfalls, community reference. The first fifteen pitfalls.
- [SH5] D. A. Wheeler, "Filenames and Pathnames in Shell: How to do it Correctly", https://dwheeler.com/essays/filenames-in-shell.html, secondary (essay). Problems and defensive idioms.
- [SH6] FreeBSD Handbook, Glossary, https://docs.freebsd.org/en/books/handbook/glossary/, primary. POLA.
- [SH7] FreeBSD, `rc.conf(5)`, https://man.freebsd.org/cgi/man.cgi?query=rc.conf&sektion=5&format=ascii, primary. Purpose, defaults file, syntax.
- [SH8] FreeBSD, `sysrc(8)`, https://man.freebsd.org/cgi/man.cgi?query=sysrc&sektion=8&format=ascii, primary.
- [SH9] FreeBSD, `service(8)`, https://man.freebsd.org/cgi/man.cgi?query=service&sektion=8&format=ascii, primary.
- [SH10] FreeBSD, `style(9)`, https://man.freebsd.org/cgi/man.cgi?query=style&sektion=9&format=ascii, primary. `getopt`, usage format, exit codes, `err`/`warn`.
- [SH11] FreeBSD, `err(3)`, https://man.freebsd.org/cgi/man.cgi?query=err&sektion=3&format=ascii, primary. Message format.
- [SH12] FreeBSD, `mdoc(7)`, https://man.freebsd.org/cgi/man.cgi?query=mdoc&sektion=7&format=ascii, primary. Section order.
- [SH13] GNU Bash manual, Job Control Basics, https://www.gnu.org/software/bash/manual/html_node/Job-Control-Basics.html, primary. Job numbers, jobspecs, Ctrl-Z, `fg`, `bg`. (The Word Splitting page of the same manual returned 429; nothing rests on it.)
- [SH14] Wooledge wiki, BashFAQ/105, https://mywiki.wooledge.org/BashFAQ/105, community reference. Why `set -e` fails.
- [SH15] zsh FAQ, chapter 3, https://zsh.sourceforge.io/FAQ/zshfaq03.html, primary. Entry 3.1, no splitting of `$var`.
- [SH16] zsh manual, Options, https://zsh.sourceforge.io/Doc/Release/Options.html, primary. `NOMATCH`, `SH_WORD_SPLIT`, `CORRECT`.
- [SH17] T. Duff, "Rc — The Plan 9 Shell", https://9p.io/sys/doc/rc.html, primary (author's paper).
- [SH18] fish, Design, https://fishshell.com/docs/current/design.html, primary.
- [SH19] fish, Fish for bash users, https://fishshell.com/docs/current/fish_for_bash_users.html, primary.
- [SH20] fish, Interactive use, https://fishshell.com/docs/current/interactive.html, primary.
- [SH21] fish, The fish language, https://fishshell.com/docs/current/language.html, primary. Job control, `$status`, `$pipestatus`, combiners, universal variables' file.
- [SH22] fish, Writing your own completions, https://fishshell.com/docs/current/completions.html, primary.
- [SH23] fish, FAQ, https://fishshell.com/docs/current/faq.html, primary. `!!`; output in `config.fish`.
- [SH24] Nushell book, Thinking in Nu, https://www.nushell.sh/book/thinking_in_nu.html, primary.
- [SH25] Nushell book, Pipelines, https://www.nushell.sh/book/pipelines.html, primary. External output, exit codes.
- [SH26] Nushell book, Running External (System) Commands, https://www.nushell.sh/book/running_externals.html, primary.
- [SH27] Nushell book, Custom Commands, https://www.nushell.sh/book/custom_commands.html, primary. Comments as help.
- [SH28] Nushell book, Coming from Bash, https://www.nushell.sh/book/coming_from_bash.html, primary.
- [SH29] Microsoft, about_Pipelines, https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_pipelines, primary.
- [SH30] Microsoft, Approved Verbs for PowerShell Commands, https://learn.microsoft.com/en-us/powershell/scripting/developer/cmdlet/approved-verbs-for-windows-powershell-commands, primary.
- [SH31] Microsoft, about_Comment_Based_Help, https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_comment_based_help, primary. Autogenerated syntax and parameter list.
- [SH32] Microsoft, about_PSReadLine, https://learn.microsoft.com/en-us/powershell/module/psreadline/about/about_psreadline, primary.
- [SH33] Microsoft, about_Parsing, https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing, primary. Modes, native argument passing, `--%`.
- [SH34] Microsoft, about_Preference_Variables, https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_preference_variables, primary. `$ErrorActionPreference`, `$PSNativeCommandUseErrorActionPreference`.
- [SH35] Oils, `README.md`, https://github.com/oils-for-unix/oils/blob/master/README.md (mirror; oils.pub returned 403), primary.
- [SH36] Oils, "Simple Word Evaluation", https://github.com/oils-for-unix/oils/blob/master/doc/simple-word-eval.md, primary.
- [SH37] Oils, "YSH Error Handling", https://github.com/oils-for-unix/oils/blob/master/doc/error-handling.md, primary.
- [SH39] M. Seaborn, plash mailing list, August 2005, https://lists.endsoftwarepatents.org/archive/html/plash/2005-08/msg00001.html, primary (author). `plash-exec` grant options and default endowment; the shell example `gcc -c foo.c => -o foo.o + .`.
- [SH40] S. Moore, C. Dimoulas, D. King, S. Chong, "SHILL: A Secure Shell Scripting Language", OSDI '14, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/moore, primary (abstract page).
- [SH41] Deno, Security and permissions, https://docs.deno.com/runtime/fundamentals/security/, primary.
- [SH42] OpenBSD, `unveil(2)`, https://man.openbsd.org/unveil.2, primary.
- [SH43] The Open Group, POSIX.1-2024, Utility Conventions, https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap12.html, primary. 12.2 guidelines.
- [SH44] Command Line Interface Guidelines, https://clig.dev/, secondary (practitioners' guide).
