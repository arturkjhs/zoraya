# TASK-31 review: own-directory sockets (`work/own-sockets`)

**Verdict: not ready to merge.** Three errors break the ADR-0465 promise "connect to one file and nothing else": `+sock` gives
the file's bytes, two narrowed entries merge into the whole directory, and the place rule for `bind` is now enforced only by
the client library.

Reviewed: `git diff origin/main...HEAD` (base `fe1feacd`, head `836524ea`). Read: ADR-0465 (proposed), ADR-0439, ADR-0412 item 5,
ADR-0341 items 3, 7 and 8, ADR-0393, ADR-0397. Also read the code the diff relies on: `crates/zoraya-fs-service/src/lib.rs` (socket
nodes, `open_with_mode`, `remove`, `rename`, `publish`, connection close), `crates/zoraya-fs/src/volume.rs` (id allocation,
`publish`), `crates/zoraya-shell/src/grants.rs` (`plan`, `through_links`, `by_path`, `covered`, `window_pills`), and the parts of
`user/src/bin/zosh.rs` that plan and apply saved words.

## What was run

- `cargo test -p zoraya-posix --test socketfile`: 26 passed.
- `cargo test -p zoraya-shell -p zoraya-settings`: all passed (306 and 69).
- Temporary probe tests, removed afterwards (`git checkout` of the two files). Their output is quoted in findings 1 to 4.
  - In `tests/socketfile.rs`: a `+sock`-rights grant on a plain file; opening the node; a forged `Own` kind on a shared directory.
  - In `grants_tests.rs`: two `+sock` words in one directory; `refuse_apps_write` on a `+sock` under `/apps`.
- QEMU was not run (as instructed). The `tests/posix/zosh-sockets.script` part of INV-0465-2 is unverified by this review.

## Findings

### 1. error: `+sock` gives read and write of the named file, not "connect" (ADR-0465 items 3 and 4)

**Where:** `crates/zoraya-shell/src/grants.rs:67` (`Access::ChangeName(_) | Access::Connect(_) => READ | WRITE | SYNC | CLONE`).
Related: `crates/zoraya-settings/src/apps.rs:288` (`sock_place`) and `crates/zoraya-shell/src/installed.rs:199`.

**What is wrong:** `Access::Connect` has exactly the rights of `ChangeName`, so a `+sock` entry is a `+w FILE` with a different
label. Nothing checks that the named file is a socket node, and that cannot be checked at all for a saved word applied while the
server is not running. The fs service does not refuse data operations on a socket node's id either. `libposix` refuses
`open` of the node (`ENXIO`), but a program that talks the protocol directly can open the node with `WRITE` and write to it.

**Scenario:** this was run. Owner `srv` writes `/data/home/srv/secret` ("private"). A client is given the `+sock` narrowing of
`secret` with `SOCK_RIGHTS`. It opens the file `O_RDWR`, reads `Ok("private")`, then writes `Ok(1)`.

The saved form makes this worse. `programs grant cl +sock /data/home/srv/secret.db` passes `check_word`, because `sock_place`
accepts any file under `/data/ROOM/NAME`. At every start the client then gets read and write on another program's private file.
ADR-0397 item 1 refuses that ("saving a file is refused"), and item 3 refuses it inside another program's own directory. ADR-0465
justifies the exception by "it gives `connect` and nothing else", and that is false. It is also false for a real socket. After
an `fsd` restart or a reboot the node is a plain empty file, and the server may later reuse the name for a data file. A raw client
can also fill the node file up to the server directory's quota.

**Fix:**
- Make "connect" a right of its own. Add a protocol right (for example `CONNECT`) that `socket_connect` accepts in place of
  `WRITE`, and have the service refuse every data operation on a socket node's id.
- Give `Access::Connect` only `CONNECT | CLONE`, plus whatever lookup needs.
- This changes the file protocol and ADR-0439 item 4. Until then, ADR-0465 item 4's exception cannot stand as written.

### 2. error: two narrowed entries merge into the whole directory, and `grants` still says "connect to"

**Where:** `crates/zoraya-shell/src/grants.rs:658-693` (`by_path`) and `:477-486` (`fallback` has no `Connect` arm, so it prints
`other.words()`). Also `window_pills` at `:632`.

**What is wrong:** entries on one path with different names merge with `name: None`. The result is the unnarrowed directory with
`READ | WRITE | SYNC | CLONE`. For `ChangeName` the fallback text says so ("read and change anything in"). For `Connect` it still
prints "connect to a".

