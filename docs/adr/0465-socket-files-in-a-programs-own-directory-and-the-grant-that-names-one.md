# ADR-0465: Socket files in a program's own directory, and the grant that names one (`+sock`)

- **Status:** accepted
- **Date:** 2026-11-19
- **Depends on:** ADR-0107, ADR-0153, ADR-0341, ADR-0393, ADR-0396, ADR-0397, ADR-0412, ADR-0430, ADR-0439
- **Evidence:** CP-01, CP-02, DM-039

## Context

ADR-0412 item 5 (accepted) lets `bind(AF_UNIX, PATH)` succeed in a program's own directory (ADR-0341) or its line's `/tmp`.
ADR-0439 (accepted as built) made only `/tmp`, and its open question asked whether the own directory was wanted. The architect
decided on 2026-11-17 (`docs/architect-queue.md`) that socket files in a program's own directory are allowed, and that unlinking a
listening node follows the ordinary rules (write on its directory). Today two programs started by separate lines cannot meet over
a socket: a line's `/tmp` is its own, and a program's own directory is read by no other program.

What is needed: a program binds in its own directory; another reaches the file only with a grant that names it; no program that
was not given the name has any path to it. **Known:** `EntryKind::Own` exists (ADR-0393 item 1), so the bootstrap record needs no
new kind; a grant that is "a directory narrowed to one name" is built (`Access::ChangeName` and the others, `NarrowNames`), and
`connect` needs `WRITE` on the node only (ADR-0439 item 4); `fsd`'s service has socket nodes, enabled for the `/tmp` service only
(`enable_sockets`). **Assumed:** a program that offers a socket to another is a program the person installed with its own directory
(ADR-0396, ADR-0397), and the person names the socket file to the client, as they name any path.

## Decision drivers

1. **No ambient authority** (CP-01): a socket file is reached only through a table entry that names it. No path from a program that
   was not given it.
2. **A grant gives what its words say**: `connect` to one file, not the directory, not its siblings, not `unlink` or `rename`.
3. **Descriptors stay handles** (CP-02): nothing new in the transport; ADR-0439's listener channel and pair are unchanged.
4. **Small and reversible**: reuse the own-directory kind and the narrowed entry; one new word.
5. **A name is not the object** (DM-039): what ADR-0439 said of `/tmp` holds here, with the difference that only the owner can
   write the directory.

## Considered options

### Option A: `bind` in the `Own` entry, socket nodes on the volume's service, a `+sock PATH` word (the recommendation)

The own directory is served by the volume's `fsd`, which keeps socket nodes like the `/tmp` service does. A client names the file
with `+sock PATH`, typed on the line or saved on its record.

### Option B: a grant of the whole directory (`+r`/`+w` on the server's own directory)

No new word. But the own directory is never merged with a grant of the line (ADR-0341 item 3), a `+w` over it is refused for what
it would give a child (ADR-0393 item 13), and `+r` gives the server's files to the client. Rejected: it gives far more than a socket.

### Option C: a socket in a shared place all programs of a room may write (a `/run`)

Any program with that place could unlink and replace a server's name (ADR-0439's "a name authenticates nothing"). Rejected: driver 1.

### Option D: as Linux has it

A socket file anywhere a program may write, reached by any program that can open the path. Rejected: ambient authority.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **Where `bind` works** (settles ADR-0439 item 1's open question). In a table entry of kind `Tmp` or `Own`, with a name inside the
   entry; anywhere else `EACCES`, the entry's root `EINVAL`. `unix::bind_place` and `unix_bind` accept `Own`. The entry's own rights
   decide the rest: a child of a program with an own directory holds only `READ | CLONE` there (ADR-0341 item 7), so its `bind` is
   refused by the service. **Not different from ADR-0412 item 5**; INV-0412-3 is now checked for both places.
2. **A transient node in the volume's service.** `fsd` enables socket nodes on every service it serves, not on `/tmp`'s alone. The
   node is a file on the volume, created empty and exclusively; its `S_IFSOCK` type, its owner connection and its listener are the
   service's memory only (ADR-0439 items 2 and 3 unchanged, including the table of 256 nodes and 64 listeners). After the service or
   the machine stops, the file remains as an empty regular file: `connect` is `ECONNREFUSED`, `bind` of that name is `EADDRINUSE`
   until someone unlinks it, as a stale socket file is on Linux. A server unlinks its name first (the test program does).
3. **The grant: `+sock PATH`.** A word of a line, a literal token like `+net` (a value never makes one), with one path word. It gives
   the file's directory narrowed to the name, with the rights `CONNECT | CLONE` (a new `Access::Connect`, so that `grants` and the
   window's pill say `connect to NAME` and `+sock PATH`, not the directory). **`CONNECT` is a new right bit** of the file protocol
   (`1 << 8`, `rights::ALL` grows with it): `socket_connect` needs it instead of `WRITE`, so a grant that names a plain file by mistake
   (or a server that plants one) gives no read and no write of it. A `Tree` grant has it as it has every right; `listen` still needs
   `WRITE` on the binder's connection. A plan that gives the same directory another way (the current directory, a `+r`, a second
   `+sock`, a file operand) is refused (`connect_conflict`), since merging would widen the narrowed entry: one socket per directory
   per line. Through that entry a program can
   `connect` to that file and to nothing else: another name in the directory is refused, so is `unlink`, `rename` and a new `bind` there
   (no `REMOVE`, `RENAME` or `CREATE`). The shell gives it only if it holds the file to change (`not_given`), like any entry.
