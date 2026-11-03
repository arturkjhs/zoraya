# ADR-0152: Shell pipelines: byte streams between programs, lists inside the shell

- **Status:** proposed
- **Date:** 2026-11-03
- **Depends on:** ADR-0003, ADR-0004, ADR-0006, ADR-0101, ADR-0103, ADR-0150, ADR-0151
- **Evidence:** DM-020, DM-077, GR-002, X-4, alternatives-postmortem.md#plan-9--inferno, shell/survey.md#plan-9-rc, shell/survey.md#nushell, shell/survey.md#powershell, shell/survey.md#oils-and-ysh

## Context

Stage 6a brings byte streams: a read end and a write end, non-blocking transfers, atomic writes of at most 4096
bytes, `PEER_CLOSED` on each end (ADR-0101). POSIX programs see them as pipes (ADR-0103). `zosh` has no pipelines
today, and programs it starts write to the console (ADR-0096: "a program has no byte stream to inherit until
stage 6"). This ADR decides what flows between commands in `a | b`, how native commands and POSIX programs meet in one
pipeline, and what the shell's own data is.

**What is known.** Byte pipes held up (GR-002); untyped streams are *contested* (DM-020): object pipelines bind every
tool to one runtime, and once rendered to text the objects are gone. PowerShell had to add byte preservation for
native programs in 7.4 after years of corrupting binary output (`shell/survey.md#powershell`). Nushell turns external
output into text or binary and gets structure from builtins (`shell/survey.md#nushell`). Plan 9 composed through
files and namespaces, not typed pipes (`alternatives-postmortem.md#plan-9--inferno`). On Zoraya the programs in
scope are native Rust programs and C programs through `libposix`; both speak bytes.

## Decision drivers

1. tcc, make, dash and toybox pipe bytes (ADR-0005); whatever native commands do must interoperate with them in one
   pipeline without converters.
2. One composition rule for every program, in any language (GR-002, DM-020's costs).
3. The shell itself needs lists (ADR-0150) and must turn program output into them simply.
4. A closed reader is not an error for the writer (DM-077, ADR-0004 rule 7: `PeerClosed` is a return value).

## Considered options

### Option A: Byte streams only (POSIX baseline)
- **Description:** `a | b` connects `a`'s output stream to `b`'s input stream; errors go to the terminal; the shell
  turns output into words with `$(...)`.
- **Evidence for:** Everything in scope already speaks it (driver 1); ADR-0101 is built for it; GR-002.
- **Evidence against:** Structure is re-parsed by every consumer (DM-020); names with spaces in line-based output
  need care.
- **Cost:** *(estimate)* Stream creation and handle lists in the spawn path, a few hundred lines.
- **Precedent:** Unix, Plan 9 rc (`shell/survey.md#plan-9-rc`).

### Option B: Structured values between native commands, bytes to and from POSIX programs (Nushell, PowerShell)
- **Description:** Native commands exchange typed records over channels (ADR-0050); the shell renders the last one as
  a table; a POSIX program in the pipeline gets the text form and its output is text.
- **Evidence for:** No re-parsing between native commands; sorting and filtering by field
  (`shell/survey.md#powershell`, `shell/survey.md#nushell`).
- **Evidence against:** Every native command must speak the record protocol and a text form; the boundary needs
  conversion rules, which PowerShell got wrong for years (`shell/survey.md#powershell`); Nushell reimplements tools as
  builtins (DM-020). A typed protocol is also a schema to version.
- **Cost:** *(estimate)* A record wire format, a renderer, converters, and every native command written twice
  (record and text); thousands of lines.
- **Precedent:** PowerShell, Nushell.

### Option C: Bytes between programs, with an optional line-record convention
- **Description:** As A, and native listing commands may offer `--records`: one tab-separated record per line with a
  header line, which a few builtins (`sort --field`, `pick`) understand.
- **Evidence for:** Some structure without a second protocol; still bytes.
- **Evidence against:** A convention that only some commands follow is another thing to learn; tabs are allowed in
  no Zoraya name (ADR-0013 forbids control characters), so it is safe, but nothing in scope needs it yet.
- **Cost:** *(estimate)* Small per command; the builtins are extra.
- **Precedent:** `ps -o`, `git --porcelain` style formats.

## Recommendation

Recommended: **Option A**, with C kept as a later addition. It is a recommendation, not a decision.

1. **Streams of a command.** Every command has three: `in`, `out` and `err`, at fixed slots defined in
   `zoraya-abi` (ADR-0153). By default all three are the terminal (ADR-0154): the shell relays bytes between them and
   the terminal, so programs never hold the console or the terminal channel. The foreground job's output is relayed
   as it is; background output is handled by ADR-0154 rule 7.
2. **Pipelines.** `a | b | c` creates one stream (ADR-0101) per `|`, gives each element the write end as `out` and
   the next element the read end as `in`, and starts all elements as one job (ADR-0158). `err` of every element goes
   to the terminal. `&|` sends `err` into the pipe as well.
3. **Redirection.** `> FILE` (create or replace), `>> FILE` (append), `< FILE`, `2> FILE`, `2>> FILE`, `&> FILE` (out
   and err). The shell opens the file itself, before the command starts, and passes the connection or a stream fed
   from it: the program receives that file and not its directory (ADR-0153).
4. **The shell's data.** Lists of strings (ADR-0150). `$(...)` gives one element per line. No values cross a
   pipe but bytes.
5. **Builtins and functions in pipelines.** A builtin or function may be the first element of a pipeline, where the
   shell writes its output into the stream from its event loop, never blocking on it (ADR-0006: `WRITABLE`). A
   builtin or function elsewhere in a pipeline is a parse error (`zosh: NAME: a builtin or function can only start a
   pipeline`), until a later ADR runs functions as separate processes.
6. **Status.** A pipeline succeeds when every element succeeds (ADR-0151 rule 3); otherwise its `$status` is that of
   the rightmost element that failed. A question command's "no" (ADR-0151 rule 3) is not a failure in any position;
   a pipeline whose elements all succeeded but whose last element answered "no" answers "no" (`cat log | search
   ERROR` with no match: status 1, nothing stops). A writer that ends because its reader closed is not a failure: a native program
   that gets `PeerClosed` on `out` stops and exits 0 (ADR-0155), and a POSIX program ended by `SIGPIPE`
   (`Terminated`, `detail` 13, ADR-0101) counts as success when it is not the last element (YSH's
   `sigpipe_status_ok`, `shell/survey.md#oils-and-ysh`).
7. **POSIX programs.** They see the same streams as fds 0, 1 and 2 (ADR-0103). Nothing is converted.

**Examples.**

```
~> ls
a.txt
b.txt
notes/
~> ls | search .txt
a.txt
b.txt
~> search TODO src | head --lines 2
src/main.c:12: TODO check the result
src/main.c:40: TODO free this
~> set lines $(cat names.txt)
~> count $lines
3
~> cat big.log | head --lines 1
first line of the log
~> echo hello | cat
hello
~> cat notes.txt | jobs
zosh: jobs: a builtin or function can only start a pipeline
  hint: run jobs on its own line
```

### Effect of the vision

For target 2 bytes are the narrowest interface: an agent's program receives two stream ends and no knowledge of what
is at the other end; a structured protocol would be one more decoder to harden. For target 3 (desktop) nothing here
precludes a later record convention.

**Tensions with accepted ADRs.** *ADR-0096* says programs started from `zosh` print to the serial console; under
rule 1 they print to the terminal through the shell once streams exist. ADR-0096's sentence was a stage 5 limit, and
its own text expects this change.

## Consequences
- **Positive.** Native and POSIX programs compose with no converters; one mechanism; `PeerClosed` handled once.
- **Negative.** No typed data between commands; listing commands must keep their output one item per line
  (ADR-0155). The shell relays all terminal output of its children, which costs a copy per byte.
- **Follow-up.** Stream ends in the spawn path; the relay loop; `&|`; the line-record convention when a measured need
  appears (X-4 is not involved: streams are readiness objects as ADR-0006 rule 1 says).

## Invariants
- **INV-0152-1.** Each element of a pipeline holds exactly the stream ends the pipeline gives it, and the shell keeps
  none after starting the job, so the last writer's exit gives the reader end of file. *Check:* an in-QEMU test with
  `echo-args`-style programs reporting their handle types per slot, and a pipeline whose reader ends only on end of
  file.
- **INV-0152-2.** The shell never blocks on a stream: a full stream to a slow reader does not stop the line editor or
  other jobs. *Check:* an in-QEMU test with a builtin producing more than a stream's capacity into a reader that
  sleeps, while keys typed at the prompt are echoed.
- **INV-0152-3.** A pipeline's status is success exactly when every element succeeded, answered "no" before the last
  element, or ended only because its reader closed; it is "no" when the last element answered "no" and none failed. *Check:* host tests of the status rule over every combination of three elements' records.

## Open questions
- The cost of relaying every byte of output through the shell, against giving programs a terminal connection of
  their own (ADR-0105's tty connection): measure on a large `cat` once streams exist.
- Whether `err` should be coloured or prefixed when it reaches the terminal, to tell it from `out`.

## Revisit triggers
- Repeated parsing of one command's output by scripts (for example `ls --long` piped into field extraction): add the
  line-record convention of Option C for that command family.
- The relay measured as a bottleneck: hand programs the terminal's output stream directly.

## Architect's decision