**Scenario:** this was run. `+sock /data/home/srv/a +sock /data/home/srv/b echo` produced:
- merged: `Merged { path: "/data/home/srv", rights: 195, name: None, lines: [("connect to a", "+sock"), ("connect to b", "+sock")] }`
- the window pill: `+w /data/home/srv`
- `grants`: `/data/home/srv  connect to a (+sock)` and `connect to b (+sock)`

The client can read every file in the server's own directory and write all of them. The same happens with one `+sock` plus a
file operand in that directory (`+sock /data/home/srv/s cat /data/home/srv/log`), and with two saved `+sock` words, which gives
a saved whole-directory grant that ADR-0397 item 3 refuses.

**Fix:**
- In `plan` (or `by_path`), refuse a line where a `Connect` part shares its path with a part that names something else or names
  nothing, unless a covering grant the person typed already gives more.
- Give `fallback` an honest `Connect` arm.
- Test it: two `+sock` words in one directory, `+sock` plus an operand, and two saved words through `apply` and `merge`.

### 3. error: the place rule for `bind` (INV-0412-3, INV-0465-1) is now enforced only in the client

**Where:** `user/src/bin/fsd.rs:843` (`service.enable_sockets()` on every service) and `crates/zoraya-fs-service/src/lib.rs:1638`.
The only checks are `crates/zoraya-posix/src/unix.rs:521` and `unix_file.rs:120`.

