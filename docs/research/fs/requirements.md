# What the filesystem must support (measured)

Part 1 of [TASK-06](../../tasks/TASK-06-filesystem-design.md). Every number below comes from data already in the
repository, produced by TASK-02 and TASK-02b. No new measurement was made. Where the data cannot answer a question,
this file says so (section 5). Where a statement comes from outside the data, it is marked `[outside evidence]` with
its source (listed in [`survey.md`](survey.md#sources)), or `[inference]`.

This is evidence for the proposed ADRs 0010–0015, not a decision.

## Sources and queries

| Short name | File | What it holds |
|---|---|---|
| libc CSV | `docs/research/task-02/libc-surface.csv` | per libc symbol: category, static importers, dynamic callers with workloads, first tier |
| syscall CSV | `docs/research/task-02/syscall-surface.csv` | per Linux syscall: programs, call count per workload, argument patterns (top patterns only) |
| patterns TSV | `tools/posix-surface/results/strace-patterns.tsv` | per tier, run kind, program and syscall: argument-pattern counts |
| call sites | `docs/research/task-02/callsites.csv` | M3 call sites with review class and notes |
| delta CSV | `docs/research/task-02b/syscall-delta.csv` | serial vs parallel syscall counts per workload (TASK-02b) |
| applets | `tools/posix-surface/results/applets-by-tier.tsv` | toybox/busybox applets each tier ran |
| method | `docs/research/task-02/method.md` | how the data was made, and its limits |

Tiers are as defined in TASK-02 (`task-02/method.md`, "Tiers, runs and sets"): T0 = W5 with dash; T1 adds W1
(tcc self-build), W2 (Lua), W3 (SQLite); T2 adds W4 (GNU hello `configure && make`); T3 is W6 (clang/lld) alone.
"Core runs" exclude comparison programs (pdpmake, oksh, mrsh, busybox sh), probe runs and `*-check` runs.

Queries used (run from the repository root; Python 3 standard library only):

```sh
# Q-L: libc rows of the file and directory categories, plus rename/remove/fsync family, with tier and callers
python3 -I -c 'import csv; [print(r["symbol"],r["category"],r["tier"],r["static"],r["dynamic"],sep=" | ")
  for r in csv.DictReader(open("docs/research/task-02/libc-surface.csv"))
  if r["category"] in ("file","dir") or r["symbol"] in ("rename","remove","fopen","fdopen","mmap","memfd_create")]'

# Q-S: per-tier totals of a syscall over the core workloads of each tier
python3 -I - <<'EOF'
import csv
tiers={'T0':['W5-dash'],'T1':['W1-configure','W1-make','W2-make-g','W3-build'],
       'T2':['W4-configure','W4-make'],'T3':['W6-compile','W6-link']}
for r in csv.DictReader(open('docs/research/task-02/syscall-surface.csv')):
    c=dict(x.split(':') for x in r['call_count_per_workload'].split(';') if x)
    print(r['syscall'],*[sum(int(c.get(w,0)) for w in ws) for ws in tiers.values()],r['programs'])
EOF

# Q-P: per-program argument patterns in core runs
awk -F'\t' '$2=="core" && $4=="openat"' tools/posix-surface/results/strace-patterns.tsv

# Q-C: call sites for the TASK-01 filesystem entries
grep -E 'DM-03[6-9]|rename|readlink|symlink|mtime|mkstemp' docs/research/task-02/callsites.csv
```

**Limit that matters here.** The syscall CSV's per-workload counts include programs built by the workload itself
(`built`: the stage-2 tcc, `conftest` binaries, `lua`, ...). The per-tier totals from Q-S therefore include them; the
per-program counts from Q-P separate them. Per `task-02/method.md` ("Attribution"), `built` programs are not part of
any tier's surface. A second limit: `read`, `write`, `readv`, `writev` and `lseek` count calls on *every* kind of fd
(regular files, pipes, terminals, `/dev/null`); the data does not separate file I/O from pipe and terminal I/O, so
these columns say nothing about whether the workload reads or writes files more.

## 1. Filesystem functions and syscalls used in T0–T2

### 1.1 libc functions

From Q-L. "Static" lists the minimal-build importers (`toybox-T2` etc.) and full programs; "called" lists the
programs the interposer saw calling the function (dynamic lower bound). Rows whose tier is `T3`, `comparison` or
`unused-applets` are listed separately below the table.

| Function | Tier | Imported by (T0–T2 programs) | Called by (T0–T2) |
|---|---|---|---|
| `open` | T0 | all T0–T2 programs | dash, tcc, busybox (awk, diff), toybox (cat, cpio, grep, head, sed, touch, wc, xargs) |
| `openat` | T0 | toybox | toybox (cp, find, install, rm) |
| `fopen` / `fdopen` | T1 | gmake, tcc, busybox, toybox | gmake, tcc, busybox awk/diff, toybox grep/sed |
| `stat` | T0 | all | dash, gmake, busybox (awk, diff, expr, gzip, tr), toybox (cp, cpio, install, ln, mv) |
| `fstatat` | T0 | toybox | toybox (chmod, cp, cpio, find, install, ls, mkdir, rm) |
| `lstat` | T0 | dash, gmake, toybox | toybox rm |
| `fstat` | T0 | dash, gmake, busybox, toybox | gmake, busybox awk |
| `access` / `faccessat` / `eaccess` | T0/T0/T1 | toybox, dash, gmake | toybox (cp, install, rm, touch), dash, gmake |
| `lseek` | T0 | dash, tcc, busybox, toybox | dash, tcc, busybox diff, toybox grep |
| `opendir` / `fdopendir` / `readdir` / `closedir` | T0 | dash, gmake, toybox, busybox | dash, gmake, toybox (find, rm) (busybox only in the busybox-sh comparison run) |
| `getcwd` / `chdir` | T0 | all | dash, gmake, tcc (`getcwd` also pdpmake, a comparison program; busybox only in the busybox-sh comparison run) |
| `fchdir` | T2 | toybox | none |
| `mkdir` / `mkdirat` | T2/T1 | toybox | toybox (cpio, mkdir) |
| `rmdir` | T2 | toybox | toybox rmdir |
| `unlink` / `unlinkat` / `remove` | T1 | gmake, tcc, busybox, toybox | gmake, tcc (`unlink`, `remove`), toybox rm (`unlinkat`) |
| `rename` | T1 | toybox, busybox | toybox mv |
| `mkstemp` / `tmpfile` / `mkdtemp` | T1/T1/T2 | gmake, busybox, toybox | gmake (`mkstemp`), toybox mktemp (`mkdtemp`) |
| `mkfifo` | T1 | gmake, busybox | none in TASK-02; called by gmake in TASK-02b (`mknodat` in the delta CSV) |
| `symlink` / `symlinkat` | T2/T1 | toybox | toybox ln (`symlink`) |
| `readlink` / `readlinkat` | T0 | gmake, toybox | toybox rm (`readlinkat`) |
| `link` / `linkat` | T2/T1 | toybox, busybox | **none** |
| `realpath` | T1 | gmake, tcc, pdpmake, busybox | none in T0–T2 (clang only, T3) |
| `utimensat` / `futimens` / `utimes` | T0/T0/T2 | toybox, busybox, pdpmake | toybox (cpio, touch) (`utimensat` only) |
| `chmod` / `fchmod` / `fchmodat` | T2/T1/T1 | toybox, busybox | toybox chmod (`fchmodat` only) |
| `fchown` / `fchownat` / `lchown` | T1/T1/T2 | toybox, busybox | **none** |
| `umask` | T0 | dash, gmake, toybox, busybox | dash, gmake, every toybox applet run in T2 |
| `ftruncate` | T1 | gmake, toybox, busybox | none in T0–T2 (ld.lld only, T3) |
| `fgetxattr` / `flistxattr` / `fsetxattr` / `getxattr` / `lgetxattr` | T1–T2 | toybox, busybox | **none** |
| `mknod` / `mknodat` | T2/T1 | toybox, busybox | none |
| `memfd_create` | T0 | dash | dash (W4) |
| `tee`, `pipe`, `dup`, `dup2`, `fcntl` | T0 | shells, gmake, toybox | shells, gmake, toybox (fd plumbing, not storage) |

Not in T0–T2 at all (Q-L): `fsync`, `fdatasync`, `sync`, `syncfs`, `flock`, `posix_fallocate`, `posix_fadvise`,
`chown`, `creat`, `mktemp` are `unused-applets` (imported only by the full toybox/busybox binaries, never called);
`truncate`, `statfs`, `fstatfs`, `statvfs`, `pread`, `sendfile`, `dirfd`, `pathconf`, `mmap`, `munmap` are T3
only; `glob` is `comparison` (mrsh). `msync`, `pwrite`, `nftw`, `ftw`, `lockf`, `tmpnam`, `mkostemp` do not appear
in the CSV.

### 1.2 Linux syscalls

From Q-S (totals per tier, core workloads, including `built` programs) and the syscall CSV's `programs` column.

| Syscall | T0 | T1 | T2 | T3 | Programs (all runs) |
|---|---|---|---|---|---|
| `openat` | 7 | 4,807 | 27,928 | 174 | all |
| `read` | 17 | 9,237 | 65,748 | 35 | all |
| `write` | 24 | 43 | 93,001 | 4 | built, busybox, clang, dash, oksh, toybox |
| `writev` | 1 | 581 | 13,136 | 0 | built, busybox, gmake, mrsh, pdpmake, tcc, toybox |
| `readv` | 0 | 54 | 7,772 | 0 | built, busybox, gmake, tcc |
| `lseek` | 0 | 1,214 | 7,381 | 12 | built, busybox, clang, dash, ld.lld, pdpmake, tcc, toybox |
| `newfstatat` | 14 | 660 | 7,169 | 117 | most |
| `fstat` | 0 | 20 | 65 | 73 | built, busybox, clang, gmake, ld.lld, oksh, toybox |
| `faccessat` / `faccessat2` | 2 / 0 | 0 / 80 | 29 / 250 | 86 / 0 | toybox, shells; dash and gmake (`faccessat2`) |
| `getdents64` | 2 | 231 | 999 | 16 | busybox, clang, dash, gmake, mrsh, oksh, toybox |
| `getcwd` / `chdir` | 1 / 3 | 20 / 7 | 366 / 23 | 2 / 0 | shells, gmake, tcc, pdpmake, busybox |
| `mkdirat` | 0 | 0 | 16 | 0 | toybox |
| `unlinkat` | 0 | 71 | 1,110 | 1 | built, gmake, ld.lld, tcc, toybox |
| `renameat` | 0 | 1 | 90 | 2 | clang, ld.lld, toybox |
| `symlinkat` | 0 | 0 | 9 | 0 | built, toybox |
| `readlinkat` | 0 | 0 | 7 | 22 | clang, toybox |
| `utimensat` | 2 | 0 | 7 | 0 | toybox |
| `fchmodat` | 0 | 0 | 4 | 0 | toybox |
| `copy_file_range` | 0 | 0 | 94 | 0 | toybox |
| `umask` | 10 | 23 | 4,303 | 0 | dash, gmake, toybox |
| `memfd_create` | 0 | 0 | 10 | 0 | dash |
| `ftruncate` | 0 | 0 | 0 | 2 | ld.lld |
| `pread64` | 0 | 0 | 0 | 19 | clang, ld.lld |

**Absent from every run of TASK-02** (not in the syscall CSV): `fsync`, `fdatasync`, `sync`, `syncfs`, `msync`,
`link`, `linkat`, `rename` (the old syscall), `renameat2`, `truncate`, `fallocate`, `fchmod`, `fchown`,
`fchownat`, `statx`, `statfs`, `fstatfs`, `flock`, `utimes`, `mknodat`. `mknodat` appears in TASK-02b only: GNU
make creates its jobserver FIFO with it, once per FIFO-style parallel build (delta CSV, `mknodat ... S_IFIFO`).

### 1.3 Open flags and who uses them

From Q-P (core runs, per program; `built` programs omitted).

| Flags | Count by program and tier | What it is (from the call sites or the program) |
|---|---|---|
| `O_RDONLY` | tcc 3,998 (T1), 24,796 (T2); gmake 10/386; dash 1/291; toybox 222 (T2) | reading sources, headers, makefiles, scripts |
| `O_RDONLY\|O_DIRECTORY\|O_CLOEXEC` | gmake 97 (T1), 141 (T2); dash 1/113 | `opendir` (make's directory cache, shell globbing) |
| `O_WRONLY\|O_CREAT\|O_TRUNC` | tcc 55 (T1), 339 (T2); dash 7 (T1), 892 (T2) | tcc writing `.o` and executables; shell `>` redirections |
| `O_RDWR\|O_CREAT\|O_TRUNC` | tcc 2 (T1), 1 (T2); toybox 3 (T2) | tcc output; toybox |
| `O_WRONLY\|O_CREAT\|O_APPEND` | dash 13 (T1), 380 (T2) | shell `>>` redirections (configure's log) |
| `O_RDWR\|O_CREAT\|O_EXCL` | gmake 2 (T2); clang 1, ld.lld 2 (T3, with `O_CLOEXEC`) | `mkstemp` (gmake, call sites); temporary outputs (T3) |
| `O_WRONLY\|O_CREAT\|O_EXCL\|O_NOFOLLOW` | toybox 1 (T2) | exclusive create without following a link |
| `O_RDONLY\|O_CREAT` | toybox 2 (T0), 2 (T2) | `touch` |
| `/tmp` prefix | dash 2, gmake 4, tcc 2, toybox 1 (T2); plus `mkdirat` toybox 1, `unlinkat` gmake 2, toybox 1 | temp files (gmake `mkstemp`), `mktemp -d` |

TASK-02b records `fcntl(F_SETFL, O_WRONLY|O_APPEND)` 12 times and `F_SETFL O_RDONLY|O_NONBLOCK` 6 times in each W2
`gmake -j` variant (delta CSV, `fcntl` rows); the serial TASK-02 core runs also show gmake calling `F_SETFL` (10 in
T1, 15 in T2, Q-P) without recording the flags, so nothing shows the `O_APPEND` setting is specific to parallel
builds. The data does not record which fds.

**File mappings.** No T0–T2 program maps a file. Every `mmap` in T0–T2 core runs is `MAP_PRIVATE|MAP_ANONYMOUS`
(read-write, or `PROT_NONE` reservations) (Q-P). File mappings appear only in T3: `PROT_READ|PROT_EXEC`/
`MAP_PRIVATE` (clang 18, ld.lld 14: shared libraries), `PROT_READ`/`MAP_PRIVATE|MAP_NORESERVE` (ld.lld 7) and
`PROT_READ|PROT_WRITE`/`MAP_SHARED|MAP_NORESERVE` (ld.lld 2: its output). ADR-0005 already excludes `MAP_SHARED`.

## 2. What the toolchain depends on for correctness

### 2.1 Durability is never requested

No measured program calls `fsync`, `fdatasync`, `sync`, `syncfs` or `msync` in any tier, T0 to T3 (section 1.2;
the libc rows are `unused-applets`, imported only by the full toybox and busybox binaries). The M3 extractor searched
the compiled code of dash, oksh, mrsh, tcc, GNU make, pdpmake and the minimal T2 builds of toybox and busybox for
`fsync|fdatasync|sync|syncfs` (`tools/posix-surface/analysis/callsites.py`, feature "DM-037 fsync"), and the call-site
table has no row for it. M3 did not search clang, ld.lld or the full multi-call binaries; for those the syscall data
alone shows no call. The measured toolchain therefore **never asks for durability**. What survives a crash in these workloads is decided entirely by what the filesystem
does without being asked.

### 2.2 Atomic rename

- **Who renames.** In T0–T2 only toybox calls `rename` (`renameat`: 1 in W1-configure, 66 in W4-configure, 24 in
  W4-make; the other programs in the row are clang and ld.lld, which run only in W6). GNU make, dash, tcc and busybox
  never rename (section 1.2, `renameat` programs).
- **What for** (call sites, DM-038 rows, all reviewed `direct`):
  - toybox `mv` (`toys/posix/cp.c:454`): `mv` is how the configure scripts and makefiles move generated files into
    place. Which scripts called `mv`, and whether they used it as an atomic replace, is not in the data: the scripts
    are workload input, not measured code.
  - toybox `sed -i` (`lib/xwrap.c:476` `xrename`, with `lib/xwrap.c:484` `xtempfile`): writes a temp file *next to
    the target* (not in `/tmp`) and renames it over the target.
  - toybox `ln -f` (`toys/posix/ln.c:77,89`): reserves a temp name with `mkstemp`+`unlink`, links there, renames over
    the target.
- **T3 does it too.** clang and ld.lld each create a temporary output with `O_RDWR|O_CREAT|O_EXCL` (Q-P) and rename
  it: `renameat` is called once in W6-compile and once in W6-link (syscall CSV; attributed to clang and ld.lld by
  workload, since the patterns TSV has no `renameat` rows). That is the replace-by-rename idiom for compiler output.
- **tcc does not.** tcc unlinks its output and recreates it in place: `unlinkat` 55 (T1) and 258 (T2) relative
  paths, and 55/339 opens with `O_WRONLY|O_CREAT|O_TRUNC` (Q-P). A crash while tcc writes leaves a partial file under
  the final name.
- **Correctness consequence.** The idioms that *are* used (`sed -i`, `ln -f`, `mv`, T3 outputs) need rename to
  replace the target atomically: after any crash, the name refers either to the complete old file or to the complete
  new one. The data does not show any program depending on rename being *durable* (nothing calls `fsync` before or
  after it, 2.1).

### 2.3 Exclusive create

- `O_EXCL` appears 3 times in T2 core runs: gmake 2 (its `mkstemp`, which the call sites list as a private temp
  file, `direct`) and toybox 1 (`O_EXCL|O_NOFOLLOW`). T3 adds clang 1 and ld.lld 2 (Q-P).
- toybox `mktemp -d` uses `mkdtemp` (libc CSV, dynamic `toybox:mktemp`); creating a directory fails if the name
  exists, which is the same exclusivity for directories.
- Correctness depends on create-if-absent being atomic with respect to other creators in the same directory: two
  `mkstemp` calls must never both succeed on one name.

### 2.4 Unlink while in use, and cleanup on interrupt

- On Ctrl-C or SIGTERM, each leaf GNU make deletes the target its running recipe was writing: 4 partial targets in 35
  runs and 3 in one SIGTERM run, with no partial target left behind in 36 of 36 runs (`task-02b/conflict-impact.md`;
  `task-02b/research-log.md`, which corrects an earlier "4 in 36/36"). In the timelines the recipe process has already
  died (W9a) or been killed by the make (W9b) before the make unlinks the target (`task-02b/event-timelines.md`), so
  the partial-target data does not show an unlink while another process holds the file open.
- GNU make's top-level process deletes the FIFO jobserver node by name, including from its fatal-signal handler,
  while the sub-makes still hold it open (W9a +0.5 ms, W9b +0.3 ms, `task-02b/event-timelines.md`; delta CSV:
  `unlinkat path=GMfifo...` in every FIFO run). This is the measured unlink-while-open.
- Requirement: unlinking a name must succeed while the object is open, and the open handle must keep working (ADR-0006
  already requires this of FIFO nodes: "`unlink` removes the name while open handles stay valid").

### 2.5 Timestamps

GNU make decides what to rebuild by comparing modification times (section 4.1). tcc's in-place output (2.2) means a
crash during a compile can leave a partial `.o` whose modification time is newer than its sources. make would then
treat it as up to date [inference from the make rule quoted in 4.1 and the tcc pattern in 2.2]. make only protects
against this on *interrupt* (it deletes the partial target), not on *crash*. No filesystem guarantee about ordering
can make a partially written file complete; the guarantee that matters is that a crash never leaves a file whose
recorded size or modification time claims more than was written (see ADR-0010).

### 2.6 Appends

dash opens with `O_APPEND` 393 times in T1–T2 (Q-P), for `>>` redirections: configure appends to its log from
many short-lived processes in sequence. Under parallel make, GNU make sets `O_APPEND` on some fd with `F_SETFL` (12 times in W2 `-j2`, section 1.3). The data does not show two processes appending to the same *file* concurrently, so whether concurrent
appends must not interleave within a write is not established by the measurements.

## 3. Links

### 3.1 Hard links

- **Never created.** `link`/`linkat` are imported by toybox (T1/T2 minimal builds) and busybox but never called, and
  no `link`/`linkat` syscall appears in any run (sections 1.1, 1.2).
- toybox `ln` without `-s` would create one; `ln -f` creates a link under a temp name then renames
  (`toys/posix/ln.c:77`), but the measured `ln` calls were `ln -s` (`symlink` is the only link call toybox `ln` made,
  libc CSV).
- No data shows any program reading a link count (section 5).

### 3.2 Symbolic links

- **Created only in T2, by `ln -s`.** `symlinkat`: 9 calls in W4-configure, of which toybox 6 (`path=dirfd`) and
  `built` 3 (configure test programs) (Q-P). libc: `symlink` called by `toybox:ln` only. None in T0 or T1.
- **What they were for.** Autoconf-generated shell scripts probe whether `ln -s` works when they start
  (`_AS_LN_S_PREPARE` in autoconf's `m4sh.m4`: `ln -s conf$$.file conf$$`, then a second `ln -s` into a directory),
  delete the probe files at once, and fall back to `ln` (a hard link) and then to `cp -pR` if `ln -s` fails
  `[outside evidence: survey source F-37]`. The 9 `symlinkat` calls in W4-configure are consistent with these probes
  in `configure` and `config.status` [inference: the traces do not record the target names]. hello's build therefore
  needs symlinks only to answer "does `ln -s` work?", and has a fallback when the answer is no.
- **Read.** `readlinkat`: toybox 7 (T2, relative paths; libc CSV: `toybox:rm`); clang 22 (T3, 12 of them under
  `/proc`, i.e. `/proc/self/exe`). `readlink` dynamically: clang only (T3).
- **Followed.** The data does not record path resolution through links, so how often a *workload* followed a
  symlink is unknown. Two structural facts are known:
  - The measurement setup itself is built from symlinks: every `PATH` set is a directory of symlinks to the measured
    binaries (`task-02/method.md`, "Sets"), so every `execve` of a measured program resolved a symlink. That is a
    property of the setup, not of the programs; on Zoraya the launcher's namespace table plays this role (ADR-0002
    rule 5).
  - toybox dispatches on `argv[0]` and follows the symlink chain with `readlink` to find the applet name
    (`main.c:320` `toybox_main`, reviewed `direct`, note: "native install can use hardlinks/argv[0] name or `toybox
    <applet>`; only matters if no symlinks").
- **Call sites that depend on symlink semantics** (20 DM-039 rows in the call-site table: 16 in core programs, toybox
  14 and GNU make 2, of which 13 are distinct sites because 3 toybox `lchown` rows duplicate another row; 4 in oksh, a
  comparison program): toybox
  `dirtree` and `cp -P/-a/-r` copy links as links (`direct`, already `*at`); `cp -s`, `cpio`, `tar -x` and `ln -s`
  create links (`emulate`: creating the link is the requested operation); `xabspath` canonicalises by resolving links
  (`restructure`, 20 lines, used by `tar` to keep extraction inside the target directory); GNU make `-L` walks link
  chains for mtimes (`src/remake.c:1635,1655`, `direct`, only with `-L`, which no workload used).
- **Classification by the TASK-02 reviewers.** Every symlink *creation* site is `emulate` "only if the native FS lacks
  symlinks"; every *follow* site is `direct` or `restructure`. No site was judged to need native symlinks to work.

## 4. Metadata read and written

**What the data records.** Calls, not fields. The stat-family calls show *that* metadata is read; they do not show
which fields the program used. The per-program counts (Q-P, core runs) are:

| Program | `newfstatat` T1 | `newfstatat` T2 | Other metadata calls | Known purpose (call sites, libc CSV) |
|---|---|---|---|---|
| GNU make | 624 | 1,892 | `faccessat2` 80 / 201; `fstat` | modification times of targets and prerequisites (`name_mtime`, `src/remake.c`); `eaccess` `X_OK` probe of `$PATH` candidates (2 sites, `emulate`) |
| dash | 29 | 1,798 | `faccessat2` 0 / 49 | `test -f/-d/-r/-w/-x/-O/-G` in configure scripts (`faccessat` `emulate`; `geteuid`/`getegid` for `-O/-G`), `$PATH` search |
| toybox | 4 | 3,345 | `faccessat` 29; `fstatat` | `find`, `rm -r`, `cp`, `install`, `ls` (with `-l` it prints mode, owner, size and mtime; the flags used were not recorded), `mkdir -p` |
| busybox | 3 | 106 | | `stat` from awk, diff, expr, gzip, tr |
| tcc | 0 | 0 | | tcc stats nothing: it opens headers by trying paths (`openat` only, Q-P) |

**Written metadata.**

- **Modes.** `umask` is called 4,336 times in T0–T2 (dash's builtin in configure, gmake, and every toybox applet at
  start-up; libc CSV). `fchmodat` 4 times (toybox `chmod`, W4). Files are created with a mode argument (`O_CREAT`).
  tcc's outputs must be executable: W1 runs the stage-2 tcc it built, W3 runs `sqlite3`, W4 runs `hello` and every
  configure test program (`task-02/method.md`, workload checks). The execute bit is therefore load-bearing: `X_OK`
  probes (`faccessat2` by gmake and dash) and `execve` of freshly built files both depend on it.
- **Owners.** `fchown`, `fchownat`, `lchown`, `chown` are never called (section 1.1). The toybox call sites that
  would set owners (`cp -p`, `install`, `cpio`, `tar`) are `emulate` "by definition"
  (`task-02/callsite-analysis.md#feature-5-identity-and-permissions`).
- **Timestamps.** `utimensat` 2 (T0) and 7 (T2), toybox `touch` and `cpio` (libc CSV). `futimens` and `utimes` are
  imported, never called.
- **Extended attributes.** Imported by toybox (`cp --preserve`), never called.
- **Anonymous files.** dash calls `memfd_create` 10 times in W4 (heredocs); the call sites put it on the `memfd` path
  of ADR-0003. It is an unnamed file, not a directory entry.

### 4.1 make and modification times

What GNU make does with times, from the data and its manual:

- **Rule.** "A target is out of date if it does not exist or if it is older than any of the prerequisites (by
  comparison of last-modification times)" `[outside evidence: GNU make manual, make.texi, "Rule Syntax"; survey
  source F-01]`. An equal time is therefore *not* out of date.
- **Interrupts.** make "may delete the target file that the recipe was supposed to update. This is done if the
  target file's last-modification time has changed since make first checked it" `[outside evidence: F-01,
  "Interrupting or Killing make"]`. The 36/36 deletions in TASK-02b (2.4) are this rule in action; it requires the
  modification time to change when the recipe writes the file.
- **Wall clock.** GNU make reads `CLOCK_REALTIME` to compare "now" with file modification times
  (`src/file.c:965` `file_timestamp_now`, reviewed: "correct use of wall clock; no change"); pdpmake stamps a rebuilt
  target that has no file with the current time (`make.c:484`); toybox `find -mtime` compares with `time()`
  (`toys/posix/find.c:715`). Modification times must therefore be on the same time base as the wall clock.
- **Resolution.** The data does not measure it (section 5). The manual says high-resolution timestamps "lessen the
  chance of make incorrectly concluding that a file is up to date", that commands which set a timestamp may discard
  the sub-second part, and offers `.LOW_RESOLUTION_TIME` to compare such files at one-second granularity; archive
  member times are always whole seconds `[outside evidence: F-01, ".LOW_RESOLUTION_TIME"]`.

**Required ordering behaviour** `[inference from the manual's rules above]`. Two rules depend on resolution:

- *Interrupts:* make deletes a partial target only "if the target file's last-modification time has changed since
  make first checked it" (F-01). A target rewritten within the same clock tick as make's first check keeps its time
  under a coarse clock, so its partial content survives the interrupt.
- *Rebuilds:* if a prerequisite is rewritten after its target within one tick, both get the same time and make
  (which treats equal as up to date) skips the rebuild. The manual says high-resolution timestamps "lessen the chance
  of make incorrectly concluding that a file is up to date" (F-01).

Both need modification times that change on every write and order later writes strictly after earlier ones: a
resolution finer than the interval between dependent writes, and a clock that does not step backwards between them.
As context for "how fine": in W8/W9 dependent steps follow each other well within a second (12 of 32 generated tables,
each a chain `mktab` → `tN.h` → `mN.o`, were complete 1.5 s after start with 4 leaf makes running;
`task-02b/event-timelines.md`, "State when the interrupt arrives"; `tools/posix-surface/workloads/w8/leaf.mk`). In
those chains the target is written *after* its prerequisite, where an equal time is harmless; the data therefore
shows the pace of builds, not a measured instance of either hazard. One-second resolution is not enough for that
pace; nanosecond storage with strictly increasing assignment is the conservative choice.

## 5. Questions the data cannot answer

| Question | Why the data cannot answer it |
|---|---|
| Which `stat` fields (size, mode, times, inode number, link count, device) each program reads | M1/M2 record calls, not fields; M3 did not search for field accesses |
| Whether any program compares inode numbers (`st_ino`/`st_dev`) for identity | as above. The only hint is a reviewer note on tcc's `#pragma once` (`libtcc.c:503`, `realpath` comparison): "compare file identity (st_dev/st_ino or handle identity) instead" |
| Whether any program reads link counts | as above |
| How often workloads *followed* a symlink during path resolution | strace logs the path given, not the resolution |
| Required timestamp resolution | not measured; section 4.1 gives the manual's statement and an inference |
| Whether concurrent writers append to one file | the traces do not correlate fds across processes |
| Largest file and largest directory | not recorded |
| Longest name and longest path | not recorded by the analysis (raw traces stayed in the measurement VM, `task-02/method.md`) |
| Whether any name was not valid UTF-8, contained control characters, or differed only in case | not exercised (`task-02/conflict-impact.md#x-8-strict-names-vs-posix-byte-names`) |
| Which configure/make steps used `mv` as an atomic replace | the scripts are workload input; only the `rename` calls are measured |
| Crash behaviour of any kind | no workload was crashed |

## 6. Requirements derived for the ADRs

Labels `R-n` are used by the ADRs. Each is a reading of the data above, not a decision.

| ID | Requirement | Basis |
|---|---|---|
| R-1 | Replacing a name by rename is atomic: after a crash the name refers to the whole old object or the whole new one. | 2.2 |
| R-2 | Exclusive create (`O_EXCL`, `mkdtemp`) is atomic with respect to other creators. | 2.3 |
| R-3 | A name can be removed while the object is open; open handles keep working. | 2.4 |
| R-4 | Without any explicit durability request, the post-crash state must still be consistent and must not expose data that was never written (stale blocks or zeros) under a size or time that claims it was. | 2.1, 2.5 |
| R-5 | Modification time changes on every write, is on the wall-clock time base, and has sub-second resolution. | 4.1 |
| R-6 | Mode bits (at least the execute bit) are stored and reported; `umask` and creation modes work. Owners are never set by the workloads. | 4 |
| R-7 | Symbolic links are created only by `ln -s` in T2 configure, as a capability probe with a `cp -pR` fallback; no site needs native symlinks to work (all creation sites `emulate` if absent). Hard links are never created. | 3 |
| R-8 | File mappings are not needed below T3. | 1.3 |
| R-9 | Anonymous (unnamed) files are needed (`memfd_create`, temp files that need only privacy). | 4, 2.3 |
| R-10 | A FIFO node can be created in the temporary directory and unlinked while open (ADR-0006). | 1.2, 2.4 |
| R-11 | Directory listing, recursive removal (`rm -r`, `find`) and directory creation work; `getdents` volume is about 1,000 per W4. | 1.2 |
