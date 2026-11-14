# ADR-0344: What part 10e decided where ADR-0336 and ADR-0342 are silent

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0153, ADR-0156, ADR-0212, ADR-0311, ADR-0336, ADR-0342, ADR-0343
- **Evidence:** CP-27

## Context

TASK-31 part 10e builds the userland of ADR-0336. Where that ADR and ADR-0342 are silent the task says: choose the
simplest, least surprising, reversible and safest option, write it as a proposed ADR, and continue with the least
committing option (CP-27: nothing is added without a program behind it). This ADR lists those choices. It decides
nothing the accepted ADRs decide.

## Decision drivers

1. A program gets what the line says and nothing more (ADR-0153).
2. Each description is a small security statement: keep its options minimal and say what it lets through.
3. Reversible: each choice below can be replaced by a later ADR without changing an accepted one.

## Considered options

### Option A: the least committing choice for each gap (the recommendation)
- **Description:** items 1 to 8 below.
- **Evidence for:** drivers 1 to 3.
- **Evidence against:** several commands are narrower than their GNU forms (item 4).
- **Cost:** *(estimate)* none beyond what is built.
- **Precedent:** ADR-0311's narrowed entry for a `create` name.

### Option B: build the missing mechanisms now (a names list in the file protocol, `-e` and `-f` for `sed` through a new description key, conditional option kinds)
- **Evidence for:** fuller commands.
- **Evidence against:** each is a new protocol or description rule with its own security argument, which this part is not placed to review.
- **Cost:** *(estimate)* a protocol message, the service's connection record, the client, the shell's table, host tests; days.
- **Precedent:** ADR-0337 item 8 plans the names list for the grant line.

### Option C: Linux's behaviour (toybox as built for Linux, no descriptions)
- **Evidence against:** ADR-0336 driver 3.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **Layout.** `/usr/libexec/toybox` holds the multi-call binary; `/bin/NAME` is a relative link to it
   (`../usr/libexec/toybox`) for each line of `ports/toybox-applets.txt`; the build enables the one build option
   `toybox-float` (`seq` needs it); `mv`, `gunzip`, `zcat` and `realpath` are options of other commands' sources.
2. **A file operand of a `cwd none` description** is planned as `ReadName` or `ChangeName`: its directory narrowed to
   the one name, with `READ | CLONE`, or `READ | WRITE | SYNC | CLONE` (no `CREATE`). The existing narrowed connection
   of ADR-0055 carries one name, so two or more operands in one directory fall back to the directory given whole
   with the union of the rights, in the words `read anything in` and `read and change files in`, so `grants` says
   what is given (the existing rule of ADR-0153 for `create`). The names list of ADR-0336 item 6 is **not built** in
   10e; `sed` with two files of one directory therefore sees that directory (not its subdirectories) until it is.
3. **`sed -i`** (and every user of toybox's `copy_tempfile`) writes the result to a file in `/tmp`, the line's own, and
   copies it back into the original, opened for writing and truncated; the original is never replaced by rename.
   It needs `WRITE` on the one name and no more. The change is not atomic.
4. **Descriptions** (56 files in `user/describe/posix/`): `cwd none` only for `sed`; each lists only the options of
   the daily use, with the short option as typed and an invented long name beside it (the format requires one);
   `sed` lists neither `-e` nor `-f` (the script is the first operand); `find` takes one `PATH` and a text
   expression; `tar -f` is `create` in every mode; `dd` designates nothing (`if=` and `of=` need `+r` and `+w`);
   `patch` needs `+w` on the tree; a word starting with `-` after the operands is passed as text. Each file's last line
   is `Lets through: ...`; the list for the architect is printed by `cargo test -p zoraya-describe --test posix_names
   -- --nocapture`.
5. **The lint** of INV-0336-2 is a test of `zoraya-describe` that `make check` runs as part of `check-argv`; it knows
   the names of `/bin` from the applet list, the ports' own programs and, if present, `build/rootfs-posix/bin`.
6. **Absent calls.** `fchdir`, `statfs` and `pathconf` are not exported; patch 0002 of toybox makes them fail with
   `ENOSYS` (so `find -execdir`, `stat -f`) (ADR-0343).
7. **awk** is one-true-awk at commit `72e9b56caddb4a6f8d1405315a5183c26b8650a4` (second edition, `version
   20261106`), grammar by the host's bison, no patch; its names are read by the same probe as toybox's once its
   block exists (open question).
8. **The image cache of ADR-0212 is not built in 10e** (see the part's report); the userland does not depend on it.

## Consequences

- **Positive.** The tools of ADR-0336 are in the image with descriptions and a check; no protocol changes.
- **Negative.** `sed` with several files in one directory sees the directory; `tar -x` is granted a name to create;
  several commands lack a GNU option people use.
- **Follow-up.** The names list; a rule for options that begin a word (`find` expressions); the image cache.

## Invariants

- **INV-0344-1.** A `cwd none` description's file operand is planned as one narrowed name, never as the current
  directory. *Check:* the host tests of `zoraya-shell`'s `grants` (`a_file_of_a_cwd_none_program_is_given_as_one_narrowed_name`).

## Open questions

- Whether one-true-awk's imported names need a block of their own (the probe of `awk` has not run on the image).

## Revisit triggers

- The names list is built (ADR-0337 item 8's part).

## Architect's decision
