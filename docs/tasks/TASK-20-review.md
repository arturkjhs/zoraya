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

(filled in below)

## Check of the changed lines

(filled in below)
