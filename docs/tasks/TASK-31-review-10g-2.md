# TASK-31 part 10g: second independent review (the fix pass, ADR-0462, closefrom)

Reviewer: an agent (Opus), not the author. Range: `git diff cbade396..origin/work/10g` (head `405b51a5`), read as
`git diff origin/main...origin/work/10g` for 10g's own lines (60 files), with the code around it. Read: CLAUDE.md,
TASK-31-stage-10.md (trust-boundary focus), TASK-31-part-10g.md, the first review and its dispositions, ADR-0172,
ADR-0224, ADR-0227, ADR-0262 (`netd` accounts as built), ADR-0288, ADR-0340, ADR-0341, ADR-0346, ADR-0393, ADR-0435,
ADR-0453 and ADR-0462.

Run here (a throwaway worktree at `405b51a5`):

- `make check`: exit 0.
- `cargo test -p zoraya-describe -p zoraya-command -p zoraya-shell -p zoraya-panel -p zoraya-abi -p zoraya-posix
  -p zoraya-settings -p zoraya-rng -p zoraya-rooms`: exit 0.
- Four hand mutations, each reverted: the `Own` arm of `fork_moves_into` (`jobs.rs:1318`) and of `inherit`
  (`jobs.rs:1208`) on `clone_conn`: `a_fork_gives_the_child_the_own_directory_to_read_only` and
  `a_child_gets_the_own_directory_to_read_and_never_to_write` fail (INV-0341-6, INV-0393-1 are true). `reach_port`
  back to the last `-p` (`netgrant.rs:377`): `ssh_reaches_the_host_it_names_on_port_22_or_the_ports_option` fails.
  `words_scope` with every word on any port: `a_rooms_net_words_make_the_scope_of_its_terminals` fails (INV-0340-1's
  and INV-0462-1's host halves).
- Two scratch tests, reverted (findings 1 and 2): a `zoraya-rng` test of `Conn` depths printed
  `panel-shell shares 8, its program may share: false` and `init-shell program may share: true`; a `netgrant` test
  printed `Some(1)` (REACH) for `cat x` and for `sleep x` with the room word `10.0.2.50:22` and no `+net` on the line.
- The OpenSSH series on the pinned archive (`openssh-10.6p2.tar.gz`, sha256 `9ae44872…d5b` as in `sources.lock`):
  `0001` to `0008` apply with `patch -p1` in order, no fuzz, no offset.
- No QEMU run.

## Findings

1. **error (must-fix).** `user/src/bin/init.rs:777-779`, `user/src/bin/panel.rs:526`, `crates/zoraya-rng/src/lib.rs:235-284`.
   **What.** `rngd` decides what a connection may `Share` by its depth (ADR-0288 item 6, ADR-0435): `init`'s is depth 0,
   the shell's depth 1 (64 live children each), a program's depth 2 (8), its children's depth 3 (none). `init` gives
   the panel `random::share(init's)` (depth 1) and the panel gives each terminal's shell a share of that, so **every
   shell the panel starts is at depth 2**: rngd treats it as a program. It may give at most 8 programs a random
   connection at a time, and every program it starts is at depth 3 and can give its own children none
   (`posix_spawn` with `POSIX_SPAWN_GRANT_RANDOM`, ADR-0435). The scratch test above shows both. **Scenario.** In a
   room's window (and now in every second terminal at home), the ninth live program (a pipeline of nine, or
   background jobs) starts with no random connection and OpenSSL stops with "PRNG is not seeded"; 10h's `git` in a
   room starts `ssh`, which gets no random connection and fails; ADR-0453 item 3 (accepted: the shell gives the
   broker a connection that may `Share`) cannot hold in a panel terminal, since the broker's share would be depth 3.
   `ssh-room.script` runs one program at a time and does not see it. INV-0462-2 ("a random connection of its own")
   is true, but that connection is not a shell's. **Fix.** Give the shells the panel starts a shell's place in the
   tree: for example `init` gives the panel a launcher connection whose shares are depth 1 (a `Share` flag only depth
   0 may set, or the panel's connection recorded at depth 0), or `init` makes each terminal's share on the panel's
   request. Either is a change to ADR-0288 item 6 and ADR-0435's table: state it in ADR-0462 (and its Depends on).
   Test it in `zoraya-rng` (a launcher's share is a shell) and in the window script (a pipeline of nine POSIX
   programs, each seeded; or a program that spawns a child with randomness).

2. **error (must-fix).** `user/src/bin/zosh.rs:1087-1094` (`net_grant_with`), `crates/zoraya-shell/src/netgrant.rs:135-150`,
   `crates/zoraya-net/src/accounts.rs:46-52`, `user/src/bin/zosh.rs:1155-1168` (`posix_net`). **What.** The room's
   `+net` words are appended to `line.nets` of *every* line in the room, and `net_for` gives a connection whenever
   `nets` is non-empty. So in a room with words every described command gets a network connection reaching the room's
   words, with no `+net` and no `host` or `url` operand: `cat`, `ls`, `sleep`, `vi`, `python3`, `make` (scratch test
   above: `Some(REACH)` for `cat x` and `sleep x`). ADR-0462 does not say this: its item 2 says the shell gets the
   words "to show and use them (a value host designates inside them)", and its INV-0462-1 is about the terminal's
   connection. It also costs what other rooms need: each POSIX program so given gets `narrow_account` with budget 2,
   and `netd` keeps at most **4 carved accounts at once, session-wide** (`MAX_CARVED`), and 8 TCP connections in all
   carved accounts (`COLUMN`). **Scenario.** In room `ops +net build.example.org:22`, `vi notes &`, `less log &`,
   `python3 &`, `sleep 600 &` (or four room windows with an editor open each) hold all four carved accounts; then
   `curl https://example.org/` or `ssh` in the first terminal at home is given no connection (`.ok()` drops the
   refusal) and fails at connect, with nothing saying why. One room's ordinary use takes another room's network. In
   an untrusted room (when the gate opens) every agent tool would get the room's network without a line asking.
   ADR-0172 point 3 does make a room's *file* grants stand on every line, and ADR-0224 item 4 says an untrusted room's
   programs get the room's words, so the behaviour may be wanted; it is a decision, and it is not recorded, not shown
   by `grants` as `(room NAME)`, and not bounded. **Fix.** Choose and record it in ADR-0462: either (a) the room's
   words bound designation only (a `host`/`url` operand or a line's `+net` must lie inside them) and a line without a
   designation or a `+net` gets no connection, so `ssh -l u build.example.org` still works; or (b) every line gets
   them, and then the connection is a plain `narrow` that carves no account (or the room's terminal shell's
   connection is itself one carved account the room's programs share), and `grants` shows `net: … (room NAME)`. Add
   a host test for the chosen rule (`cat FILE` in a room with a word) and a test that four room programs do not take
   home's network.

3. **error (must-fix).** 21 citations of `ADR-0394` (`git grep -n ADR-0394`: `crates/zoraya-panel/src/lib.rs:62,67,386,403,415`,
   `crates/zoraya-panel/src/tests.rs:94,850`, `crates/zoraya-settings/src/rooms.rs:23,329`,
   `crates/zoraya-settings/src/tests_rooms.rs:548`, `crates/zoraya-shell/src/netgrant.rs:261`,
   `crates/zoraya-shell/src/netgrant_tests.rs:1025`, `tests/gui/ssh-room.script:1`, `user/src/bin/init.rs:776`,
   `user/src/bin/panel.rs:301,304,504`, `user/src/bin/zosh.rs:789,1088,4454`, `user/src/gui.rs:448`) and the ADR is
   `docs/adr/0462-…`. **What.** No ADR-0394 exists on any origin branch, and 0394 is inside TASK-31's reserved range
   (0380 to 0399, `TASK-31-stage-10.md:19`); 0462 is outside it, and no reservation of 0462 for this task is in the
   task, the architect queue or the report. **Scenario.** The next part that takes 0394 from the reserved range (it is
   the next free number) makes every one of these comments cite the wrong decision; the first review's finding 3 was
   the same class. **Fix.** Ask the architect which number is 10g's; rename the file or the citations to match, and
   say in the report where the number came from.

4. **warning (should-fix).** `crates/zoraya-shell/src/netgrant.rs:9-10` and `135-245`, `user/src/bin/zosh.rs:3918-3980`
   (`enter_room`), `zosh.rs:1306-1328`. **What.** ADR-0224 item 2's room bound ("in a room, literal or not, an operand
   designates only inside the room's network words … refused before it runs") is not built: the module comment still
   says "the room's rules come with the rooms' words, which are not built". Two consequences now that rooms have words.
   (a) The first terminal's shell after `room ops` holds `init`'s root connection, so in "room ops" a line
   `+net any curl …` or `curl https://other.example/` gets what it asks; the room's words bound nothing there. (b) In a
   room's own window the panel's narrowed connection does bound it (`netd` refuses a wider `narrow`,
   `zoraya-net/src/connection.rs:154`), but the shell does not refuse the line: `narrow` fails, `.ok()` gives the
   program no connection, and it runs and fails at connect without the item 5 hint. **Scenario.** A person in
   `room ops` (first terminal) runs `wget http://192.0.2.10/x`, believing the room confines it; it is fetched.
   **Fix.** In `net_for`, when the line runs in a room, refuse every rule (line words, operand rules) not within one of
   the room's words, with the ADR-0224 wording; host tests for both shells; correct the module comment.

5. **warning (should-fix).** `crates/zoraya-shell/src/grants.rs:856-884` (`own_conflict`), `crates/zoraya-posix/src/jobs.rs:1205`.
   **What.** Only a grant at or *inside* the view path (`/home/.ssh`) is refused. A `+w` that *covers the real path*
   (`+w /data`, `+w /data/home`, or a room's `+w /data/ROOM`) is accepted, so the program holds a `Directory` entry
   over its own directory, and a child gets that entry by `clone_conn` with full rights. **Scenario.** In 10h,
   `+w /data/home git commit` (or a room made with `+w /data/r`): a hook that git starts writes
   `/data/home/git/config` through the inherited `/data/home` entry; ADR-0341 item 7 ("the child never holds a handle
   that can write, whatever its own runtime does") does not hold. With ssh as built nothing is started, so it is not
   reachable today. **Fix.** In `own_conflict`, refuse a write grant (line or room) that covers `own.real`, naming the
   own directory; or have `libposix` lower to read a child's `Directory` entry that covers an `Own` entry's object.
   Host test with `+w /data/home` and `+w /data`.

6. **warning (should-fix).** `user/src/bin/init.rs:432-439` and `780`. **What.** The panel gets `root_connection`,
   i.e. `rights::ALL` (`REACH | LISTEN | STATUS | NARROW`) on the root scope, though it only ever narrows to
   `REACH | NARROW` (ADR-0462 item 3 says no status or listen is given). The panel is the process that reads the
   launcher channels of every application. **Scenario.** A flaw in the panel's message handling gives listen and status
   on the whole network, which nothing the panel does needs. **Fix.** `init` narrows the panel's connection to
   `REACH | NARROW` on the root scope; record it in ADR-0462's Consequences.

7. **warning (should-fix).** `tools/test-map.txt:120` (`tests/gui/` → `test-gui`), `:102`, `:108`, `:125`, `:144`,
   `:236`, `:241`, `:275`. **What.** `tests/gui/ssh-room.script` runs in `test-apps` (`tools/test-apps-ssh.sh`) but
   maps to `test-gui`; `user/src/bin/panel.rs`, `crates/zoraya-panel/`, `user/src/bin/init.rs`, `user/src/gui.rs`,
   `crates/zoraya-settings/` and `crates/zoraya-shell/src/netgrant.rs` do not map to `test-apps`. The only check of
   INV-0462-1's "none when the room has none" is that script: the choice is made in `panel.rs:509-537`, which no host
   test reaches (`zoraya-panel`'s test only lists the slot). **Scenario.** A change of `panel.rs` that gives the root
   scope when `net_scope` is `None` passes `make test-changed` and the candidate run. **Fix.** Map
   `tests/gui/ssh-room.script` and those paths to `test-apps` too; move the "which scope for this launch" choice into
   `zoraya-panel` (a function of the `Launch`) with a host test that a room without words and home get none.

8. **warning (should-fix).** `docs/adr/invariants.md:620` (INV-0340-4). **What.** The row says `checked: make apps …;
   the half about libc.a exporting no listen or accept no longer holds`. An accepted ADR's invariant that is half
   false is not `checked:`; ADR-0001 rule 6 allows the open-question note, not a Status that records a violation as a
   pass. **Fix.** Status "checked (partly): …" and a proposed ADR (or the architect's decision) for the changed
   invariant text; the open-question note in ADR-0340 stays.

9. **warning (should-fix).** `crates/zoraya-settings/src/rooms.rs:338` (`is_net_word`), `user/src/bin/panel.rs:509-512`,
   `crates/zoraya-shell/src/netgrant.rs:139-149`. **What.** The reader takes any printable word after `+net` (`h:0`,
   `a..b`, `10.0.0.1/33`); only `room new` checks it with `parse_word`. For such a line the panel silently gives no
   network (`words_scope(..).ok().flatten()`), and the shell, which adds the words to every line (finding 2), refuses
   **every described command** in that room's windows with `+net h:0: the port is not a number from 1 to 65535`.
   **Scenario.** A hand edit of `/config/rooms` with a typo makes `ls` and `cat` fail in the room's every window, with
   a message about a word the person did not write on the line. **Fix.** Skip the line as ADR-0157 rule 5 says (bad
   line reported once) by checking the word's form in the reader, or have the shell report the bad room word once and
   drop it.

10. **note.** `user/src/bin/zosh.rs:4468-4479`. The "inside what this room holds" check of `room new … +net WORD` is
    string equality, not `within`: a room with `+net any` or `+net '*.example.org'` cannot give `api.example.org:22`,
    and case differs from the shell's lowering (`Build.example.org:22`). Fails closed. No host or QEMU test covers the
    refusal paths (`this shell holds no network`, `outside what this room holds`). Compare `parse_word(..).rule()`
    with `within`, and test it.

11. **note.** `docs/tasks/TASK-31-part-10g.md`, "Hand checks for the architect". (a) `ssh` is in `build/rootfs-apps`,
    which `tools/run-disk.sh` uses only if it exists: the steps need `make apps` (after `make fetch-ports`) first. (b)
    At home `ssh -l ada build.example.org` takes the name's *public* answers only (`netgrant.rs`, `Named::PublicName`);
    a host the architect owns on the LAN (a private address) is refused; the line must be
    `+net build.example.org:22 ssh -l ada build.example.org`, or the host's public address used. (c) The room part
    ("the same line, an empty directory `/data/r/ssh`") must be typed in a window opened in the room, not in the first
    terminal after `room r` (refused, ADR-0393 item 11), and needs `ssh-keygen` there and that key in
    `authorized_keys` too, since the room's directory is empty. (d) `make run-gui` has the Internet through SLIRP; the
    steps are safe (nothing listens in the guest, the key is the person's). Say these four in the report.

12. **note.** `tools/test-sshd.sh`, `tools/test-apps-ssh.sh:11-19`. Confinement is sound as far as read (below). Two
    leftovers: the key material is under `build/apps-sshd` and in `build/apps-ssh*-disk.img`, not a temporary
    directory, and stays after the run (the servers end, so the client key opens nothing afterwards); and `ForceCommand`
    is run through the test user's login shell, which reads that user's shell start-up files (`~/.bashrc` under
    Debian's sshd-aware bash, `~/.zshenv` on a Mac) and `/etc/ssh/sshrc` if present (`PermitUserRC no` covers
    `~/.ssh/rc` only). Neither is the guest's to choose. Say it in the script's header, or make the directory with
    `mktemp -d` and remove it in the `EXIT` trap.

13. **note.** `ports/apps/sshshim.c:30-36`. `libc.a` exports `socketpair` since the Unix sockets
    (`crates/zoraya-posix/src/ffi/sockets.rs:69`); the shim's ENOSYS `socketpair` comes first on the link line and
    shadows it. Unreachable as built (ProxyCommand), but INV-0393-2's "names `libc.a` lacks" and the file's header are
    no longer true for it. Remove it from the shim.

14. **note.** `docs/adr/0462-…md`. Completeness: *Depends on* lacks ADR-0262 (the accounts the room's programs use),
    ADR-0288 and ADR-0435 (the random depths, finding 1), ADR-0172 point 3 and ADR-0453; Options B and C have no
    Description or Cost; Consequences omit the panel's full-rights root connection (finding 6), the words on every line
    (finding 2) and the `/system/tls` read hold, which the frame shows as a pill `+r /system/tls` of the room
    (`zoraya-panel/src/tests.rs:866`) though it is no room grant. The invariants table is consistent here (864 rows,
    Total 864), but `main` has gained INV-0463-1..3 since: the merge must recompute the Total (867).

## Checked and found sound

- **No words, no network.** A room without `+net` words and every home terminal: `words_scope(&[])` is `None`,
  `Give::Net` yields `None`, the slot stays empty (`panel.rs:527-537`); an untrusted room's line with a word is
  rejected by the reader (`rooms.rs:551-553`) and `room new` refuses it (the untrusted gate's markers pass in
  `make check`). The scope is exactly the words (`words_scope` = `encode` of the words' rules; a word that does not fit
  gives none), rights `REACH | NARROW`, no `LISTEN` or `STATUS`; `netd` refuses any `narrow` wider than its parent
  (`connection.rs:154`), so the room's window cannot exceed its words by any line.
- **No room reaches another's.** Each terminal's network and random connections are made fresh per launch; the
  panel's slots 11 and 12 are moved only to the panel; the shell's `--net` argument only labels and designates, the
  authority is the connection at slot 9. The random service is one generator whose outputs reveal nothing of other
  connections; a room can only spend its time (no per-connection rate limit, availability only).
- **ssh's network at home** comes only from its `HOST` operand (`reach`, port 22 or the first `-p`), a `+net` word, or
  now the room's words; `ssh://` and long options are refused; `--help` with other words is refused.
- **closefrom** (`0008`): with `HAVE_PROC_PID` undefined and no `F_CLOSEM`/libproc in the cross headers, `closefrom` is
  `closefrom_fallback`: `close(fd)` for every fd from `lowfd` to `sysconf(_SC_OPEN_MAX)`, which `libposix` answers
  with `OPEN_MAX` (`ffi.rs:2682`), the fd table's bound (`fd.rs`), so every fd is closed and the loop is bounded;
  `EBADF` is ignored and `close` is rightly not retried on `EINTR`. The patch is in the OpenSSH tree only; `libc.a`
  and the tier lists are unchanged.
- **The test servers**: loopback only, one throwaway key per run for one user, no password or keyboard method, PAM
  off, `DisableForwarding yes`, `PermitTunnel no`, `PermitUserRC no`, `X11Forwarding no`, a `ForceCommand` that only
  prints (subsystems included); the guest is `restrict=on` with the two `guestfwd` relays to `NET_HOST`
  (`10.0.2.50`) ports A and B; `perl alarm` ends a server nobody stopped; on CI a missing sshd fails the step.
- **Fix-pass items**: first `-p` wins; long options refused with the short form named; `ssh://` refused;
  `own_connection` makes levels below the deepest entry that opens; `own_conflict` takes the room's grants; the
  `Own` arms of spawn and fork are tested and the mutants listed (`tools/mutants/own-directory.toml`); the
  `files-write.script` refusal step; `known_hosts.old` by copy; INV-0340-1/-2, INV-0341-4/-5/-6 Status texts say what
  runs and what does not; ADR-0393 has no collision; one OpenSSH port, one `sshshim.c`; the series applies in order.
