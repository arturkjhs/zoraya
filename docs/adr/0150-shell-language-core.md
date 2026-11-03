# ADR-0150: The shell language core: words, quoting, variables and globs

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0002, ADR-0003, ADR-0013
- **Evidence:** DM-008, DM-085, DM-095, CP-21, CP-22, GR-015, alternatives-postmortem.md#plan-9--inferno, shell/survey.md#posix-sh, shell/survey.md#zsh, shell/survey.md#plan-9-rc, shell/survey.md#fish, shell/survey.md#nushell, shell/survey.md#powershell, shell/survey.md#oils-and-ysh, shell/survey.md#classic-pitfalls, shell/survey.md#freebsd-sh-and-base-system

## Context

`zosh` (TASK-11) splits a line at spaces into at most 16 words and has no quoting, variables or globs. TASK-18 makes
it the shell people use on Zoraya, not a POSIX shell: dash covers `make` recipes, `configure` and `#!/bin/sh`
scripts (ADR-0005, ADR-0109). This ADR decides what a word is: how a line becomes the argument vector of `spawn`
(ADR-0003 rule 1, INV-0003-3: a vector, never one string). Control flow is ADR-0151, pipelines ADR-0152, grants
ADR-0153.

**What is known.** The survey's pitfall table (`shell/survey.md#classic-pitfalls`) shows that the commonest shell
mistakes come from three POSIX rules: unquoted values are split and globbed (`shell/survey.md#posix-sh`), an
unmatched glob is passed on literally, and file names starting with `-` become options (DM-095, DM-008). Every shell
that broke with POSIX for people fixed the first two: rc (`shell/survey.md#plan-9-rc`), zsh (`shell/survey.md#zsh`),
fish (`shell/survey.md#fish`), YSH (`shell/survey.md#oils-and-ysh`). Zoraya names are valid UTF-8 with no control
characters and no `/` (ADR-0013), so a name never contains a newline; spaces are allowed.

**What is assumed.** That the people using `zosh` know bash and will type bash habits; nothing measures how often.

## Decision drivers

1. A value is one argument unless the person says otherwise: no splitting or globbing of values
   (`shell/survey.md#classic-pitfalls`, DM-095).
2. Nothing silently becomes something else: an unmatched glob or an empty value in a word is reported, not passed on
   (`shell/survey.md#zsh` `NOMATCH`, `shell/survey.md#fish`).
3. Few rules, each stated once (fish's law of orthogonality, `shell/survey.md#fish`; Duff's one quoting rule,
   `shell/survey.md#plan-9-rc`).
4. What a bash user types either works the same or fails with a message that says what to type instead
   (POLA, `shell/survey.md#freebsd-sh-and-base-system`).
5. Strings are UTF-8 (CP-22, GR-015); variables are not inherited global configuration (CP-21, DM-085).

## Considered options

### Option A: POSIX `sh` words (baseline)
- **Description:** Field splitting on `IFS`, pathname expansion of every unquoted field, unmatched patterns left as
  they are, `name=value` assignment, `"$@"`.
- **Evidence for:** Every shell user knows it; dash already implements it on Zoraya; scripts from elsewhere run.
- **Evidence against:** It is the source of most pitfalls (`shell/survey.md#posix-sh`,
  `shell/survey.md#classic-pitfalls`); Duff's criticisms still hold (`shell/survey.md#plan-9-rc`). The task rules it
  out as Zoraya's face.
- **Cost:** *(estimate)* A POSIX word expander is large; dash is already ported for it.
- **Precedent:** sh, bash, FreeBSD `sh` (`shell/survey.md#freebsd-sh-and-base-system`).

### Option B: List variables, no rescanning (rc, fish)
- **Description:** Variables hold lists of strings. `$x` gives one argument per element and is never split or
  globbed. Two quote forms. Globs only in literal text. A glob matching nothing is an error.
