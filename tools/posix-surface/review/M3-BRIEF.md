# M3 call-site review brief

You classify POSIX call sites for TASK-02 (`docs/research/TASK-02-posix-surface-measurement.md`, section M3 —
read it). Context: we are designing a new OS whose native ABI avoids `fork`, asynchronous signals, ambient
global paths and process-global state (see `docs/research/candidate-principles.md`, conflicts X-1, X-2, X-3).
POSIX will be provided by a sandboxed compatibility layer. The question per site: **how hard is it to avoid
the problematic feature here?**

## Input

`tools/posix-surface/results/callsites.tsv` — one row per site, found by pattern match and restricted to code
compiled into the measured binary. Columns: program, kind, tier, feature, api, file, line, function, code,
rule_class, rule_effort_lines, rule_reason, signals. The `rule_*` columns are a mechanical first guess. Treat
them as wrong until you check.

Source code is in the measurement VM. Read the context around each site there:

    ssh -F ~/.lima/alpine-posix/ssh.config lima-alpine-posix 'sed -n "<from>,<to>p" ~/psurf/build/<srcdir>/<file>'

with `<srcdir>`: dash→`dash`, oksh→`oksh`, mrsh→`mrsh`, toybox→`toybox-T2`, busybox→`busybox-T2`,
tcc→`tcc`, gmake→`gmake`, pdpmake→`pdpmake`. Batch many `sed`/`grep` calls into one ssh command. Never modify
anything in the VM.

## Classes (from the task)

- `direct`: a small local change replaces the feature with a non-problematic primitive. Give estimated lines
  changed. Examples: fork+exec of a fixed command → posix_spawn (or a spawn-from-handles primitive); `waitpid` of
  your own child → wait on a process handle; `signal(SIGPIPE, SIG_IGN)` → delete it (the native write returns an
  error); `"/dev/null"` → a null handle; `isatty(fd)` → query the handle type; `mkstemp` in `/tmp` → a temp-dir
  capability.
- `restructure`: needs a change in program structure (e.g. a SIGCHLD handler driving a job loop → an event loop
  over child handles; `alarm()` timeouts → deadlines in the wait call; code that relies on process-global cwd
  spread over many functions). Give an effort estimate in lines, a rough order of magnitude is enough.
- `emulate`: realistically the compat layer must provide the POSIX semantics, because the program's purpose
  *is* those semantics or rewriting is out of proportion. Examples: a shell's subshell `fork`, job control with
  process groups, the `cd` builtin, `install`/`chmod` setting modes, `$PATH` search in a shell.

Rules: classify the *site in its context*, not the API name. If one site does several things, choose the hardest
class and say so. If the match is a false positive (a comment, a string, a different function with the same
name, a macro definition), mark class `n/a` and say why. Every value is an estimate. Be conservative: when
unsure between two classes, pick the harder one and note the doubt.

## Output

Write `tools/posix-surface/review/m3-<NAME>.tsv` (NAME given in your prompt), tab-separated, with this header
and one row for **every** input row of your programs (same order):

    program  file  line  api  class  effort_lines  what  note

- `what`: one short sentence saying what the code does there (e.g. "forks a subshell for `( ... )`").
- `effort_lines`: integer, 0 for `emulate`/`n/a`.
- `note`: optional. Use it for doubts, coupling to other sites ("same change as jobs.c:1002"), or DM IDs.

Also write `tools/posix-surface/review/m3-<NAME>-notes.md`: per program, ≤15 lines on patterns you saw. Which
sites drive the hard classes? Would one compat-layer feature cover many sites? Then name the 2–3 best candidates
for a validation patch (M4): a `direct` site whose change is small and testable by rerunning a workload.

Do not edit any other file. Do not run git. Reply with a short summary (≤120 words): counts per class per
program, and your M4 candidates.
