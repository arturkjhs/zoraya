# TASK-18 review: the native shell, Part 2

Reviews of Part 2, one step at a time, as the work goes on (the task's review of the whole of Part 2 follows its last
step). Each is by the main model, as `CLAUDE.md` asks, in a session that wrote none of the code it reviews; then
mutation testing of the step's host code, sorted by the `mutant-triage` subagent and rechecked by the main model.

## Step 8: byte streams and pipelines (ADR-0152), commit `01af1c2`

Read: the whole diff of `01af1c2` (`zoraya-shell`'s evaluator and its tests, `zoraya-commands`' input, `zoraya-rt`'s
streams, `zosh`'s relays, feeds and jobs, the scripts), against ADR-0152, ADR-0155 rule 6, ADR-0154 rule 7 and
ADR-0158 rule 2. Found on the way: R5, by step 9's `tests/shell/jobs.script`, which runs two writers on one `err`.

### Findings and dispositions

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| R1 | medium | A line stopped by a failed pipeline was reported under the last element's name, not the failing one's: `crash \| cat` printed `cat: crashed (...)`. `several` left `last_name` at the last program. | Fixed: `pipeline_failure` gives the element whose record is the status (ADR-0152 rule 6), and the report names it; `a_pipelines_report_names_the_element_that_failed` (fails without the fix). |
| R2 | medium | A background job's unfinished line was held whole: a job writing without a newline grew the shell's memory without bound. | Fixed: a held line is shown in pieces of at most 4 KiB (`MAX_HELD_LINE`), cut on a UTF-8 boundary. |
| R3 | low | A job brought to the front with a held partial line printed its new output before that line. | Fixed: the held part is shown first. |
| R4 | low | `cat`, `head` and `search` reading their input broke a UTF-8 sequence cut between two reads when an invalid byte came before it in the same read (`from_utf8` reports only the first error). | Fixed: `incomplete_tail` judges the last bytes alone; the input test covers it at four chunk sizes (fails without the fix). |
| R5 | low | `print!` and `eprint!` wrote each formatting piece separately, so lines of two programs sharing a stream were cut into each other (`cd: interrupt 1` / `: interrupt 1`). | Fixed: a print gathers its pieces (`zoraya_rt::Gather`, 512 bytes) and writes once, which a stream keeps whole (ADR-0101). Checked by `jobs.script`. |
| R6 | low | The keys given to a job skipped `ESC [ ... final` but ended `ESC O x` (F1 to F4) at `O`, so the `x` was typed. | Fixed: three states, `ESC [` until a final byte and `ESC O` for one more. |
| R7 | low | A misplaced doc comment (`is_success_no`'s on `Prepared`) and an unused `checked` parameter of `several`. | Fixed. |
| R8 | low | A write to a redirected file that fails (`> FILE` on a full volume) is dropped without a message. | Stated in `docs/design.md`'s limitations; a message needs the relay to keep the path. |
| R9 | question | `cat` with no file replaces bytes that are not UTF-8 by U+FFFD and adds no final newline; `cat FILE` prints `N bytes of binary` and adds one. Neither is wrong by ADR-0155, but they differ. | Left for the architect: which of the two should both follow. |

No problem was found in the stream ends given and kept (INV-0152-1: every end made for the children is closed after
the spawns, on the failure paths too), in the non-blocking feeds (INV-0152-2: every write is `WouldBlock`-armed), in
`pipeline_status` (INV-0152-3), in `PeerClosed` ending a writer with 0 (INV-0155-4), or in the subscriptions: ADR-0006
rule 2 removes a subscription when its handle closes, so the relays' and feeds' closes need no `port_unsubscribe`.

### Mutation testing

`make mutants-host` (cargo-mutants 27.1.0, under the watchdog) on `zoraya-shell` and `zoraya-commands`, limited with
`--in-diff` to the lines step 8 and the fixes above changed, on the tree of the fixes (`1a41864`). The
`mutant-triage` subagent sorted the survivors; the main model rechecked each verdict, and the re-run is the judge.

| Crate | First run: caught, missed, timeouts | Holes / equivalent | After the new tests |
|---|---|---|---|
| `zoraya-shell` (`eval.rs`) | 35, 0, 0 (of 47, 12 unviable) | 0 / 0 | not needed |
| `zoraya-commands` | 32, 10, 2 (of 44) | 8 / 2 | 40 caught, 2 missed, both equivalent; the 2 timeouts are mutants detected by tests that then never end |

The two equivalent mutants replace `Files::read_input` with `Ok(0)` and `Files::print` with `false`, which are the
default bodies themselves. The holes: the defaults were never run (only `Mem`, which overrides both, implemented
`Files` in the tests, while `zosh`'s own `Files` uses them), a four-byte UTF-8 lead was never cut, `flush`'s threshold
and its `&&` were not pinned, and `head`'s choice between its input and its files was not either.
`a_caller_with_the_defaults_has_no_input_and_gets_all_output`, `input_gives_each_read_and_then_its_end`,
`output_is_offered_only_past_print_at` and `head_reads_its_input_only_when_it_needs_it` kill them.
