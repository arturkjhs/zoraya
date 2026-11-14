# ADR-0336: The userland is toybox's finished commands, each with a description; the gaps are filled by name, and busybox leaves

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0005, ADR-0107, ADR-0109, ADR-0153, ADR-0154, ADR-0155, ADR-0156, ADR-0200, ADR-0212, ADR-0335, ADR-0342
- **Evidence:** CP-01, CP-05, CP-27, DM-010, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#awk, stage-10/survey.md#bsd-and-other-permissive-sources, stage-10/survey.md#busybox, stage-10/survey.md#everyday-tools-compared, stage-10/survey.md#ports-as-built, stage-10/survey.md#the-userland-today, stage-10/survey.md#toybox-upstream, stage-10/survey.md#what-posix-has-and-what-people-rely-on, stage-10/survey.md#what-the-missing-tools-need-from-libc

## Context

A person in a Zoraya terminal has the shell's native commands (`cat`, `cp`, `ls`, `find`, `search`, `count` and
others) and, as POSIX programs, twelve toybox applets and busybox's `tr` and `diff`. There is no `grep`, `sed`,
`awk`, `tar`, `gzip`, `xargs`, `diff` under a permissive licence, or `patch`. Scripts from elsewhere, `configure`
(ADR-0005's tier T2) and git's own shell scripts assume them. This ADR decides which tools the image carries, from
which source, how a person and a script meet them, and in what order they arrive.

**What is known.**

- toybox 0.8.15 (0BSD) has finished versions of `grep`, `sed`, `find`, `xargs`, `tar`, `cpio`, `patch` (unified
  diffs), `cmp`, `dd`, `od`, `stat`, `du`, `df`, `date`, `tee`, `tail`, `uniq`, `cut`, `printf`, `test`, `seq` and
  of `gzip`, `gunzip` and `zcat` with its own 540-line deflate. In its `pending` directory, which its README calls
  unfinished or unreviewed: `awk`, `vi`, `diff`, `tr`, `expr`, `bc`, `sh`, `more`, `man`, `xzcat`. Absent: `less`,
  `ed`, a bzip2 or xz compressor, `unzip` (`stage-10/survey.md#toybox-upstream`,
  `stage-10/survey.md#everyday-tools-compared`).
- Zoraya builds 12 of toybox's applets; 20 of the 31 T2 applets and busybox's `awk`, `expr` and `gzip` do not link
  for want of about twenty libc names, some of which ADR-0310 has since added (`stage-10/survey.md#the-userland-today`).
- busybox has everything and is under GPL version 2 only (`stage-10/survey.md#busybox`).
- Other permissive sources: FreeBSD's tools as ported by chimerautils (BSD, with `libxo`, `flex`, `byacc` and
  `meson` to build), sbase (MIT, about 94 tools, no `awk`, `diff`, `patch` or `gzip`), OpenBSD's `diff` and `patch`
  (ISC-style and BSD-like), one-true-awk (a Lucent permission notice; own regex code; UTF-8; needs `bison` to
  build) (`stage-10/survey.md#bsd-and-other-permissive-sources`, `stage-10/survey.md#awk`).
- POSIX.1-2024 has `find -print0`, `xargs -0`, `diff -u` and `patch`; it has no `tar`, no `grep -r` and no `sed -i`,
  which scripts use daily (`stage-10/survey.md#what-posix-has-and-what-people-rely-on`).
- No toybox applet has a description, so the shell designates nothing for one (ADR-0153's open question;
  `stage-10/survey.md#the-userland-today`).
- Eighteen toybox applets look users up; Zoraya has one identity and no owners
  (`stage-10/survey.md#what-the-missing-tools-need-from-libc`; DM-010 is why).

**What is assumed.** That the unreviewed `pending` commands `tr`, `expr` and `diff` are sound enough after a
review and the tests named below; that a working day needs roughly the tools of the survey's comparison table.

## Decision drivers

1. **One source for most tools**, so that there is one build, one patch and one behaviour to learn (CP-27).
2. **A permissive licence for the default set**; a copyleft program only where no other exists, as a separate
   program (ADR-0109; `stage-10/survey.md#busybox`).
3. **Every tool on the image can be used as the shell means it to be**: with a description, so that operands
   designate and `grants` tells the truth (CP-01, CP-05, ADR-0153).
