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

## The whole of Part 2 (steps 1 to 12), at `453a796`

Read, in two halves by the main model in sessions that wrote none of the code: (S) the host crates and the shell
program (`zoraya-shell`, `zoraya-describe`, `zoraya-command`, `zoraya-commands`, `zoraya-settings`,
`zoraya-resolve`, `tools/tour`, `zosh`, `user/src/{namespace,files,filecmd,command}.rs`, the POSIX launch), against
ADR-0150 to ADR-0159, ADR-0200 and `docs/adr/invariants.md`; (K) the kernel, ABI and runtime changes, the runner,
the scripts, the Makefile and the CI workflow. Host tests were run in both; nothing in QEMU. Every finding below was
then fixed or stated, and the fixes were checked again by reading their diff and by `make test`.

### Findings and dispositions

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| S1 | high | An option argument attached to its option (`-o$x`, `--output=$x`) was taken as literal, so rule 4 did not apply to it: `grants::value_of` matched a whole word by identity, and "not found" meant literal. | Fixed: a designated text is a value's if it lies inside one, and fails closed when no word contains it. `values_inside_a_word_are_values`. |
| S2 | medium | A connection narrowed to one name widened through it: opening that name as a directory gave a connection with the requested rights and no narrowing, so `create`, `remove` or `rename NAME` reached the whole tree below NAME (INV-0153-2). | Fixed in the file service: a narrowed connection opens its name as a directory to read and list only (`ACCESS_DENIED` for more). `a_narrowed_name_that_is_a_directory_opens_to_read_only`; `docs/design.md` says so. |
| S3 | medium | An entry inside a broader one hid the broader rights: `+w ~ prog` in `~/project` left `~/project` read only, since a path is served by its longest entry. | Fixed: `merge` gives every entry the rights of each entry that covers it (`covered`). `entries_inside_others_take_their_rights`. |
| S4 | medium | `rm --recursive` followed link objects and removed what they pointed at. | Fixed: `remove_tree` learns each entry's kind from its parent's listing and never follows a link; a link is removed as itself. `removing_a_tree_never_follows_a_link`. |
| S5 | medium | A redirection target from a value (`echo x > $f`) was opened with the shell's own access, outside rule 4. | Fixed: a redirection target from a value must lie inside the current directory or a grant of the element. `a_redirection_from_a_value_stays_inside`. |
| S6 | medium | `grants LINE` did not show what the line would get (INV-0153-4): every word was literal, markers from values counted as grants, and a native volume program got the POSIX report. | Fixed: the builtin evaluates the line as a run does (marks kept, markers literal only) and asks the host for the program's kind. `grants_sees_the_line_as_a_run_would`, `a_posix_programs_report_shows_its_toolchain_set`. |
| S7 | medium | INV-0153-3's named cases (`+r` and `+w /` as values, empty and two-element lists, link objects from variables) had no tests, nor had the arity refusal. | Fixed: `a_grant_is_written_and_is_one_path` and the rule-5 cases of `grants.script`. |
| S8 | low | A glob after `+r`/`+w` with one match was accepted (rule 3 refuses a pattern); the arity check repeated one clause. | Fixed: a grant's word with an active pattern is refused ("a grant names one path: a pattern is not one"); same test. |
| S9 | medium | A line with more than about 62 designated paths could not run (64 entries), undocumented. | Fixed with S3: a path covered with the same rights by another entry gets no entry of its own. The limit left (64 distinct entries) is in `docs/design.md`'s limitations. |
| S10 | low | Rule 4's hint always offered `+w` on the parent, also for a read. | Fixed: the hint offers `+r` for a read operand. |
| S11 | low | The refused-path hint put the grant before the whole line, where it does not reach a later element. | Fixed: in a pipeline the hint says "put +r DIR before NAME in the line". |
| S12 | low | A line that failed after a POSIX element started left its pid unreaped in the broker and its `/tmp` directory. | Fixed: the refusal paths reap the killed POSIX elements and remove the line's `/tmp`. |
| S13 | low | Patterns with `.` or `..` components never matched. | Fixed: they are steps, not names to list. `dot_and_dot_dot_are_steps`. |
| S14 | low | Repeated `**` did combinatorial work. | Fixed: `**/**` is one `**`, and a pattern makes at most 10 000 listings. `repeated_any_is_one_and_listings_are_bounded`. |
| S15 | low | The tour's step 15 promised a narrowed output that its own example does not get. | Fixed: step 15 says that both names are in the current directory, given whole, and points to step 17. |
| S16 | low | A value naming the current directory itself designated its name in the parent. | Fixed: kinds that narrow the parent want a path strictly below the current directory or a grant. In `values_inside_a_word_are_values`. |
| S17 | low | History entries of several lines came back as separate lines after a restart. | Fixed: a line end inside an entry is kept in `/config/history` as U+001E and restored on reading. |
| S18 | medium | Found by mutation testing (a survivor in `expand`): a function's parameters and `$args` lost the marks of the words the caller wrote, against ADR-0153 rule 4, so `fn view { cat $args }; view ../x` was refused where `cat ../x` runs. It failed closed: `expand` ignored every value's mark. | Fixed: a word that is one variable alone takes its value's mark; and since `set` and `for` stored their words' marks, which rule 4 makes data, they now store data, so only parameters and `$args` carry the caller's. `parameters_keep_the_callers_marks_and_variables_are_data` (fails without the fix). The first half's review had called the parameter marks sound: it read the binding, not `expand`. |
| S19 | low | Found because a mutation run slowed down: the language builtins (`set`, `echo`, `unset`, ...) matched each parsed operand to its argument by searching all the arguments from the start, which is quadratic: `set v $a` with $a of 100 000 values took 31 s in a debug build. | Fixed: operands come in the arguments' order, so one forward walk matches them; the 100 000-value test of `set` now runs in milliseconds. |
| K1 | medium | INV-0159-1's check never used a `Power` handle: the "without `POWER`" case was a console handle. | Fixed: the test kernel starts `t_console` a second time with a `Power` handle without `POWER`; `power_reset` and `power_off` are refused and it returns 0. Status text corrected. |
| K2 | medium | The tour jobs would likely pass CI's 10-minute job limit under TCG (GitHub's runners have only TCG). | Fixed: `tour.script` is one job per accelerator, the GUI tour is a step of its own (`make test-tour-gui`), one job per accelerator, and jobs whose command names the tour may take 30 minutes. The measured times are in the task report. |
| K3 | medium | Fixed sleeps stood in for readiness before `o` checks and before Ctrl-Z to a program that installs a handler. | Fixed: the runner's new step `w TEXT` waits for TEXT since the last match (`wait_leaves_the_last_match`); `jobs.script` and `zosh.script` wait for the `ready` and `quit` lines. The tour's `pause 1000` before Ctrl-C and Ctrl-Z stays: it goes to native programs, whose job exists before zosh reads the key, so there is no race to lose. |
| K4 | low | A POSIX child whose record could not be written, or that the broker did not commit, was left running with its pid prepared. | Fixed: its process job is killed and the pid aborted. |
| K5 | low | A failed `Broker::call` after its write left the reply to be read as the next call's; every request was transaction 1. | Fixed: the port is made and subscribed before the write, requests are numbered, and a reply of another number is dropped. |
| K6 | low | init's broker start leaked its job, hand-off ends and image on failure paths, and a broker it gave up on kept running. | Fixed: one cleanup on every way out (`spawn_broker` closes its image whatever happens). |
| K7 | low | init waited a fixed 5 s for the broker's hand-off. | Fixed: it waits for the hand-off or the broker's `TERMINATED`, with no deadline. |
| K8 | low | `zoraya-rt` judged its stream slots on first use, when a handle the program made could be at one of them. | Fixed: the entry asks before `main`, when the table holds only what the spawn placed; `input_read` uses the same answer. |
| K9 | low | INV-0200-2's "did not stop" half was untested, and the order handler, stop, report was not checked. | Fixed: `t_posix notstp` ignores `SIGTSTP` and `zosh.script` expects `did not stop`; zosh passes on a job's output while it waits for the stop, and the script expects `restoring the terminal` before `[1] stopped`. |
| K10 | low | `probe` did not try `rename`, `token`, `share`, `narrow`, or an unnamed file and `publish`. | Fixed: it tries them, after the others, and `grants.script` expects their words. |
| K11 | low | `Startup::decode` accepted a path repeated with another path of its length between. | Fixed: any repeated path is refused; in `the_rules_are_kept`. |
| K12 | low | A GUI script's `window` after Ctrl-L could match the screen before the clear. | Fixed: the runner's new step `top TEXT` waits until the top row reads TEXT; the generator and `terminal.script` use it after Ctrl-L. |
| K13 | low | INV-0155-1's Status still said "not yet applicable". | Already fixed by step 11 before the review ended: `checked: make test-files (tests/shell/conventions.script)`. |

Checked and found sound, among others: `zoraya-resolve`'s path rules and link following, the service's narrowing of
every request but S2's, the value marks through globs and function calls, the bounds of `math`, the settings reader,
the editor, history and captures, PSCI `SYSTEM_RESET` and its rights check, `Startup::decode`'s totality and handle
bitmap, `zoraya_elf::section`, `Gather`, the reaping of POSIX pids when a line's job ends, and init's ordering of the
shell's and the terminal's ends.
