# TASK-31: review of 10g, a program's own directory and ssh (branch `work/10g`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff 47f59d26..cbade396`: the `data`, `reach` and `rest` keys (`zoraya-describe`, `zoraya-command`), the planner
(`crates/zoraya-shell/src/grants.rs`, `netgrant.rs`) and their tests, `zosh` (`posix_own`, `own_connection`,
`end_of_options`, `userland_options`, `enter_room`), the panel's `/data/ROOM` hold, `namespace::record`, `libposix`
(`clone_read_only`, `inherit`, `fork_moves`, `open_resolved`), `EntryKind::Own`, the OpenSSH port (`ports/Makefile`,
four patches, `ports/apps/sshshim.c`), `ssh.cmd`, `ssh-keygen.cmd`, `data-names.txt` and its lint, the test server
and script (`tools/test-sshd.sh`, `tools/test-apps-ssh.sh`, `tests/shell/apps-ssh.script`), the two changed files
scripts, CI, the invariants table and proposed ADR-0390. Read with the code around it: `Held` and `not_given` in
`zosh.rs`, `posix_env`, `net_for`, the unpacked OpenSSH 10.6p2 tree (`ssh.c`, `readpass.c`, `hostfile.c`,
`misc.c`). Against ADR-0340, ADR-0341 (both in full), ADR-0342 item 11, ADR-0230, ADR-0265 item 5, plan row 10g.

What was run here, in `zoraya-10g` at `cbade396`:

- `make check`: all checks passed (exit 0).
- `cargo test --no-fail-fast -p zoraya-describe -p zoraya-command -p zoraya-shell -p zoraya-panel -p zoraya-abi
  -p zoraya-posix`: 625 passed, 0 failed.
- Hand mutations, each against the check its invariant row names (restored after each): killed: inherit's `Own` arm
  to `clone_conn`, the link-into-`Own` refusal, `cwd_reaches_data` off, the value-inside-cwd rule, `own_directory`'s
  base-system and list gates, `own_conflict` to equality, `USER@` stripping, `-p` ignored, a bad `-p` granting,
  `--` doubled, `rest` off, `rest`'s `--help`, `Own.is_directory`, `is_data_path` with `..`, `reach` on a non-host
  operand, the panel's hold, `-i` added to `ssh.cmd`, a program added to `data-names.txt`. **Survived:**
  `fork_moves`' `Own` arm to `clone_conn` (finding 4), `|| into_data` in the narrowing rule (finding 18).
  (`.filter(|_| line.base_system)` in `net_for`'s `reach` also survives, but it is equivalent: `operand` is already
  `None` without the base system.)
- Read, not run: the last `target/test-logs/apps-ssh.log` and `build/apps-sshd/sshd-a.log` the implementer's run left
  (12:37); `gh run list --branch work/10g`: one run, 1m20s (the document checks), so no candidate run exists.

Not run: `make test-apps`, any GUI run, a terminal window's shell, CI.

## The ADR-number collision (asked for; not resolved here)

- **ADR-0390 is taken twice.** `origin/work/ports-early` (and `origin/work/10d`, which carries it) has
  `docs/adr/0390-surface-for-curses-less-and-vi.md` with its index row; 10g adds
  `docs/adr/0390-what-part-10g-decided.md` and a second `ADR-0390` index row. Both are in TASK-31's reserved range
  (0343 to 0349, 0380 to 0399).
- **ADR-0346 is not unused.** The 10g report says "0346 (a surface block for ssh) is not needed and stays unused";
  ports-early has `docs/adr/0346-surface-for-ssh.md` (proposed), which builds the same ssh.
