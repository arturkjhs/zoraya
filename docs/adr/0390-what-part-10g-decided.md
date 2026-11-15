# ADR-0390: What part 10g decided where ADR-0340 and ADR-0341 are silent

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0107, ADR-0153, ADR-0156, ADR-0172, ADR-0224, ADR-0265, ADR-0340, ADR-0341, ADR-0342
- **Evidence:** CP-01, CP-27, stage-10/survey.md#what-the-ssh-client-needs

## Context

Part 10g (TASK-31) builds ADR-0341's own directory and ADR-0340's ssh. Both accepted ADRs leave small shapes open
(the ABI mark, the names list, the port's configuration, the test server). This ADR records what was built, each the
simplest, safest and most reversible choice, so that the architect can accept or replace it.

## Decision drivers

1. Do not change an accepted decision; add shapes only where it is silent (CLAUDE.md). A program's secrets are its own (CP-01).
2. Keep `libc.a`'s surface unchanged where a port can bring its own stubs (INV-0005-1, CP-27). The client's needs were measured by the link probe (`stage-10/survey.md#what-the-ssh-client-needs`).
3. Everything reversible by deleting a file or a line.

## Considered options

### Option A: what 10g built (below)
- **Evidence for:** drivers 1 to 3; each item is tested on the host or in `make test-apps`.
- **Evidence against:** more shapes to keep true.
- **Cost:** *(estimate)* as built.

### Option B: change the accepted ADRs' shapes (a new `EntryKind` is an ABI change; an `exec` hook for each program)
- **Evidence against:** driver 1.

### Option C: leave the names, stubs and mark to the port (no `EntryKind::Own`; the shell hands a read-only handle to every child itself)
- **Evidence against:** the shell does not start children of a POSIX program; `libposix` does (ADR-0107), so the mark has to be in the table.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The mark** (ADR-0341 item 7) is a new bootstrap entry kind, `EntryKind::Own = 5`: a directory entry. `libposix`
   gives a child (`inherit`, `fork_moves`) a second connection with `READ | CLONE` only; the child's table keeps the
   kind, so a grandchild gets the same. A link met outside the entry that leads into it is refused with `EACCES`
   (`open_resolved`). A literal path typed to the entry resolves as always.
2. **The names list** is `user/describe/data-names.txt` (`NAME program...`). The lint of INV-0341-4 and INV-0340-2 is
   `cargo test -p zoraya-describe --test data_names`, not a step of `make check`: a new step needs lines in the lane,
   CI and plan files, and the test runs in `make test-unit` and `make test-changed`.
3. **The view path.** `~` in a `data` line is `/home` (`grants::USER_HOME`), the home of the image's user database;
   a `data` path is `~/` and plain segments only. `HOME` of such a program is `/home`, and a line that sets `HOME`
   or names the view path in `+r`/`+w` is refused.
4. **The shell's own entry.** The panel's `terminal()` adds `/data/home` (or `/data/ROOM`) to the shell's holds, to
   change anything in, and not to the record's entries (no pill, not in `/config/rooms`). `namespace::record` makes
   missing levels of a hold the shell may change. `zosh` makes `/data/ROOM/NAME` when the program first runs.
5. **The current directory** (item 8) gives nothing at `/`, `/data`, `/data/ROOM` (or `home`) and inside
   `/data/ROOM/NAME` for a listed `NAME`; a value inside it is not covered; narrowing of file operands is unchanged.
6. **`reach`** names under it get public answers only, as curl's URL names do (the architect's decision of
   2026-11-14); an address keeps its own rule; `USER@HOST` takes the host after the last `@`; a bad `-p` gives no
   grant. `rest` stops option reading (including `--help` and `--`) once the operands before it are in; the shell writes
   `--` before the host (not doubled). A description with `reach` or `data` is held to its options as the userland's
   are.
7. **No surface change for ssh.** The link probe of OpenSSH 10.6p2 found nineteen names `libc.a` lacks (`sleep`,
   `initgroups`, `execl`, `execlp`, `madvise`, `getifaddrs`, `freeifaddrs`, `getservbyname`, `gethostbyaddr`,
   `openlog`, `closelog`, `syslog`, `__res_state`, `__h_errno_location`, `res_init`, `res_query`, `dn_expand`,
   `ns_get16`, `ns_get32`). `ports/apps/sshshim.c` supplies them (`sleep` over `nanosleep`; the others fail with an errno
   or do nothing), as `zshim.c` does for curl; none is reached on a path a Zoraya ssh takes. ADR-0342 item 11's
   `getnameinfo` and `getservbyname` were not needed (OpenSSH's own numeric `getnameinfo` replacement is used), so no
   `posix-surface-added` block is written and the number 0346 is unused.
8. **The port.** OpenSSH portable 10.6p2, configured as `--host=aarch64-unknown-elf` (no Linux defaults), with three
   patches under `__zoraya__` (no configuration files; only `-T -V -t -v -l -p`; the options end at the host; a user
   name is needed; no askpass variables; no agent). `ssh` and `ssh-keygen` only.
9. **The test server** is the host's own `sshd` on the loopback with throwaway keys (`tools/test-sshd.sh`), reached
   through `--net-guestfwd` relays; the guest's client key is placed on the test disk with mode 600. CI's image and
   fallback step install `openssh-server`. The ssh script is part of `make test-apps`, not `test-net`, because only that
   step has the apps' image.
10. **Not built** (open): the storage limit on `/data/ROOM` (ADR-0341 item 6; the creating shell holds no
    connection to `/data` to set it, so it needs the keeper or a `/data` hold); `scp` and `sftp` with a `HOST:PATH`
    kind; the `help` FILES list names both paths by hand, not from the description.

## Consequences

- **Positive.** ssh and ssh-keygen run with their own directory; no accepted ADR changed.
- **Negative.** A new ABI value; a shim of 19 names; a CI package.
- **Follow-up.** The quota; 10f's planner changes rebase on `grants.rs`; 10h's `repository` key.

## Invariants

- **INV-0390-1.** `EntryKind::Own` is a directory entry and a child's connection to it carries no write right. *Check:* `cargo test -p zoraya-abi` and `cargo test -p zoraya-posix --test files`.
- **INV-0390-2.** Every name `ssh` and `ssh-keygen` import that `libc.a` lacks is in `sshshim.c`. *Check:* `make -f ports/Makefile openssh` links.

## Open questions

- The storage limit above; whether the own directory should be a pill in a window's frame.

## Revisit triggers

- A program with a `data` line that must fork and keep write access in its child.

## Architect's decision