4. **Saved.** `install` and `programs grant` accept `+sock PATH` with `PATH` exactly `/data/ROOM/NAME/.../FILE`: plain parts, a name
   inside a directory under `/data/ROOM/NAME`, no `.` or `..`. At start `installed::apply` plans it like the typed word, if the shell
   holds it; otherwise it drops it with the line `saved +sock PATH is not held here`; never in an untrusted room. **This is a stated, narrow
   exception to ADR-0397 item 1 ("saving a file is refused")**: it is the one saved word that names a file, because what it gives is
   `connect` and nothing else. `ADR-0397` item 3 refuses a saved path inside another program's own directory for `+r` and `+w`; `+sock` is the
   one word that may name one, since its client has no other way to reach the server. For the architect.
5. **Unlinking and renaming a listening node** follow the ordinary rules (the architect, 2026-11-17): they need write on the node's
   directory. In an own directory only the owner (and the shell, which holds `/data/home`) has it; a `+sock` grant does not. The
   `/tmp` case stays as ADR-0439 says it. ADR-0439's concern (a name authenticates nothing) is therefore narrower here: only the
   owner of the directory and the person's shell can replace a name.
6. **The two views.** The server sees its directory at its view path (`~/.config/NAME/...`, ADR-0341 item 3); the client's
   entry is at the real path (`/data/ROOM/NAME/...`), so the two use different strings for one file. A port that hard-codes one
   path in both programs must be given the other by its arguments. **Known limit**; a view-path alias for the client's entry would
   be a change to ADR-0341's table and is not done.
7. **Known limits.** `bind`'s place rule (item 1) is enforced by `libposix`, which labels its own entries; the service makes a socket
   node wherever a connection with `CREATE` asks, now on the whole volume (before: the `/tmp` service). A program that does not use
   `libposix` and holds a writable directory can make a node there and listen: it holds that write already, and a client reaches the
   node only through a grant naming it or a `Tree` grant of that directory. Restricting socket creation to `/data/ROOM/NAME` needs the
   service to know a directory's path: for the architect. The service's 256 nodes and 64 listeners are now the volume's: one program (or one room) can fill them for
   every program of the machine (ADR-0442's bounds per room do not cover the volume service); `readdir` shows the node as a plain
   file; a stale node stays after a crash (item 2); a node is not removed when a listener goes (ADR-0439 item 2).

### Effect of the vision

A socket file in an own directory is a capability the owner offers by name and a person gives by typing its path; the program that
was not given it has no path to it. The person sees the grant in `grants` as `connect to NAME`.

## Consequences

- **Positive.** Two separately started programs meet over a socket without sharing `/tmp`; tmux's default place works for its server
  and for a client the person gives the path; the grant is visible and revocable (`programs revoke`).
- **Negative.** A new grant word and a saved-word exception; the volume's socket tables are shared by all rooms; stale files after a
  crash; two path strings for one file.
- **Follow-up.** A per-room count of nodes and listeners; a view-path alias for a client's entry; `readdir` with the node kind.

## Invariants

- **INV-0465-1.** `bind` makes a socket file only in a `Tmp` or `Own` entry with a name inside it; in a `Directory` entry it is refused, and the file a refused `bind` made is removed. *Check:* `cargo test -p zoraya-posix` (`unix::tests::bind_is_allowed_only_in_the_tmp_and_own_entries`, `tests/socketfile.rs`).
- **INV-0465-2.** A program reaches a socket file in another program's own directory only through an entry that names that file (`+sock`): with none it is refused, a grant to one file does not reach a sibling, and the grant cannot `unlink`, `rename` or `bind` in the directory. *Check:* `cargo test -p zoraya-posix --test socketfile` (`a_grant_to_one_socket_reaches_no_other_and_cannot_unlink_or_rename`) and `tests/posix/zosh-sockets.script` in QEMU (`make test-posix`).
- **INV-0465-3.** A `+sock` word is a literal word of the line (a value never makes one), is saved only for `/data/ROOM/NAME/.../FILE`, is applied only if the shell holds the file, and never in an untrusted room. *Check:* `cargo test -p zoraya-shell -p zoraya-settings` (`a_sock_word_is_a_literal_grant_with_one_path`, `a_saved_sock_word_applies_narrowed_and_only_if_held`, `a_sock_word_is_saved_only_for_a_file_in_a_program_directory`).

## Open questions

- **For the architect.** The exception to ADR-0397 item 1 (item 4), and the two path strings (item 6).
- Whether the volume's socket tables should be counted per room (item 7) before a second program offers sockets: a node whose
  binder has gone stays until unlinked, so a room can fill the 256 for every other; and whether the service should refuse socket
  creation outside `/data/ROOM/NAME` (item 7).
- A `BIND` right, given only on the own-directory and `/tmp` connections, would let the service enforce item 1 (the reviewer's suggestion, now that a right bit exists). Also: a socket named as a file operand (a `Change` or `Create` entry) used to connect with `WRITE` and no longer does; only `+sock`, a `Tree` grant and `/tmp` connect.
- A client that connects to a stale node gets `ECONNREFUSED`, the same as a node with no listener: no way to tell them apart.

## Revisit triggers

- A port that must use one path string for server and client (item 6).
- Channel endpoints become duplicable: a listening socket could be inherited (ADR-0439's trigger).

## Architect's decision

Accepted 2026-11-19. Decided by the architect; recorded by an agent at the architect's request.

Accepted with its stated limits, including the saved `+sock` word as a narrow exception to ADR-0397 item 1. The review's error (the place rule of `bind` is enforced only by `libposix`) is to be closed, not only recorded: a `BIND` right carried only by the own-directory and `/tmp` connections, checked by the file service when a socket node is made, so that a program without `libposix` cannot plant one elsewhere; a small part (`work/bind-right`) with a test from a raw-call program, before `work/rust-std`. The volume's shared 256 nodes and 64 listeners become per-room bounds in F11 (still queued, a gate condition).
