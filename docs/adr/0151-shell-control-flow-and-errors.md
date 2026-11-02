# ADR-0151: Shell control flow, functions, and how failures are noticed

- **Status:** proposed
- **Date:** 2026-11-03
- **Depends on:** ADR-0004, ADR-0034, ADR-0046, ADR-0150
- **Evidence:** DM-017, DM-027, CP-06, CP-12, alternatives-postmortem.md#plan-9--inferno, shell/survey.md#posix-sh, shell/survey.md#bash, shell/survey.md#plan-9-rc, shell/survey.md#nushell, shell/survey.md#powershell, shell/survey.md#oils-and-ysh, shell/survey.md#classic-pitfalls

## Context

`zosh` today runs one command per line and prints a short report when a program does not exit 0: `[exited 3]`,
`[terminated: Interrupt]`, `[fault: data abort, class 0x24]`, `[killed]` (`docs/design.md`, "Using the shell"). The
kernel gives every ended process an `ExitRecord { code, reason, detail, request_kind }` with reasons `Exited`,
`Terminated`, `Killed`, `Fault`, `Limit` (ADR-0004 rule 1, ADR-0046 Decision, ADR-0034): the end of a process is a
typed value (CP-06, CP-12), not POSIX's single integer where 130 means "killed by SIGINT" and also "exited 130"
(`shell/survey.md#posix-sh`). This ADR decides `if`, loops, functions, what counts as a failure, what a failure
stops, and how exit records appear in the language and on screen.

**What is known.** POSIX `set -e` is off in conditions and in every function called from one, so errors are lost in
the places people least expect (`shell/survey.md#posix-sh`, `shell/survey.md#bash`). YSH turns errexit on everywhere,
forbids the trap cases and adds `try` (`shell/survey.md#oils-and-ysh`); Nushell fails a pipeline whose external
program exits non-zero (`shell/survey.md#nushell`); PowerShell continues by default and ignores native exit codes
unless asked (`shell/survey.md#powershell`). rc keeps `$status` as a string, empty for success
(`shell/survey.md#plan-9-rc`).

## Decision drivers

1. A failure is never silently lost (`shell/survey.md#classic-pitfalls`, the `set -e` row).
2. One rule for scripts and the prompt: what a line does at the prompt it does in a script.
3. The reason a program ended is shown in words a person reads, and kept as data a script can test, without
   folding it into an integer (CP-06, CP-12).
4. Control flow a bash user can read without a manual: `if`, `else`, `for`, `while`, functions.
5. No asynchronous interruption of the shell (DM-017, DM-027): Ctrl-C is a request to the foreground job
   (ADR-0004 rule 8, ADR-0158), never a handler inside the shell's own code.

## Considered options

### Option A: POSIX statuses and `set -e` off (baseline)
- **Description:** `if ...; then ...; fi`, `$?` as an integer, 128+N for signals, failures ignored unless `set -e`,
  with its exceptions.
- **Evidence for:** Known to everyone; lets a script continue past an expected failure without ceremony.
- **Evidence against:** Lost errors (`shell/survey.md#bash`); an integer cannot say "fault" or "killed"
  (`shell/survey.md#classic-pitfalls`); the measured W9 build relies on 130 meaning SIGINT precisely because nothing
  else is available (ADR-0004 driver 4).
- **Cost:** *(estimate)* Small.
- **Precedent:** sh, bash.

### Option B: Failures stop the enclosing block unless checked (YSH-style, as exceptions)
- **Description:** A command that does not exit 0 fails. A failure stops the rest of the line, block, function or
  script, outward, until something that checks it: a condition (`if`, `while`), `!`, `&&`, `||`, or `try`.
  Functions called in a condition are not exempt: a failure inside one stops the function, which then fails.
- **Evidence for:** Closes the `set -e` holes by construction (`shell/survey.md#oils-and-ysh`); Nushell does the same
  for external programs (`shell/survey.md#nushell`). The prompt and scripts behave alike (driver 2).
- **Evidence against:** `a; b` at the prompt no longer runs `b` when `a` fails, which bash users do not expect; a
  program that uses a non-zero status for "no" (a search that finds nothing) must be checked with `if` or `||`.
- **Cost:** *(estimate)* The evaluator returns a result per command; a few hundred lines.
- **Precedent:** YSH, Nushell (for external programs), exceptions in most programming languages.

### Option C: Continue by default, errors reported (PowerShell)
- **Description:** Every failure is reported on screen and the script continues; `try` or a preference stops it.
- **Evidence for:** Nothing is lost from the screen; long interactive lines keep going.
- **Evidence against:** PowerShell's default `Continue` and its ignoring of native exit codes are the documented
  confusion (`shell/survey.md#powershell`); a script that continues after a failed `cd` acts in the wrong directory.
- **Cost:** *(estimate)* Small.
- **Precedent:** PowerShell.