- **The two ssh builds duplicate and conflict.** Same archive and hash (`openssh 10.6p2`, `9ae44872...`), but:
  - `ports/apps/sshshim.c`: added on both branches with different contents (83 and 144 lines): an add/add conflict.
  - `ports/sources.lock`, `ports/Makefile` (`openssh` target, `apps` stamp), `ports/README.md`,
    `ports/patches/README.md`: both add OpenSSH: textual conflicts.
  - `ports/patches/openssh/`: the file names differ on every patch (ports-early: `0001-fake-rfc2553-stdio`,
    `0002-no-configuration-files-no-options-after-host`, `0003-no-askpass`, `0004-no-host-key-dns`; 10g:
    `0001-ssh-reads-no-configuration-and-offers-only-the-lines-options`, `0002-no-askpass`, `0003-no-agent`,
    `0004-no-hard-links`), so git merges both sets **without a conflict** and `unpack` then applies eight patches
    whose hunks overlap in `ssh.c` and `readpass.c`: the build breaks after a clean-looking merge.
  - Decisions differ: ports-early puts `openlog`, `closelog`, `syslog` in a `posix-surface-added` block of `libc.a`
    (ADR-0346 item 4); 10g shims them in `sshshim.c` and writes no block (ADR-0390 item 7). ports-early refuses a
    missing user with `no user name: give one with -l USER or as USER@HOST`, 10g with `no user name: write -l USER
    or USER@HOST` (which `apps-ssh.script` expects). Only 10g has no-agent and no-hard-links (without the latter,
    `ssh-keygen -R` fails); only ports-early has no-DNS-key-check and the `fake-rfc2553` include.
