# TASK-20 review: stage 6c, the `zoraya-posix` core

One independent review by a subagent that wrote none of the code (on the main model, as `CLAUDE.md` asks), read-only
and with no builds, of `01f82c5..781406f`; then mutation testing of the new Rust code, sorted by the `mutant-triage`
subagent; then one check of the changed lines. Fixes are in `1976faf` (the review) and the commits named below.

During the mutation run the host froze (about 60 GB): `docs/backlog.md` has the command, the cause (an integration
test binary of `zoraya-posix` without the heap cap) and the fix; `9ce0ae9` bounds the test runner's output and
`fcf2c06` caps the test and adds `make check-test-caps`.

## Findings and dispositions: the review

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| R1 | error | A last component of `.` or `..` vanished in normalisation, so `rmdir("d/.")`, `rename("d/.", ...)` and `mkdir("new/.")` acted on the parent's name. | Fixed: the resolver marks it, the naming calls refuse it (`EINVAL`, or `EEXIST` for `mkdir` and `symlink`); host test `dot_as_the_last_name_is_refused_by_the_naming_calls`. |
| R2 | error | `fchmod` through a read-only fd reopened its old place with `WRITE`: after a rename it changed another object; a silent rights choice. | Fixed: it acts only if the place still holds the fd's object (same volume and id), else `EACCES`; stated in ADR-0180 item 14; host test `fchmod_by_place_acts_only_on_the_fds_object`. |
| R3 | error | `st_blocks` from the size deviates from ADR-0014 rule 9 and was cited as stated in ADR-0180, which it was not. | Stated in ADR-0180 item 14. |
| R4 | warning | The `SELF` handle of the second bootstrap message was forgotten; `SELF`'s slot was documented as filled. | Kept in `Posix::self_handle` for 6d; the slot is documented as a `fork` child's; ADR-0180 item 5 says so. |
| R5 | warning | The heap's growth leaked the memory object when its map failed. | Fixed. |
| R6 | warning | `writev` sent one request per piece, so a stdio line to an `O_APPEND` file was two appends. | Fixed: up to 60 KiB in all is gathered into one write; stated in ADR-0180 item 14. |
| R7 | warning | The outcomes of `plan.md`'s two risks were not recorded. | Recorded below. |
| R8 | warning | The start-up vector on the heap and `AT_RANDOM`'s seed differ from ADR-0109 item 4, unstated. | Stated in ADR-0180 item 14. |
| R9 | warning | Times before 1970 cannot be set (`set_attributes` takes unsigned seconds), unstated. | Stated in ADR-0180 item 14, with a revisit trigger; design.md's limitations say so. |
| R10 | note | Past 16 reports, a path was not remembered, so it was counted again. | Fixed. |
| R11 | note | `readv` on a stream blocks for the second piece. | Left: only musl's `__stdio_read` reaches it, and `fread` waits anyway; `readv` is not exported. |
| R12 | note | A comment named `EEXIST` where the code gives `EPERM`. | Fixed. |
| R13 | note | A relative path joined to cwd could pass what a request carries and fail with `EINVAL`. | Fixed: `ENAMETOOLONG`, host-tested. |
| R14 | note | `open_with_mode` answered `OK` with mode 0 when its `stat` failed. | Fixed: `STORAGE_FAILED`, the connection's end closed. |
| R15 | note | `Posix::share` is unused, so `shares=` is always 0, and it asks fixed rights. | Left for 6d, its first user; the round trip is measured by `posixrun -s`. |
| R16 | note | `O_RDONLY\|O_CREAT` of a directory gives a directory fd; `O_PATH` is a read open. | Left: no measured program does either. |
| R17 | note | A stream write cut by a closed reader lost its count. | Fixed: it returns what it wrote. |
| R18 | note | `fstatat(AT_FDCWD, "", AT_EMPTY_PATH)` is `EBADF`; unknown flags of `stat_at` are not refused. | Left: no measured program does either. |
| R19 | note | The broker count is 0 by construction until 6d. | INV-0002-3's and INV-0103-3's statuses say so. |
| R20 | note | The crate root has the test build's cap, which uses `unsafe`. | INV-0100-3's status says so. |
| R21 | note | `mkdir` is hidden and `rmdir` absent, as the tier list has neither. | Recorded here: the C program uses `mkdirat` and `unlinkat`, which the list has. |

## The two risks of `plan.md`

- **The shim header's completeness.** Every call the retained musl files make is one of ADR-0141's eleven, and each
  now has a real target in `ffi.rs` (`zoraya_posix_read`, `readv`, `writev`, `close`, `fcntl`, `ioctl`, `mmap`,
  `set_tid_address`, `exit_group` for `exit` and `exit_group`); `futex` is answered in the header and is unreachable
  with one thread. Any other call fails to compile (the shim), the excluded files are exactly those the shim refuses
  (`check-excluded`, INV-0141-2), and the archive has `svc` only in `zoraya-rt`'s member (`make test`'s
  `check-libc`). The C program exercises `readv`, `writev`, `close`, `fcntl`, `ioctl`, `set_tid_address` and
  `exit_group` through stdio and start-up; `mmap` only when a program's TLS outgrows musl's built-in block, which none
  here does.
