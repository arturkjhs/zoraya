# ADR-0393: What part 10g decided where ADR-0340 and ADR-0341 are silent

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
7. **No surface change for ssh from 10g.** ADR-0346 (ports-early) holds the block (`openlog`, `closelog`, `syslog`
   from musl) and `ports/apps/sshshim.c` supplies the other names OpenSSH's link probe finds missing
   (`getservbyname`, `getifaddrs`, `freeifaddrs`, `execlp`, `madvise`, `gethostbyaddr`, `initgroups`);
   10g keeps that one shim (without its early-build marker, since the programs are now in the apps' image) and finds no
   other name undefined after the merge. None is reached on a path a Zoraya ssh takes.
8. **The port.** OpenSSH portable 10.6p2 and ADR-0346's four patches (`0001` to `0004`: the include, no configuration
   file and no `-o`/`-F`, no options after the host, a user name needed, no askpass, no DNS host-key check) plus four
   of 10g under `__zoraya__`: `0005` (`getopt` offers only `-T -V -t -v -l -p`), `0006` (no agent), `0007` (no hard
   links: `known_hosts.old` is a copy and `known_hosts` stays until replaced; `ssh-keygen -R` renames) and `0008` (`closefrom` closes descriptors one by one: configure finds `/proc/PID/fd` on a Linux build host, and `libc.a` has no `dirfd`). The message of
   a missing user name is ADR-0346's.
9. **The test server** is the host's own `sshd` on the loopback with throwaway keys (`tools/test-sshd.sh`), reached
   through `--net-guestfwd` relays; the guest's client key is placed on the test disk with mode 600. CI's image and
   fallback step install `openssh-server`. The ssh script is part of `make test-apps`, not `test-net`, because only that
   step has the apps' image.
10. **Not built** (open): the storage limit on `/data/ROOM` (ADR-0341 item 6; the creating shell holds no
    connection to `/data` to set it, so it needs the keeper or a `/data` hold). Until then a line in a `--disk` room can
    write `/data/ROOM` beyond the limit: recorded in `docs/design.md` and ADR-0341's open questions. `scp` and `sftp`
    with a `HOST:PATH` kind; the `help` FILES list names both paths by hand, not from the description.
11. **A room's own directory is made below the shell's hold.** A room's terminal shell holds `/data/ROOM` and nothing
    above; `own_connection` finds the deepest level that opens and makes only the levels below it. The first
    terminal's shell, after `room r`, does not hold `/data/r`: ssh and ssh-keygen are refused there with a hint that
    says so and names a terminal in the room (the alternative, a hold on all of `/data`, would reach every room's and
    every application's files). A room's terminals get the random service and the network its words allow (ADR-0462).
12. **Options.** A description with `data` or `reach` is held to its options; the program reads short options only, so a
    long form is refused with the short one named; `--help` with other words is refused; the first `-p` is the port, as
    OpenSSH reads it; `ssh://...` is refused as a host. A room's grant on the view path is refused like a line's.
13. **Inherited open files.** A file inside the own directory that the program holds open for writing stays writable
    in a child that inherits the descriptor (`share_rights`): ADR-0341 item 7 says a child "never holds a handle that
    can write". Not reachable with ssh (it starts no child); to be closed before 10h's git starts hooks (a descriptor
    remembering its entry, or `O_CLOEXEC` on files of the own directory). INV-0341-6's Status says so.
14. **ADR-0341's tensions.** `/data/ROOM` also holds the applications' directories (`/data/ROOM/APP`, ADR-0245): the
    shell can now name them to read and write; recorded here for the architect, not changed.
15. **The test server** forwards nothing and runs nothing of the guest's (`DisableForwarding`, `ForceCommand` that
    prints the command), and makes `/run/sshd` when it runs as root (CI).

## Consequences

- **Positive.** ssh and ssh-keygen run with their own directory; no accepted ADR changed.
- **Negative.** A new ABI value; a CI package.
- **Follow-up.** The quota; 10f's planner changes rebase on `grants.rs`; 10h's `repository` key.

## Invariants

- **INV-0393-1.** `EntryKind::Own` is a directory entry and a child's connection to it carries no write right. *Check:* `cargo test -p zoraya-abi` and `cargo test -p zoraya-posix --test files`.
- **INV-0393-2.** Every name `ssh` and `ssh-keygen` import that `libc.a` lacks is in `sshshim.c`. *Check:* `make -f ports/Makefile openssh` links.

## Open questions

- The storage limit above; whether the own directory should be a pill in a window's frame.

## Revisit triggers

- A program with a `data` line that must fork and keep write access in its child.

## Architect's decision