- **Evidence for:** Removes the splitting class by construction (`shell/survey.md#plan-9-rc`: values are "not re-read
  when substituted"). fish and zsh show it works for daily use (`shell/survey.md#fish`, `shell/survey.md#zsh`).
  rc is the non-Unix precedent: Plan 9's shell (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** bash habits fail: `x=1`, `$@`, `${x:-d}`. fish users report this as the main cost
  (`shell/survey.md#fish`).
- **Cost:** *(estimate)* A tokenizer and expander of a few hundred lines in a host-tested crate.
- **Precedent:** rc (1990s), fish (2005 onwards), zsh's default.

### Option C: Typed values (Nushell, PowerShell)
- **Description:** Words are expressions with types: numbers, strings, lists, records; commands receive typed
  values; external programs receive their text form.
- **Evidence for:** Comparisons and arithmetic need no separate syntax; structured data is natural
  (`shell/survey.md#nushell`).
- **Evidence against:** Two parsing modes confuse people (`2+2` is 4 or a string, `shell/survey.md#powershell`);
  passing arguments to native programs was wrong for years in PowerShell; every native Zoraya program receives a
  byte-string vector anyway (ADR-0003), so types stop at the process boundary.
- **Cost:** *(estimate)* A type system, conversions and their documentation; far larger than B.
- **Precedent:** PowerShell, Nushell.

### Option D: B with stricter words (YSH)
- **Description:** As B, but an unmatched glob or empty value disappears (`nullglob`), lists are spliced only with
  an explicit `@x`, and splitting is a function.
- **Evidence for:** Explicit splicing makes the number of arguments visible in the source
  (`shell/survey.md#oils-and-ysh`).
- **Evidence against:** `@x` is one more sigil to learn; `nullglob` turns `rm *.tmp` with no match into `rm` with no
  operand, which a command may read as "nothing to do" or as a usage error.
- **Cost:** As B.
- **Precedent:** YSH.

## Recommendation

### Decision

Accepted as recommended (architect, 2026-11-03), with two additions from the tour: `count --lines FILE...` prints
the number of lines in the named files (rule 3), and a familiar Unix command name that Zoraya does not have gets the
same kind of hint as `x=1`, naming the Zoraya command (ADR-0154 rule 6).

The original recommendation follows.

Recommended: **Option B**, with the rules below. It is a recommendation, not a decision.

1. **Words and commands.** A line is commands separated by `;` or a newline. A command is words separated by spaces
   or tabs. `#` at the start of a word begins a comment to the end of the line.
2. **Quoting.** `'...'` is literal: nothing inside it is special, and it cannot contain `'` (as in POSIX; write
   `"it's"`). `"..."` is one word in which `$name`, `$name[i]`
   and `$(...)` are expanded and `\"`, `\\`, `\$`, `\n`, `\t` are escapes; nothing else is special. Outside quotes,
   `\` makes the next character literal (`my\ notes.txt`). Nothing else quotes.
3. **Variables.** `set name value...` sets a list (`set name` sets the empty list); `unset name` removes it. `$name`
   expands to one argument per element, never split, never globbed; the empty list gives no argument and an empty
   string gives one empty argument. `$name[i]` is element `i` from 1, negative from the end; out of range is an error.
   `count $name` prints the number of elements. `set` takes `--global` (ADR-0151 rule 2) and
   `--query NAME`, which succeeds when `NAME` is set and not empty (ADR-0151 rule 5). Names are letters, digits and `_`, starting with a letter or `_`.
   `name=value` alone is an error with a hint (example below). The shell's own messages have the form
   `zosh: SUBJECT: PROBLEM` with optional `  hint:` lines (ADR-0155 rule 5).
   *Added by the Decision:* `count --lines FILE...` prints the number of lines in the named files together (ADR-0155's
   Decision says how it reads them).
4. **A word that mixes text and a list.** A word yields one argument per element of the one list in it
   (`$files.bak`); a word with two lists of more than one element is an error; a word with an empty list and other
   text is an error ("`$files` is empty"). So `$dir/*` never becomes `/*`.
5. **Command substitution.** `$(command)` runs the command and gives one element per line of its output, without the
   line ends; inside `"..."` it gives the whole output as one string without the final line end. A substitution
   whose command fails makes the command it is in fail without running it, and the failure propagates by the stop rule
   (ADR-0151).
6. **The environment of programs.** Variables are the shell's own. `export name value` sets and marks a variable,
   `export name` marks an existing one; only marked variables go into a child's environment vector, as `name=value`,
   and a marked variable must hold exactly one element. `NAME=value command` adds `NAME` to that one command's
   environment. Nothing else is inherited (CP-21, DM-085).
7. **Globs.** In unquoted literal text only, never in the value of a variable or substitution: `*` (any characters
   but `/`), `?` (one character), `**` (any depth of directories). Matches are sorted byte by byte (ADR-0013 rule 3).
   A pattern that matches nothing is an error, except as the values of `for` (ADR-0151) and `set`, where it gives
   the empty list. A pattern matches a name starting with `.` only if the pattern's component starts with `.`. A
   match that would start with `-` is given as `./-name`, so it cannot be read as an option (DM-095).
8. **Home.** `~` at the start of a word, alone or before `/`, is the directory named by the setting `shell.home`
   (ADR-0157); elsewhere it is an ordinary character.
9. **Text.** Every word is UTF-8 (CP-22). Input that is not UTF-8 is refused by the line editor (ADR-0154) and by
   `source`, with the byte offset.

**Examples.** What a person types and sees (`zosh` prints its own messages as `zosh: ...`, ADR-0155):

```
~> set name 'my notes.txt'
~> echo $name
my notes.txt
~> ls $name
my notes.txt
~> set files *.txt
~> echo $files
a.txt b.txt my notes.txt
~> count $files
3
~> echo $files.bak
a.txt.bak b.txt.bak my notes.txt.bak
~> rm *.tmp
zosh: *.tmp: no file matches
~> x=1
zosh: x=1: not a command
  hint: to set a variable, type: set x 1
~> echo "you are in $(pwd)"
you are in /home
~> echo "it's" 'a*b'
it's a*b
```

**What a bash user will trip over, and why it is acceptable.**

| Typed from habit | What happens | Why acceptable |
|---|---|---|
| `x=1` | error with a hint | one way to set (rule 3); the hint says it |
| `"$x"` | works | quotes around one value are harmless |
| `$x` holding `a b` | one argument, not two | this is the fix (driver 1) |
| `$@`, `$1`, `$#` | `$args`, `$args[1]`, `count $args` (ADR-0151) | named lists replace special variables |
| `${x:-default}` | error | `set --query x \|\| set x default` (ADR-0151); one way |
| `rm *.tmp`, no match | error, nothing removed | the literal `*.tmp` was never wanted (driver 2) |
| `` `cmd` ``, `$((1+2))`, heredocs, `{a,b}` | not supported | `$(cmd)`; `math` (ADR-0155); files or `echo`; typing both names |
| `export X=1` | error with a hint (`export X 1`) | same rule as `set` |
| `grep`, `wc -l`, `man`, `less`, `which` (added by the Decision) | error with a hint naming `search`, `count --lines`, `help`, `cat`, `help` (ADR-0154 rule 6) | one name per action (ADR-0155) |
| `cmd \| read x`, `ls \| count` | error: a builtin can only start a pipeline (ADR-0152) | `set x $(cmd)`, `count $(ls)` |
| `echo *` with a file `-n` | prints `./-n` | the price of rule 7's protection |

### Effect of the vision

None on the choice. For target 2 (untrusted code, such as agents, typing commands) rule 7's refusal of globbed
leading dashes and rule 4's refusal of empty values remove two injection paths into a program's options.

**Tensions with accepted ADRs.** None. ADR-0003's vector (INV-0003-3) is what rule 1 builds; ADR-0013's names are
what rule 7 matches.

## Consequences
- **Positive.** The commonest scripting bugs cannot be written. One expander, testable on the host.
- **Negative.** bash scripts and habits do not carry over; dash is the answer for scripts (ADR-0005), and the hints
  are the answer for habits. No brace expansion or arithmetic syntax.
- **Follow-up.** The tokenizer and expander in a `no_std` crate with a fuzz target; the hint table.

## Invariants
- **INV-0150-1.** Expanding `$name` never yields a number of arguments different from the number of elements of
  `name`, whatever the elements contain. *Check:* a host property test of the expander with random lists of random
  UTF-8 strings, including spaces, `*`, `?`, quotes and `-`.
- **INV-0150-2.** A glob never yields an argument that starts with `-`, and a glob that matches nothing outside `for`
  and `set` is an error and runs no command. *Check:* host tests of the expander over a fake directory with names
  starting with `-` and `.`, and a test that the command runner is not called.
- **INV-0150-3.** The parser is total: any byte string gives a parse tree or an error with a position, never a
  panic, in bounded memory. *Check:* a deterministic random test in `make test-unit` and a coverage-guided target in
  `fuzz/`.
- **INV-0150-4.** A child's environment vector contains exactly the exported variables and the command's own
  `NAME=value` prefixes. *Check:* a host test of the spawn-request builder, and an in-QEMU test with `echo-args`
  printing its environment.

## Open questions
- Whether `${...}` forms or arithmetic deserve syntax after use shows a need; `math` is the first answer.
- Whether `**` should stop at a depth or a count to keep a mistaken `**` in `/` from walking the whole volume.

## Revisit triggers
- People repeatedly need to pass a list through one word (for example to join with commas): add a `join` builtin
  before syntax.
- People repeatedly need a `'` inside single quotes: add fish's `\'` escape there.

## Architect's decision

Accepted as recommended, 2026-11-03. Decided by the architect; recorded by an agent at the architect's request. Additions from the tour: `count --lines`, and hints for familiar Unix
names (ADR-0154 rule 6).