4. **The person meets one command per name** (ADR-0154, ADR-0155).
5. **Scripts written for Unix run**, including the options outside POSIX that they use
   (`stage-10/survey.md#what-posix-has-and-what-people-rely-on`).
6. **Order by need**: what unblocks T2's `configure` and the stage's own programs first
   (`stage-10/survey.md#the-userland-today`).

## Considered options

### Option A: GNU's tools (the Linux baseline)

- **Description:** coreutils, findutils, grep, sed, gawk, diffutils, tar, gzip.
- **Evidence for:** Driver 5 in full: these are the tools scripts are written against.
- **Evidence against:** GPL version 3 for the ones the survey read (diffutils), eight build systems of the
  autoconf kind, each wanting a hosted libc far beyond the surface (drivers 1 and 2, CP-27)
  (`stage-10/survey.md#what-posix-has-and-what-people-rely-on`).
- **Cost:** *(estimate)* The largest surface of any option, by a wide margin.
- **Precedent:** Linux distributions.

### Option B: busybox for everything

- **Description:** Enable busybox's applets; it has `awk`, `vi`, `less`, `ash`, `bzip2`, `ed`.
- **Evidence for:** Drivers 1, 5 and 6: one binary, already ported for `tr` and `diff`, complete
  (`stage-10/survey.md#busybox`).
- **Evidence against:** GPL version 2 only (driver 2); it is the licence toybox's author left it over
  (`stage-10/survey.md#toybox-upstream`). The project's present use of it is recorded as a stopgap.
- **Cost:** *(estimate)* Small.
- **Precedent:** Embedded Linux; Alpine.

### Option C: toybox's finished set, the gaps filled one by one

- **Description:** Build every finished toybox command that makes sense on Zoraya; take `tr`, `expr` and `diff`
  from its `pending` directory after review; `awk` from one-true-awk; `vi` and `less` as ADR-0335; nothing else
  until someone needs it.
- **Evidence for:** Drivers 1, 2, 3 and 6. toybox is already ported with one patch
  (`stage-10/survey.md#ports-as-built`), needs nothing beyond libc, walks trees with its own code, and its finished
  list covers most of the comparison table (`stage-10/survey.md#everyday-tools-compared`). Android ships it.
- **Evidence against:** The gaps are real: three commands come from unreviewed code, `awk` from a second source
  with a licence text of its own, and there is no `bc`, `ed`, xz or zip. toybox's commands are not GNU's in every
  option (driver 5, partly).
- **Cost:** *(estimate)* The missing libc names (ADR-0342); about sixty descriptions; a review of three pending
  files; one more port for `awk`.
- **Precedent:** Android.

### Option D: FreeBSD's userland (Chimera Linux's choice)

- **Description:** Port chimerautils: coreutils, findutils, diffutils, grep, sed, awk, patch, gzip, ed from
  FreeBSD.
- **Evidence for:** Drivers 2 and 5: complete, mature, BSD, with `awk`, `diff`, `ed` and `bc` in one family;
  Chimera chose it for "high quality code and solid feature set"
  (`stage-10/survey.md#bsd-and-other-permissive-sources`).
- **Evidence against:** It builds with `meson`, `flex`, `byacc` and `libxo` and leans on FreeBSD's libc (`fts`,
  the user database), a much larger port than toybox (driver 1, CP-27).
- **Cost:** *(estimate)* A new build system in the ports and several dozen libc names beyond ADR-0342's.
- **Precedent:** Chimera Linux; the BSDs themselves.

### Option E: Only native commands (Plan 9's way)