- **`share` against the single-threaded loop.** Requests of one description take effect one at a time and the
  position moves inside each: `interleaved_requests_at_the_position_lose_no_update` (40 seeds, three connections of
  one description and one of another, against a model, closes in every order), `shared_connections_see_one_position`,
  and `posixrun -o` in QEMU, where the child's writes through a shared description move the position the launcher then
  reads (35 after five lines). No lost update.

## Mutation testing

cargo-mutants 27.1.0 through `make mutants-host`, two jobs, 300 s a mutant, under the watchdog, every test process
capped at 512 MiB (after `fcf2c06`). `ffi.rs` is left out (`--exclude`): the host does not compile it, and its
behaviour is `make test-libposix`'s. The other crates were run over the lines this task changed (`--in-diff`).

| Crate | First run | Holes, now tested | Last run | Left |
|---|---|---|---|---|
| `zoraya-posix` (all but `ffi.rs`) | 717: 161 missed, 499 caught, 56 unviable, 1 timeout | the `mutant-triage` subagent's list: fd flags, `dup3`, `F_SETFL`, open's flags on the wire, an entry's root, `O_TMPFILE`, reports of a create, stream waits (the fake kernel gained a reader that acts during a wait), request counts at `MAX_INLINE`, `ENOSPC` after a partial write, special entries, `stat_at`'s empty path, links in `access` and `utimensat`, the root and dot names, temporary parents closed, `share`, the counters, `from_bootstrap`, replies that are not OK (crafted), `O_DSYNC`, and unit tests of `fd::same`, the signal bits, `User::part` and a tie of prefixes (`4c3e4c6`, `e11e5f6`) | 717: 46 missed, 614 caught, 56 unviable, 1 timeout, before `e11e5f6`'s two | 44, equivalent: `\|` to `^` (or `&`) on disjoint bits, rights of a connection on which nothing is ever sent or checked (cwd's, an `O_RDONLY` connection's `SYNC`), `EXCLUSIVE` without `CREATE`, a link kind that only `NO_FOLLOW` returns, the request size of a report, a peer-gone wait that the next read reports the same way, `User::part` clamping a length that is the remainder, `write_stream`'s arms after a wait whose retry gives the same result, and `isize::MAX` exactly, which no memory holds |
| `zoraya-fs-service` (changed lines) | 87: 9 missed, 75 caught, 3 unviable | the durable flag of a write at the position; `connection_of`, unused, removed | not re-run | 2, equivalent: a write ending past `i64::MAX` is refused by the engine as well |
| `zoraya-abi` (changed lines) | 162: 17 missed, 135 caught, 10 unviable | every kind's round trip, `has_slot`, the bounds `MAX_PATH`, `OPEN_MAX` fds, `umask` 0o777, prefixes of one length, a report's read access | 162: 1 missed, 151 caught, 10 unviable | 1, equivalent: a cwd entry past the end is refused later, as no entry is its directory |

The triage found two places where the code, not the tests, was in question: names under a special entry answered four
different errnos (now `ENOTDIR`, as POSIX has it), and a tie between equal prefixes, which a decoded record cannot
have (the first wins, now tested).

## Check of the changed lines

One check, by a fourth subagent that wrote none of it and ran nothing, of `git diff 781406f..d3a30ea` (the review's
fixes, the test runner's bounds, the heap caps, the mutation holes' tests, docs): one error and eight warnings and
notes, all fixed in the commit after `d3a30ea`.

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| C1 | error | The line-mode protocol checker kept a copy of every bad line as a problem, so neither its memory nor the terminal was held to the runner's bounds. | Fixed: at most 100 problems are kept and the rest counted, at most 10,000 test names; a unit test feeds 100,000 bad lines. |
| C2 | warning | The runner's unit tests wrote `tools/test-runner/target/test-logs/`, and that file was committed. | Removed; the unit tests log to the temporary directory; `.gitignore` ignores `target/` at any depth. |
| C3 | warning | CLAUDE.md's cap rule spoke of every host-tested crate; the check covers `crates/` only. | CLAUDE.md says `crates/`, and that `tools/` is the backlog's. |
| C4 | warning | Gathering `writev` copied stream pieces in user space, so a bad pointer faulted instead of `EFAULT`. | `writev` gathers only for files; ADR-0180 item 14 says so. |
| C5 | note | Past 16 reports, the remembered paths grew without bound. | At most 256 are remembered; ADR-0180 item 14. |
| C6 | note | With a record that does not decode, the `SELF` handle stayed open. | Closed. |
| C7 | note | `gui.rs`'s last drain failed a run on the deadline the steps had met. | The last drain fails only on a flood or forbidden text. |
| C8 | note | CLAUDE.md said every run keeps a log; the crash test keeps none. | Said. |
| C9 | note | `read_line` took `Interrupted` as the end of the output. | It retries. |

The task allows one more check if this one found errors; it found one, so the lines changed by its fixes were checked
once more (below).