- **What a merge of 10g onto ports-early needs:** one patch series, which is the union (configuration reader off,
  `getopt` restricted, no second pass after the host, user required, askpass off, agent off, no DNS key check, no
  hard links, the include), renumbered; one `sshshim.c`; one `openssh` target; one decision on the three syslog names
  (block or shim), and so on which ADR (ADR-0346 or 10g's) holds INV-0390-2's surface rule; 10g's ADR renumbered to a
  free number of the reserved range, with its index row, invariant rows and every citation (`grants.rs`,
  `ports/Makefile`, `ports/README.md`, `docs/adr/invariants.md`, the report); the invariants' Total recomputed
  (ports-early says 821, 10g 816, both from 814); the user-name message chosen once, and the script matched to it.

## Findings

1. **error (must-fix).** `user/src/bin/zosh.rs:1805-1828` (`own_connection`). **What.** The loop makes the levels of
   `/data/ROOM/NAME` from the top, starting with `root.open("data", OPEN_DIRECTORY, all)`. In a room's own terminal
   shell the root is `Held::Table` with exactly the room's grants, `/bin /usr /etc` and `/data/ROOM`
   (`crates/zoraya-panel/src/lib.rs:362-391`): no entry covers `/data` or `/`, so `Held::open` returns
   `ACCESS_DENIED` (`zosh.rs:605-607`), the fallback `root.open("", ...)` for `make_directory("data")` is refused the
   same way, and `own_connection` fails, though `/data/ROOM` is held and `namespace::record` made it. In the first
   terminal's shell entering a room (`room r`, then `ssh-keygen`), `not_given(.., "/data/r/ssh", true)` refuses,
   because that shell holds `/data/home`, not `/data/r`, and its hint ("put +w /data/r before ssh-keygen") cannot help.
   **Scenario.** On the desktop, `ssh` or `ssh-keygen` in any room's terminal window fails before it starts; the
   ADR-0340 example (`room new ops +net build.example.org:22`, then `ssh` in the room) and the report's hand check
   "In a room ... an empty directory `/data/r/ssh`" cannot pass. `apps-ssh.script` does not see it: it runs on the
   serial console, whose shell is `Held::Root` (`zosh.rs:5752-5755`), which covers everything; no test runs a
   terminal window's shell (`Held::Table`) with an own directory. INV-0341-2's QEMU half is therefore checked on the
   one shell kind that does not have the problem. **Fix.** Make only the levels below the entry that covers
   `own.real` (find it with `covering`, open it, create below), so a shell that holds `/data/ROOM` never touches
   `/data`; decide (and record) what an entered room in the first terminal does (refuse with a true hint, or have
   that shell's room get its directory another way). Test it with a table: a host test if the creation moves into a
   function over a trait, or a terminal-window script (a room's terminal, `ssh-keygen -t ed25519`, `ls /data/r/ssh`).

2. **error (must-fix).** `tools/test-sshd.sh:73-88`. **What.** The test servers run with OpenSSH's defaults for
   everything the configuration does not name: `AllowTcpForwarding yes`, `AllowStreamLocalForwarding yes`,
   `PermitUserRC yes`, no `ForceCommand`. An authenticated client can open `direct-tcpip` channels
   through the host's sshd to any address the host reaches, and runs commands in the real login shell of the user
   running the tests (root on CI's container, the developer on a Mac). **Scenario.** The guest is `restrict=on`
   (INV-0230-1), but with the client key on the test disk (readable by any program in the guest) a guest program,
   or a regression in the client under test (a `-W` or `-L` that comes back with a patch rebase), reaches the
   Internet or the host's other services through server B; the confinement now rests on the client being right,
   which is what the test is testing. The far-side words of the script (`-o ProxyCommand=true`, `-J ...`) are run by
   that shell as commands. **Fix.** Add `DisableForwarding yes`, `PermitTunnel no`, `PermitUserRC no` and a
   `ForceCommand` that prints `$SSH_ORIGINAL_COMMAND` (so the script still sees `far-side-says-hi`, `-o`, `-J`) and
   runs nothing; with no command given, a fixed message. Keep `AllowUsers`.

3. **error (must-fix).** `docs/adr/0390-what-part-10g-decided.md`, `docs/adr/index.md:206`. **What.** The number is
   ports-early's and 10d's (section above), and the report's "0346 stays unused" is not true on ports-early.
   **Scenario.** Whichever merges second has two ADR-0390 files and two index rows; citations of "ADR-0390" in code
   comments (`grants.rs`, `ports/Makefile`) become ambiguous. **Fix.** Renumber 10g's ADR in the reserved range
   after the architect allocates, and correct the report's sentence on 0346.

4. **warning (should-fix).** `crates/zoraya-posix/src/jobs.rs:1204` (`fork_moves_into`). **What.** The `Own` arm of
   `fork` is untested: changed to `clone_conn` (a full-rights connection for the child), every test of
   `cargo test -p zoraya-posix` passes. INV-0390-1 and INV-0341-6 are `checked:` by that command, and the new test
   covers `inherit` (spawn) only. **Scenario.** A refactor of `fork_moves_into` drops the arm into the
   `Directory | Tmp` pattern; a forked child of git (10h) writes `~/.config/git/config`; nothing fails. **Fix.** A
   test that forks with an `Own` entry and asserts the move's connection reads and cannot write, as the spawn test
   does. Put the mutation in a list under `tools/mutants/`.

5. **warning (should-fix).** `crates/zoraya-posix/src/jobs.rs:1013-1021` and `1148-1157`. **What.** ADR-0341 item 7
   says the child "never holds a handle that can write". An open file description inside the own directory is passed
   to the child with `share_rights(access)` on spawn and on `fork`, so a file the parent opened for writing without
   `O_CLOEXEC` stays writable in the child. **Scenario.** A program with a `data` line holds
   `~/.config/git/config.lock` open read-write and starts a hook (10h's git does both); the hook writes the
   configuration through fd N. Not reachable with ssh as built (it starts no child). **Fix.** Either lower to read a
   description whose object was opened through an `Own` entry (the description would need to remember that), or
   record it as a limit in ADR-0390 and INV-0341-6's Status before 10h relies on it.

6. **warning (should-fix).** `crates/zoraya-shell/src/netgrant.rs:337-349` (`reach_port`). **What.** The planner
   takes the **last** `-p`; OpenSSH takes the **first** (`ssh.c:990-998`: `if (options.port == -1)`). **Scenario.**
   `ssh -p 22 -p 2222 -l ada host`: `grants` shows `net: host:2222`, the program is given TCP to 2222, ssh connects to
   22 and is refused. It fails closed, but `grants` states a grant the program does not use, and the run's refusal
   report names a port the person never meant. **Fix.** Take the first occurrence (or refuse a repeated `-p` as a
   usage mistake, and `-l` with it, which OpenSSH also takes first). Add the case to
   `ssh_reaches_the_host_it_names_on_port_22_or_the_ports_option`.

7. **warning (should-fix).** `user/describe/posix/ssh.cmd:9-13`, `ssh-keygen.cmd:6-10`. **What.** Every option has a
   long name (the format requires one), so the shell accepts `--port`, `--login`, `--tty`, `--no-tty`,
   `--verbose`, `--type`, `--bits`, `--comment`, `--remove`, `--quiet`, plans grants from them and passes them as
   typed. OpenSSH parses with `getopt("TVtv" "l:p:")`: `--port` is the unknown option `-`, and ssh prints its usage
   and exits 255 (ssh-keygen the same). **Scenario.** `help ssh` lists `-p --port PORT`; `ssh --port 2222 --login ada
   host` is granted `host:2222` by the shell and dies with a usage message. **Fix.** Have the shell write the short
   form for these two descriptions (a description key or a per-program rule), or allow a short-only option in the
   description format (an ADR-0156 change, so a proposed ADR); at least say it in both help texts.

8. **warning (should-fix).** `docs/adr/invariants.md`, rows INV-0340-1, -2, INV-0341-4, -5, -6. **What.** Each is
   `checked:` while a named half of its check is not built, and ADR-0340's and ADR-0341's open questions say nothing
   (CLAUDE.md rule on ADR-0001 rule 6). Two halves are cheap now: INV-0340-1's configuration file (put
   `/data/home/ssh/config` and `/etc/ssh/ssh_config` with `Host *` / `Hostname 10.0.2.99` / `Port 1` into
   `apps-ssh-root` in `tools/test-apps-ssh.sh` and keep the script as it is: the login to B still has to work) and
   INV-0341-5's other `shell.home` (one `shell.home = /u` line in the test root's settings and one more `ssh-keygen`).
   The lints of INV-0340-2 and INV-0341-4 run in `make test-unit`, not in `make check` as both accepted ADRs say;
   ADR-0390 item 2 proposes that, so until it is accepted the Status should say "checked (partly)" rather than
   `checked:`. **Fix.** Run the two cheap halves; write the rule-6 note in ADR-0340 and ADR-0341 for the halves that
   wait (git's hook, the askpass prompt) and mark the rows partial.

9. **warning (should-fix).** ADR-0341 item 6, `crates/zoraya-panel/src/lib.rs:391`, `zosh.rs:4048-4072`
   (`apply_disk`). **What.** "Not built" is recorded (ADR-0390 item 10, the report), but its effect is not: the room's
   shell now holds `/data/ROOM` with tree rights, and `--disk` limits only the room's `+w` grants. **Scenario.** In a
   room made with `--disk 64M`, `+w /data/r cp big.img /data/r/x` (or any program's own files) writes outside the
   limit ADR-0265 item 5 sets, which no room line could do before this part. **Fix.** Record the bypass in
   `docs/design.md`'s known limitations and as an open question in ADR-0341; until the limit is built, refuse a line
   in a `--disk` room that names `/data/ROOM` with a write and give no own directory there, or say why not.

10. **warning (should-fix).** `docs/design.md`. **What.** Not changed. It does not say that programs have own
    directories, that `EntryKind::Own` exists, that `ssh` and `ssh-keygen` are in the apps' `/bin`, that a terminal's
    shell holds `/data/ROOM`, or that a current directory at `/`, `/data`, `/data/ROOM` or inside an own directory
    now gives **every** described command (native `ls`, `find` included) nothing. **Fix.** Make the shell, ports and
    rooms sections true (CLAUDE.md: keep `docs/design.md` true).

11. **warning (should-fix).** `tools/test-sshd.sh:89-90`, `.github/ci/Dockerfile:24`, `.github/workflows/test.yml:386`.
    **What.** The CI half is unverified (no candidate run). On Ubuntu 24.04, an `sshd` started as root fails with
    "Missing privilege separation directory: /run/sshd" unless the directory exists, and neither the image nor the
    fallback step makes it (the package's `RuntimeDirectory` comes from systemd, which the container does not run).
    Not run here. **Scenario.** On CI (root), `tools/test-apps-ssh.sh` fails with "the test servers did not start";
    `test-apps` goes red on the first full run. **Fix.** In `test-sshd.sh`, `[ "$(id -u)" = 0 ] && mkdir -p
    /run/sshd`, or the same in the Dockerfile and the fallback step; run a candidate before the merge.

12. **warning (should-fix).** `tools/test-apps-ssh.sh:55-60`. **What.** The guard "server A saw no connection" greps
    `Connection`, which the readiness probe of `test-sshd.sh` (`wait_for`) always writes: the left-behind
    `sshd-a.log` has `Connection from 127.0.0.1 port 64911 ... kex_exchange_identification: Connection closed` before
    the guest's `[preauth]` line. The authentication patterns miss `Postponed publickey`, sshd's line for a public
    key offered without a signature. **Scenario.** If the guest's run stopped reaching A (a relay change), the log
    check still says ok; the script's own `REMOTE HOST IDENTIFICATION HAS CHANGED` is then the only thing that shows
    A was reached. **Fix.** Require a second `Connection from` (or a `[preauth]` line), and add `Postponed` to the
    refused patterns.

13. **note.** `crates/zoraya-command/src/lib.rs:180`, `zosh.rs:1874`. **What.** With a `rest` operand, a `--help`
    before the host makes the parse `Help`, and `userland_options` lets a `Help` line through unchecked:
    `ssh --help -o ProxyCommand=x -l ada host` reaches ssh. The patched `getopt` refuses `--help` and `-o` (exit 255)
    and the line has no network, so nothing happens; but the shell's half of INV-0340-2 ("refused before ssh runs")
    does not hold for it. **Fix.** For a description with `reach` or `data`, treat `Help` with other words as an
    option error, or say so in the row.

14. **note.** `crates/zoraya-shell/src/netgrant.rs:286-292`. **What.** `ssh://[USER@]HOST[:PORT]` is not a valid name,
    so the planner gives nothing ("the command says what is wrong with it"), but OpenSSH accepts it
    (`ssh.c:1099`, `parse_ssh_uri`) and connects to the URI's port. Fails closed without a grant; with a room's or a
    word's `+net host:2222` the URI's port and user are used though neither `-p` nor `-l` was written. **Fix.** Refuse
    `ssh://` in the planner as a usage mistake, or patch `parse_ssh_uri` out.

15. **note.** `tests/shell/files-write.script:16-19, 117, 128, 137-140, 185`, `storage-down.script:16`. **What.** The
    changed expectations follow ADR-0341 item 8 (accepted: at `/` the current directory gives nothing) and are
    legitimate; the `grants cp` line losing `/ read (current directory)` is that rule. Nothing now asserts what a
    bare `ls` at `/` does (the refusal and its hint): the steps were moved around the change rather than testing it.
    **Fix.** Keep one step `cd /`, `ls` with the expected refusal text.

16. **note.** `crates/zoraya-panel/src/lib.rs:391`. **What.** `/data/ROOM` also holds the room's panel applications'
    directories (`/data/ROOM/APP`, ADR-0245). A room's shell could not reach them before; now any line in the room can
    name them to read and write, and the first terminal's shell gains write to `/data/home/APP` (it had read through
    `+r /`). ADR-0341's tensions list ADR-0245 only for a shared name. **Fix.** For the architect: record it in
    ADR-0341's tensions (or ADR-0390), or give the shell a hold that excludes the application directories.

17. **note.** `docs/adr/0390-what-part-10g-decided.md` item 8, `docs/adr/index.md:206`. **What.** "three patches"; there
    are four (`0004-no-hard-links` was added later). The `getopt` string also lets `-V` through, which `ssh.cmd` does
    not offer (harmless). **Fix.** Correct the text.

18. **note.** `crates/zoraya-shell/src/grants.rs:246`. **What.** Removing `|| into_data` from the narrowing rule
    leaves every test green. **Fix.** A test that shows what it changes (a value operand naming a `+r` directory at
    `/`), or drop it if it is equivalent.

19. **note.** `ports/patches/openssh/0004-no-hard-links.patch` (`hostfile.c:729`). **What.** `link(known_hosts,
    .old)` becomes `rename`: between it and `rename(temp, known_hosts)` the file is absent, and if the second rename
    fails it stays absent, so the next connection asks as for a new host instead of refusing a changed key.
    `UpdateHostKeys` (on by default, ADR-0340 item 7) rewrites the file this way on ordinary connections. **Fix.**
    Copy to `.old` (read and write) and keep `known_hosts` in place until the temporary file replaces it.

20. **note.** `user/src/bin/zosh.rs` (`posix_own`), `grants::own_conflict`. **What.** Only the line's `+r`/`+w` words
    are checked against the view path; a room grant `+w ~/.ssh` (when `shell.home` is `/home`) gives the table a room
    entry and the `Own` entry with the same prefix `/home/.ssh`. Which one `libposix` resolves to depends on table
    order. Not run. **Fix.** Check the room's grants in `own_conflict` too, or refuse the overlap when the room is made.

## Checked and found sound

- `rest` stops option reading once the required operands before it are in, `--` and `--help` after the host are
  the far side's; a `rest` operand must take several. `reach` names a one-word `host` operand and an option with a
  value; `is_data_path` allows `~/` and plain segments only. `EntryKind::Own = 5` is appended, `from_raw(6)` and a
  bootstrap with kind 6 are refused, `is_directory` covers it.
- The planner: an own directory only for an image description whose program the names list gives the name;
  `/data/ROOM/NAME` per room and name; `HOME` on the line refused; `+r`/`+w` on the view path refused;
  `cwd_reaches_data` for `/`, `/data`, `/data/ROOM`, inside a listed own directory, not for `/data2`, `/database` or an
  application's directory; a value inside such a current directory refused. `reach`: TCP to the typed host on 22 or
  `-p`, public answers only for a name, the host after the last `@` (as `ssh.c` takes it), a value host refused unless
  a `+net` word covers it on that port, no grant for a bad port or without the image's description.
- `libposix`: a spawned child gets a `READ | CLONE` connection and the kind stays `Own`; a link resolved outside an
  `Own` entry into it is refused (the one link-following site, `open_resolved`, also serves `parent`); `chdir`'s
  connection is read-only, so `fork`'s cwd clone does not widen.
- The patches: every configuration read goes through `process_config_files`, which returns first; `allow_askpass` is
  never set and `ssh_askpass` returns at once, `notify_start` starts nothing; the agent socket is never read;
  `getopt` holds only `TVtvl:p:`; no second option pass after the host; a missing user is fatal. `HOME` is
  `/home`, the passwd entry's directory.
- The panel's hold is not in the record's entries (no pill); `enter_room`'s held room leaves `/data/NAME` out; room
  names cannot be `home`.
- The guest is `restrict=on` with only the two relays to `127.0.0.1:PA` and `:PB`; the servers listen on the
  loopback only, accept one throwaway key for one user, no password; `perl alarm` survives the `exec`.
- `make apps` refuses `sshd` and the other tools in the image and a `listen`/`accept` export (INV-0340-4).
- Invariants: 816 rows, Total 816, INV-0390-1 and -2 counted.
