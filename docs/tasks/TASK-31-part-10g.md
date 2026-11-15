# TASK-31 part 10g: a program's own directory, and ssh (report)

Branch `work/10g`, from `main` at `47f59d26`. Written by an agent (Sonnet). ADR numbers used: 0390 (proposed); 0346
(a surface block for ssh) is not needed and stays unused.

## Done

1. **The description keys.** `zoraya-describe`: `data NAME as PATH [read]`, `reach OPERAND tcp PORT [OPTION]` and the
   operand kind `rest`. `zoraya-command`: a `rest` operand ends option reading (also `--help` and `--`) after the
   operands before it.
2. **Own directory in the planner** (`crates/zoraya-shell/src/grants.rs`): `own_directory` (image description, listed
   program only), `cwd_reaches_data` (ADR-0341 item 8), `own_conflict`, `report_with` (both paths in `grants`),
   `end_of_options_at` (`--` before the host). `netgrant.rs`: `reach` gives TCP to the typed host on 22 or `-p`'s port.
3. **`zosh`**: makes `/data/ROOM/NAME`, gives it as `EntryKind::Own` at `/home/...`, sets `HOME=/home`, refuses a line
   that sets `HOME`, writes `--`, holds a program with `data` or `reach` to its description's options. The panel gives
   a terminal's shell `/data/home` or `/data/ROOM` to change, not shown as a pill; `namespace::record` makes missing
   levels. `ports/apps/etc/passwd` and `group` go into the apps root with ssh (ssh needs a user-database entry; only the test rootfs had them).
4. **`libposix`**: a child gets the own directory read-only (`clone_read_only`, spawn and fork), and a link outside it
   is not followed into it. `zoraya-abi`: `EntryKind::Own = 5`.
5. **The port**: OpenSSH 10.6p2 (`ports/sources.lock`, hash from the https download; no `.sha256` published, `.asc` not
   verified), four patches under `__zoraya__`, `ports/apps/sshshim.c` (19 names), `ssh.cmd`, `ssh-keygen.cmd`,
   `user/describe/data-names.txt`. `make apps` puts `ssh` and `ssh-keygen` in `/bin` and checks that no server or tool
   comes with them.
6. **Tests**: `tools/test-sshd.sh` (the host's sshd, two servers, throwaway keys), `tools/test-apps-ssh.sh`,
   `tests/shell/apps-ssh.script`, run by `make test-apps`. CI image and fallback step install `openssh-server`.
7. **Docs**: ADR-0393 (proposed), invariants rows and statuses (Total 816), ports README, dependencies.

## What was run

- `cargo test -p zoraya-describe -p zoraya-command -p zoraya-shell -p zoraya-panel -p zoraya-abi -p zoraya-posix`: pass.
- `make check`: exit 0 (the commit before the last; rerun at the head below).
- `make -f ports/Makefile apps`: builds both programs; `tools/test-apps-ssh.sh` by hand against the built image:
  `PASS: 102 steps` and both server-log checks ok (first connection with the question and `SSH_ASKPASS` on the line;
  second connection without it; words after the host sent to the far side; `-J`, `-F`, `-o` before the host refused;
  no user name refused; value host refused; changed key refused with status 255 and no authentication seen by the
  server; `ssh-keygen` in a room into `/data/keys/ssh`; `ssh-keygen -R`; `cat` of a named file in the own directory).
- `make test-changed` (twice; ports fetched with `tools/fetch-ports.sh`): every step ran green (boot, qemu, libposix,
  posix, posix-gui, tour-gui, tour-terminal, gui, net, apps with the ssh script, netsurf, cxx, fuzz and the rest) except
  `test-files` in the first runs, which failed on ADR-0341 item 8 and then passed on its own re-run: `files-write.script`
  and `storage-down.script` worked at `/` with the implicit current directory; they now name `/` (`ls /`) or give
  `+r /` (`find`, `ls | search`), and `grants cp` no longer lists the current directory. `test-python*` are skipped
  locally (too long). Not run: `FULL=1`, `test-par`, `make mutants`.

## Gaps and known limits

- The storage limit on `/data/ROOM` (ADR-0341 item 6) is **not built**: the creating shell holds no connection to
  `/data`. Needs the keeper or a `/data` hold (ADR-0393 item 10).
- `scp` and `sftp` (the follow-up) are not built.
- The INV-0340-1 test with a configuration file in `~/.ssh` and `/etc/ssh` is not run in QEMU; the patch removes the
  reader. INV-0341-5 is not run with another `shell.home`. INV-0341-6's git-hook script waits for 10h.
- The lint is a host test (`make test-unit`), not a `make check` step (a new step needs lane and CI files).
- `grants` words the network row of ssh as `(operand URL)`: `netgrant::describe` sees only the scope bytes.
- `/etc/passwd`'s only name is `user`; ssh needs `-l USER` or `USER@HOST` anyway.
- 10f will also change `zoraya-describe` and the planner: this part's change there is the keys above, the
  `Kind::Rest` arm, and `plan`'s `into_data` lines; rebase on whichever merges first.
- No `.asc` check of the OpenSSH archive; the hash is trust on first use (ADR-0109).

## Hand checks for the architect (a login to a real host; nothing here connected to one)

On the Mac mini, in a terminal window of `make run-gui` (or `make run-net` console) with a host you own, replacing
`ada` and `build.example.org`:

```
~> ssh-keygen -t ed25519
(press Enter for the file; type a passphrase twice or Enter twice)
~> ls /data/home/ssh
~> cat /data/home/ssh/id_ed25519.pub
(add that line to ~/.ssh/authorized_keys on the host)
~> grants ssh -l ada build.example.org
~> ssh -l ada build.example.org
(compare the fingerprint with the host's: ssh-keygen -l -f /etc/ssh/ssh_host_ed25519_key.pub there; answer yes)
ada@build:~$ vi notes.txt          (full-screen, drawn in the window)
ada@build:~$ exit
~> ssh -l ada build.example.org echo second    (no question)
```
Then on the host change its host key (or edit `/data/home/ssh/known_hosts` here) and run the login again: it is
refused with status 255 and nothing is sent; `ssh-keygen -R build.example.org` and connect again: it asks as for a new
host. In a room (`room new r +net build.example.org:22`): the same line, an empty directory `/data/r/ssh`.

## Proposed ADRs

- ADR-0393 (what 10g decided): recommends Option A (as built).