- **Description:** Write each tool as a native Zoraya command, as `cat`, `cp`, `find` and `search` already are.
- **Evidence for:** Drivers 3 and 4 by construction; every command follows ADR-0155 and has its description.
  Plan 9 wrote its tools new (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Driver 5 fails: scripts, `configure` and git call the Unix names with Unix options. It is
  the unbounded rewriting CP-27 warns of, and the postmortem's verdict on Plan 9 is that its ideas spread and its
  software did not.
- **Cost:** *(estimate)* Dozens of programs.
- **Precedent:** Plan 9; Zoraya's own first commands.

## Recommendation

**Recommendation (not a decision): Option C.**

1. **toybox is the POSIX userland.** One file in the tree, `ports/toybox-applets.txt`, lists the enabled commands,
   one per line, and the build takes its list from it. A command is listed when it is in toybox's finished
   directories and does not need what Zoraya lacks by design (users, devices, mounts, the Linux `/proc`). The first
   list is the finished rows of `stage-10/survey.md#everyday-tools-compared`: `cat cp mv rm ls mkdir rmdir ln chmod
   touch stat du df head tail sort uniq cut wc tee grep sed find xargs patch cmp tar cpio gzip gunzip zcat bzcat
   dd od xxd date sleep env printf test seq basename dirname realpath which file echo true yes uname`.
2. **Three from `pending`, after review:** `tr`, `expr` and `diff`. Each is enabled only when a reviewer who did
   not port it has read it and it passes toybox's own tests for it and the stage 6 workloads that today run
   busybox's (`w5` for `tr`, `w10` for `diff`). **busybox then leaves the image and `ports/sources.lock`.**
3. **`awk` is one-true-awk** (the second edition: UTF-8, its own regex code), as `/bin/awk`. Its grammar is turned
   into C on the host by `bison`, a build-time host tool recorded in `docs/dependencies.md`. Its Lucent notice is a
   permissive licence outside the list `CLAUDE.md` names for crates; for a separate ported program the project
   already accepts stricter ones (`wget`), and the architect is asked to confirm it (open question).
4. **Not in stage 10:** `bc`, `ed`, `man`, toybox's `sh`, `vi` and `more`, an xz or bzip2 compressor, `zip` and
   `unzip`. `xzcat` (toybox, `pending`) follows the rule of item 2 when a real archive needs it.
5. **A description for every command, or it is not installed.** Each enabled command gets a trusted description
   in `user/describe/posix/` naming `/bin/NAME`, with a kind for every operand and option argument that is a path
   (ADR-0156). The options a description lists are the options the shell lets through, as for `curl`. Examples of
   kinds: `grep PATTERN [FILE...]` reads its files, and `-r` makes them trees to read; `sed -i` makes its files
   `write`; `sort -o FILE` is `create`; `tar -c -f ARCHIVE PATH...` creates the archive and reads the paths, and
   `tar -x -f ARCHIVE -C DIR` reads the archive and gets `DIR` as a tree; `find PATH...` reads trees, and with
   `-delete` they become `remove-tree`. A command that runs others (`xargs`, `find -exec`, `env`) passes its own
   table to them (ADR-0107) and no more: `find . -name '*.o' -exec rm {} +` needs `+w .`, and says so by ADR-0153's
   hint.
6. **One command per name for the person.** `shell.path` stays `/zoraya/bin /bin`: where a native command and a
   toybox command share a name (`cat`, `cp`, `ls`, `mv`, `rm`, `head`, `find`, `mkdir`, `stat`, `date`, `sleep`),
   `zosh` runs the native one, and `help NAME` says so. A script run by `sh` has `PATH=/bin:/usr/bin` and gets
   toybox's. A person who wants the POSIX one types `/bin/ls`.
7. **Users.** The tools ask `libposix` and get the one identity of `/etc/passwd` in the toolchain set (uid 1000);
   `ls -l` prints it, `id` prints it, `chown` succeeds and changes nothing, `tar` stores and ignores it. No applet
   is patched for this.
8. **Order.** (a) the missing libc names and the image cache of ADR-0212, as the architect ordered on 2026-11-04;
   (b) the text tools `grep sed sort uniq cut tr head tail wc tee xargs find`, which is where T2's `configure`
   stops today; (c) the file tools; (d) `diff`, `patch`, `cmp`, `tar`, `gzip`; (e) `awk`; (f) the rest of the list.
   After (b) and (e), workload W4 is run for its measurement (ADR-0005's condition for T2).

**Examples.**

```
~/proj> grep -n TODO *.c
main.c:14:  /* TODO: check the length */
~/proj> grants grep -n TODO main.c
grep gets:
  in, out, err         the terminal
  ~/proj               read (current directory)
  ~/proj/main.c        read (operand FILE)
~/proj> sed -i s/colour/color/ notes.txt
~/proj> tar -c -z -f ~/backup/proj.tar.gz .
~/proj> grants tar -c -z -f ~/backup/proj.tar.gz .
tar gets:
  in, out, err         the terminal
  ~/proj               read (current directory)
  ~/backup             create or replace proj.tar.gz (option -f)
  ~/proj               read, and all in it (operand PATH)
~/proj> help ls
ls: list a directory (a native command; /bin/ls is toybox's, for scripts)
```

(each with the lines every POSIX program has). `sed -i` changes `notes.txt` with no grant because the operand
designates it and `sed` is in the image. On the desktop these are lines in a terminal window; the show-commands mode
has nothing to add, since they are already text.

**Tensions with accepted ADRs.**

- *ADR-0005* names busybox 1.38.0's `tr` and `diff` in tier T1 and its `awk`, `expr` and `gzip` in T2. Item 2 and
  item 3 replace them by toybox's and one-true-awk, with the same workloads as the acceptance tests. Accepting this
  ADR supersedes those words of ADR-0005's tier definitions; the surface itself is ADR-0342's.
- *ADR-0153's open question* ("which descriptions to ship for toybox applets, and whether toybox runs as POSIX
  programs with the toolchain set") is answered by items 5 and 6; its note that "toybox runs with the toolchain set
  and no description" ends.
- *ADR-0154 rule 6*: the hints for `grep` and `wc` when nothing of that name exists end for the names that now
  exist.
- *ADR-0109 item 6* lists the applets built; the list moves to the file of item 1.

### Effect of the vision

For target 2 the userland is where designation pays off: an agent's `sed -i` on one file gets that file, and
`grants` shows it, because every tool has a description. A tool without one would be either useless or a reason
to type `+w .`; item 5 forbids shipping one. Target 3: the names people know. Networking: none of these tools gets
the network. Threads: none.

## Consequences

- **Positive.** A day's tools under 0BSD from one already-ported source; busybox and its licence leave; every tool
  is designated and explained; T2's `configure` can proceed.
- **Negative.** About sixty descriptions to write and keep true, each a small security statement. Three commands
  rest on a review of unreviewed upstream code. Two `ls` exist, one for people and one for scripts. No `bc`, `ed`,
  xz or zip yet.
- **Follow-up.** The applet list file and its build rule; the descriptions and their lint; the review of `tr`,
  `expr`, `diff`; the `awk` port; removing busybox; `docs/dependencies.md`; W4's measurement.

## Invariants

- **INV-0336-1.** The commands in the image's `/bin` that toybox provides are exactly the lines of
  `ports/toybox-applets.txt`, and no line names a command from toybox's `pending` directory other than those item 2
  admits. *Check:* a script in `make check-ports` comparing the file, the built binary's command list and the
  directory of each command's source in the pinned archive.
- **INV-0336-2.** Every program in the image's `/bin` has a trusted description naming its path, and every option
  of that description that takes a path gives it a kind. *Check:* a lint in `make check` over
  `user/describe/posix/` and the image's file list.
- **INV-0336-3.** No program or archive under GPL version 2 only is in `ports/sources.lock` once item 2 is done.
  *Check:* `ports/README.md`'s licence table, compared by a script with `sources.lock`.
- **INV-0336-4.** For a name that is both a native command and a toybox command, `zosh` runs the native one and
  `sh` the toybox one. *Check:* an in-QEMU script running `ls --version`-style probes from both and matching each
  program's own output.
- **INV-0336-5.** A command started by `xargs`, `find -exec` or `env` holds exactly its parent's entries. *Check:*
  an in-QEMU script in which `find . -exec cat /config/settings` is refused without a grant and allowed with `+r`.

## Open questions

- Whether one-true-awk's licence text is acceptable for a ported program (it is permissive and not on
  `CLAUDE.md`'s list); the alternative is toybox's pending `awk` after the review of item 2.
- Which further finished toybox commands to list (hashes, `base64`, `nl`, `paste`, `comm`); each is one line and
  one description.
- Whether the native `cat`, `cp`, `ls` and the rest should give way to toybox's for people too, or the reverse; a
  count of which scripts break under each would settle it.
- Whether `ps`, `kill` and `top` from toybox make sense beside the shell's own, given that pids are the broker's.

## Revisit triggers

- The review of a `pending` command finds it unsound: take that tool from OpenBSD or sbase instead.
- Scripts in scope depend on GNU behaviour toybox lacks, repeatedly.
- toybox finishes `awk`, `diff` or `tr` upstream: the list follows.

## Architect's decision