**What is wrong:** before this part, the volume's service refused `S_IFSOCK` in `open_with_mode` (`mode & !MODE_MASK`), so on the
volume "only `/tmp`" held on the service side. Now any connection with `CREATE` on any directory of any service `fsd` serves can
make a socket node and listen on it. `EntryKind` lives in the program's own memory (its `libposix` table), so it is no boundary.
That is the shared rendezvous of Option C, which the ADR rejects ("any program with that place could unlink and replace a
server's name"). Here it works across lines and over time: the node lives as long as the volume's `fsd`.

**Scenario:** this was run. A process lists its `/data/home/proj` connection (a `+w` directory) as `EntryKind::Own` and binds and
listens on `/data/home/proj/s`. A second process with a plain `Directory` entry on that directory connects: `Ok(())`. In practice
a program given `+w ~/proj` today plants a listener there. A later client is pointed at a socket in `~/proj` (`TMUX_TMPDIR`,
`SSH_AUTH_SOCK`), or a third program with `+w ~/proj` unlinks a real server's name and binds its own: the client then connects to
the planted listener.

**Fix:** the service must know where a node may be made. Options:
- A right (for example `BIND`) that the shell gives only on the `Own` entry and the `/tmp` root and never through `+w`.
- A flag on the connection the shell opens for the own directory.

Either needs a line in ADR-0465. Until then, INV-0465-1's check (`cargo test -p zoraya-posix`) proves only the library's
behaviour; say so in the invariant.

### 4. warning: a typed `+sock` is not limited to programs' directories, and `/apps` is not refused

**Where:** `crates/zoraya-shell/src/grants.rs:536-550` (no `sock_place` check on the typed word) and
`crates/zoraya-shell/src/installed.rs:71` (`refuse_apps_write` looks at `Access::Tree` only).

**What is wrong:** typed, `+sock` accepts any path the shell holds. With finding 1 it is a `+w FILE` that `grants` shows as
"connect to". This was run: `refuse_apps_write` on the plan of `+sock /apps/x/image` returned `Ok(())`, so `+sock` lets a line
change an installed program's file, which ADR-0396 item 2 refuses for `+w`. `own_conflict` (ADR-0341 item 3) also ignores
`+sock`.

**Fix:** apply `apps::sock_place` to the joined typed path as well, which also keeps `/apps` and the person's files out, and add
`Connect` to `refuse_apps_write`. Test both.

### 5. warning: a link planted at the socket's name is followed, with the client getting the target

**Where:** `crates/zoraya-shell/src/grants.rs:528-550` (`Access::Connect` counts as "opens" in `through_links`).

**What is wrong:** the server program controls its own directory. It can replace `s` with a link to a file under the person's
current directory, such as `s -> /home/notes.txt` with the cwd `~`. The typed `+sock /data/home/srv/s` then becomes
`Connect("notes.txt")` on `/home`, which with finding 1 is read and write of the person's file, shown as "connect to notes.txt
(+sock, through a link at …)". The refusal path's hint also says `+w` for a `+sock`.

**Fix:** do not follow links for `Connect`. Refuse a `+sock` whose path is or passes through a link ("is a link, not a socket"),
and test it.

### 6. warning: the volume's node table is shared by every room, and nodes never expire

**Where:** `crates/zoraya-fs-service/src/lib.rs:1019-1023` (an owner's close only clears `owner`) and `:1646` (`MAX_SOCKET_NODES`).

**What is wrong:** a node whose binder closed stays in the table until its name is unlinked or `fsd` stops. On the volume, `fsd`
lives for the whole session. A program, including one in an untrusted room, that binds 256 names in its own directory (or
anywhere, because of finding 3) and exits refuses `bind` for every program of the machine (`LIMIT_REACHED`) until the shell
removes the files. ADR-0465 item 7 records the bound as known, but not the untrusted room. ADR-0268 and ADR-0442 gate what an
untrusted room may exhaust.

**Fix:** state it in ADR-0465 for the architect, or count nodes per account (`account_of(i)`) as listeners already are.

### 7. note: what is not covered by tests

- No host test shows that an own-directory child (`READ | CLONE`, ADR-0341 item 7) cannot `bind`, though ADR-0465 item 1 claims
  it. The service would refuse it (`CREATE` is missing), which was read in the code, not run.
- No test opens the node through the grant with the protocol directly (finding 1).
- `a_grant_to_one_socket_reaches_no_other_and_cannot_unlink_or_rename` accepts three errnos for the sibling (`EACCES`,
  `ECONNREFUSED` or `ENOENT`). Pin the one the resolver gives.

## Checked and found sound

- **Stale nodes and id reuse.** Object ids come from `next_id` and are reserved in batches through `id_limit`, which is persisted
  (`volume.rs:609`, `:325`), so an id is never reused, even across mounts. A node is entered only for an object this open created
  (`a.id >= first_new`). There are no hard links: `publish` refuses an object that already has a name (`volume.rs:1831`). Every
  path that removes a name calls `socket_unnamed`: `remove` (unlink and its rmdir fallback), `rename` Replace and `publish`
  Replace. `rmdir` needs an empty directory. A plain file therefore cannot be reported `S_IFSOCK`. After a restart the table is
  empty and the old node is a plain file.
- **Listening.** Only the connection that made the node may make it listen (`owner == Some(i)`), and the owner is cleared when that
  connection closes, so a reused connection slot gets nothing.
- **Narrowing.** A narrowed client cannot create, unlink, rename or bind (no `CREATE`, `REMOVE` or `RENAME`), and cannot reach a
  sibling. This was run in the branch's own tests.
- **Words of the line.** `+sock` is a literal token in the parser (`syntax.rs:716`), and from a variable it is a command word
  (`lang_tests`). A pattern that expands to more than one path is refused (`eval.rs:1640`).
- **Saved words.** A saved `+sock` is dropped when not held, dropped in an untrusted room, and not followed through links, since
  `add_saved` runs after `through_links`.
- **ADR edits.** The notes on ADR-0397 and ADR-0439 are under their Open questions sections (rule 6); no status was changed.

## Dispositions (implementation session, after the review)

1. **error, fixed.** `Access::Connect` now has `CONNECT | CLONE`, a new right bit (`rights::CONNECT = 1 << 8`) of the file protocol;
   `socket_connect` and `libposix`'s `connect` use it instead of `WRITE`. Tests: `a_connect_grant_reads_and_writes_nothing_and_a_read_only_own_cannot_bind`
   (a plain file given by `+sock` cannot be read or written), `listening_and_connecting_need_write` (WRITE alone does not connect).
2. **error, fixed.** `grants::connect_conflict` (called by `zosh`'s `table`) refuses a plan that gives a socket's directory another way
   (a second `+sock`, the current directory, `+r`, an operand). Tests: `a_socket_entry_is_refused_beside_another_of_its_directory` and the
   `zosh-sockets.script` line `is also given to this command`.
3. **error, recorded as a known limit (ADR-0465 item 7, open question for the architect).** The service cannot tell a directory's path, so
   it cannot restrict socket creation to `/data/ROOM/NAME`; the rule is `libposix`'s, as `/tmp`'s was (only that service had nodes). A program
   that skips `libposix` and holds a writable directory can make a node there; it holds that write already.
4. **warning, not changed.** A typed `+sock` is the person's word; with `CONNECT | CLONE` it reads and writes nothing wherever it points.
5. **warning, not changed.** A link planted at the socket's name makes `through_links` give the target; the entry holds `CONNECT | CLONE`
   only, so it gives no byte of the target.
6. **warning, recorded** (ADR-0465 item 7 and open questions): the volume's 256 nodes are shared by every room.
7. **note, fixed** by the test of item 1 (a `READ | CLONE` own entry cannot `bind`).