### Option D: rc's string status, no stop rule
- **Description:** `$status` is the exit reason as a string, empty for success; `if not`; no automatic stop.
- **Evidence for:** Typed status without integers (`shell/survey.md#plan-9-rc`), Plan 9's shell
  (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Errors are as easy to lose as in sh; Duff himself prefers Bourne's `if ... else`.
- **Cost:** Small.
- **Precedent:** rc.

## Recommendation

Recommended: **Option B**, with D's typed status. It is a recommendation, not a decision.

1. **Blocks.** Braces delimit blocks; a block may span lines.
   - `if COMMAND { ... } else if COMMAND { ... } else { ... }`: the condition is true when the command succeeds.
   - `for NAME in WORDS { ... }`, `while COMMAND { ... }`, `break`, `continue`.
   - `!` before a command inverts success; `A && B` runs B if A succeeded, `A || B` runs B if A failed.
   - `try { ... } catch { ... }`: runs the first block; if a failure stops it, runs the second with `$status` set to
     that failure. `try` without `catch` runs the block and swallows a failure.
2. **Functions.** `fn NAME PARAM... { ... }` defines a command. Arguments bind to the named parameters in order;
   all arguments are also in `$args`; too few arguments is a usage error naming the parameter. `return` ends a
   function with success, `return N` with exit status N (1 to 255, a failure). Comment lines directly above `fn` are
   its help (ADR-0156). A function's variables are local unless `set --global`.
3. **Success and failure.** A command succeeds when its record is `Exited` with code 0. A builtin or function
   succeeds or fails as its page says. Anything else fails.
4. **The stop rule.** A failure that is not checked stops the line, block, function or script that contains it, and
   propagates outward; at the prompt it stops the rest of the line. It is checked by the condition of `if` or
   `while`, by `!`, by being the left side of `&&` or `||`, or by `try`. The check applies to the command written
   there, including the whole body of a function called there: inside that body the stop rule still holds, and the
   function's failure is what the condition sees. A pipeline fails when any element fails (ADR-0152).
5. **`$status`.** After every command, `$status` is a one-element list: the decimal exit code for `Exited`
   (`0` on success), or one of `interrupted`, `terminated`, `hung-up`, `quit`, `request-N` (a `Custom(N)` request),
   `killed`, `fault`, `limit`. `status` prints the whole last record in words; `status --code`, `--reason`,
   `--detail` print its fields. `set --query NAME` succeeds when `NAME` is set and not empty.
6. **On screen.** The shell prints nothing for a success. A program that exits with a non-zero code has said why on
   its error stream (ADR-0155), so at the prompt the shell adds nothing; `status` shows the record. For every other
   end of an unchecked failure the program could say nothing, and the shell prints one line, `NAME: WHAT`, before the
   next prompt:

   | Record | Line |
   |---|---|
   | `Exited`, code N | none at the prompt; `make: exit status 2` where a script stops (below) |
   | `Terminated`, `Interrupt` | `spin: interrupted` (with `(Ctrl-C)` when the terminal sent it) |
   | `Terminated`, `Terminate` / `Hangup` / `Quit` | `spin: terminated` / `hung up` / `quit` |
   | `Terminated`, `Custom(n)` | `spin: ended by request n` |
   | `Killed` | `spin: killed` |
   | `Fault`, class C | `fault: crashed (data abort, class 0x24)` |
   | `Limit` | `hog: stopped at its limit` |

   A failure that stops a script adds where: `zosh: build.zosh line 4: tcc: exit status 1` and
   `(the rest of build.zosh was not run)`. A checked failure prints nothing; its command's own error message, if
   any, is the program's (ADR-0155).
7. **Exit codes of scripts.** A script or `zosh -c` ends with the status of its last command, or with the failure
   that stopped it; `exit N` ends it with N.

**Examples.**

```
~> if test -d build { echo yes } else { echo no }
no
~> mkdir build; echo made
made
~> mkdir build; echo made
mkdir: build: already exists
~> mkdir build || echo 'it was there'
mkdir: build: already exists
it was there
~> for f in *.txt { cp $f $f.bak }
~> fn greet who {
...>   echo "hello, $who"
...> }
~> greet
greet: missing argument WHO
usage: greet WHO
~> greet Ada
hello, Ada
~> spin
^C
spin: interrupted (Ctrl-C)
~> echo $status
interrupted
~> try { cat missing.txt } catch { echo "status was $status" }
cat: missing.txt: not found
status was 1
~> fault
fault: crashed (data abort, class 0x24)
```

### Effect of the vision

For target 2 a script that stops at its first unchecked failure is easier to reason about when an agent writes it:
a failed `cd` or `mkdir` cannot let later commands act somewhere unintended.

**Tensions with accepted ADRs.** None. The words in item 6 replace TASK-11's bracketed reports, which no ADR fixes.

## Consequences
- **Positive.** No lost errors, one rule everywhere, readable reports, typed reasons kept as data.
- **Negative.** `a; b` stops after a failed `a`, which bash users must learn; commands that use status 1 for "no"
  need a check. The stop rule needs tests for every construct.
- **Follow-up.** The evaluator; the report table in the shell crate (it extends today's `Report`); TASK-11's test
  scripts change their expected lines.

## Invariants
- **INV-0151-1.** An unchecked failure runs no later command of its line, block, function or script, and a checked
  one runs exactly the branch that checks it. *Check:* host tests of the evaluator with a fake runner over every
  construct (sequence, `if`, `while`, `for`, `&&`, `||`, `!`, `try`, functions called as conditions, nested blocks).
- **INV-0151-2.** `$status` and the screen line are a function of the exit record alone, for every reason and
  request kind. *Check:* a host test over every `ExitReason` and `RequestKind` with codes 0, 1, 255 and details 0
  and 2^32-1.
- **INV-0151-3.** The shell treats a record as success exactly when it is `Exited` with code 0. *Check:* the same
  host test, asserting no other record is a success, including `Terminated` with detail 0.

## Open questions
- Whether `try` should also offer the failing command's name and line as variables, as YSH's `_error` does.
- Whether a "no" status (1) and an error status (2 and up) should be distinguished in conditions, as YSH's
  `boolstatus` does; ADR-0155 keeps 1 for both "no" and "failed" for now.

## Revisit triggers
- People report the stop rule at the prompt as surprising often enough: make `;` at the prompt continue and print the
  failure, keeping the rule in scripts.
- A ported workflow needs the full record of every element of a pipeline: add `$pipestatus`.

## Architect's decision
