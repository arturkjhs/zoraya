# ADR-0466: The `BIND` right: the file service makes a socket node only through an own-directory or `/tmp` connection

- **Status:** proposed
- **Date:** 2026-11-19
- **Depends on:** ADR-0055, ADR-0153, ADR-0341, ADR-0393, ADR-0412, ADR-0439, ADR-0465
- **Evidence:** CP-01, CP-02, DM-039

## Context

ADR-0465 (accepted 2026-11-19) lets `bind(AF_UNIX, PATH)` succeed in a program's own directory (`Own` entry) or its line's
`/tmp` (`Tmp` entry), and made `fsd` keep socket nodes on every service it serves. Its item 7 and the review of the part
(`docs/tasks/TASK-31-review-own-sockets.md`, finding 3) record the consequence: the place rule is enforced by `libposix`
(`unix::bind_place`), which labels its own table entries. The service makes a node wherever a connection with `CREATE` asks. A
program that does not use `libposix` and holds write on a directory can plant a socket node there and listen, and that is
the shared rendezvous that ADR-0465 rejected (Option C). The architect's decision of 2026-11-19 (ADR-0465, "Architect's
decision") asks for the error to be closed: a `BIND` right carried only by the own-directory and `/tmp` connections and
checked by the file service when a socket node is made.

**Known:** the rights word of the file protocol is a `u32` of bits (`READ` to `CONNECT`, ADR-0055, ADR-0465); a connection's
rights only shrink (`clone`, `share`, `narrow`, `open` each need `has(requested)`); `rights::ALL` is what the shell, `libposix`
and the tests ask for when they mean "every ordinary right". **Assumed:** the shell is trusted to ask for `BIND` for exactly two
connections (ADR-0341, ADR-0153); a program holds only what it was given.

## Decision drivers

1. **No ambient authority** (CP-01): a program that has write on a directory has no way to make a socket node there.
2. **The service decides, not the client** (DM-039: the name is not the object; the table in a program's memory is no boundary).
3. **Rights only shrink**: nothing a program holds can be turned into `BIND`.
4. **Small and reversible**: one bit, one check in the service, no change to the transport or to ADR-0439's listener channel (CP-02: descriptors stay handles).

## Considered options

### Option A: a `BIND` bit that `ALL` does not contain, held by service roots, asked for by the shell for two connections (the recommendation)

`rights::BIND = 1 << 9`. `rights::ALL` stays `(1 << 9) - 1`, so every request for `ALL` (a `+w` grant, `libposix`'s
`share_conn`, `create-tree`) never names it. `rights::WITH_BIND = ALL | BIND` is the mask of the protocol's `Rights` type
and what a service's root connection holds. The service refuses `open_with_mode` of `S_IFSOCK` with `CREATE` unless the
connection used holds `BIND`.

### Option B: a flag on the connection the shell opens (not a right)

A boolean on the service's connection record, set by a new request. It needs a new message and does not shrink by itself on
clone, narrow and share: each of them must copy or drop it. Rejected: a right is the form the protocol has, and `has()` makes
every derivation shrink with no further code.

### Option C: the service learns the directory's path and refuses socket nodes outside `/data/ROOM/NAME` and `/tmp`

Needs a path for each directory record (the service sees ids, not paths), and the `/tmp` service has its own layout.
Rejected as larger; and it would tie the service to the shell's layout of the volume.

### Option D: as Linux has it

A socket node anywhere a program may write. Rejected: ADR-0465 driver 1, and the review's finding 3.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The right.** `rights::BIND = 1 << 9` in `zoraya-proto-file`. `rights::ALL` does not contain it (ADR-0465 said `ALL` grows
   with `CONNECT`; with `BIND` it does not, on purpose). `rights::WITH_BIND = ALL | BIND`; the protocol's `Rights` type is
   `Flags<{ WITH_BIND }>`, so a request may name it and any other bit above is refused as before. A change of the ABI of the
   rights word, in a bit that was unused.
2. **The check.** `open_with_mode` that creates (`CREATE` or `UNNAMED` in its flags) a node of type `S_IFSOCK` needs `BIND` on the
   connection it is made through, in addition to `CREATE` (it joins the rights the request needs, so a narrowed connection needs it
   for the name, too). A refused request makes nothing and counts nothing against the table of 256 nodes (the right is checked before the table's bound). A node does not leave the place that made it either: `rename` (also `exchange`) of a socket node, or of a directory that may hold one, into another directory needs `BIND` on the target's connection, and so does `publish` of an unnamed node. The same check
   covers every service `fsd` serves (ADR-0465 item 2).
3. **Who holds it.** A service's root connection (`add_root`) holds `WITH_BIND`. Every other connection has what its
   parent gave: a `clone`, `share`, `narrow`, `narrow_names` or `open` that asks for `BIND` of a connection without it is
   `ACCESS_DENIED`; one that does not ask has none, whatever its parent holds. A child of a program with an own directory holds
   `READ | CLONE` there (ADR-0341 item 7), so it has none.
4. **Who asks for it.** The shell (`zosh`), for two connections only: the program's own directory (`own_connection`), and
   the line's `/tmp` (`line_tmp`). The trusted hand-overs of a root that lead to those two (`init` to the shell and to the
   panel, the broker to the first program's `/tmp`, the write entries of a window's *shell* record (`namespace::record` with `bind`; never the record of an application such as `files`), `posixrun`'s test `/tmp`, which it takes from any path its caller names) ask for it as
   they pass the root on (`user::files::MASTER` is `WITH_BIND`). The children of a line keep the `/tmp` of the line with it
   (`libposix`'s `clone_tmp`), as they kept every right of it before; a child's own directory is read only, as before. No `+w`,
   `+own`, `+sock`, `create-tree` or file operand asks for it. A shell that does not hold `BIND` gives the directory without it:
   the program then cannot bind in it.
5. **`libposix` keeps its rule** (`unix::bind_place`: `Tmp` or `Own`, a name inside it): it gives the right errno without a round
   trip; the service is now the boundary.
6. **What it does not do.** `BIND` is on a connection, not a path: a connection to the own directory with `BIND` can make a
   socket node anywhere below it, and a program holding it can bind in its whole own tree and its `/tmp`. The 256 nodes and
   64 listeners remain the volume's (ADR-0465 item 7; per-room counts are F11). A node that exists is used as ADR-0465 says.

### Effect of the vision

A socket node is something a place is given the right to hold, and the person's shell gives that right to the two places a
program is meant to be a server in. A program that was handed a writable folder was handed a folder, not a rendezvous.

## Consequences

- **Positive.** The place rule of `bind` is a rule of the service, so it holds for a program without `libposix`; a connection
  that a program narrows, clones or passes on cannot gain it; ADR-0465's known limit (item 7, first half) is closed.
- **Negative.** A new bit in the ABI of the file protocol (`WITH_BIND` as the mask, a second constant to know besides `ALL`);
  every trusted hand-over of a root between `init` and the shell must pass it on (a hand-over that asks for `ALL` silently
  gives a shell that cannot bind in its own directories: the check `bind_is_named_only_where_the_adr_says` lists the
  places); a test shell without it is refused at the service.
- **Follow-up.** Per-room counts of nodes and listeners (F11); `readdir` with the node kind.

## Invariants

- **INV-0466-1.** The file service makes a socket node (`open_with_mode` of `S_IFSOCK` with a create) only through a connection that holds `BIND`: with the right it makes the node; without it, in a writable directory, the request is `ACCESS_DENIED`, makes no node and uses no slot of the table. *Check:* `cargo test -p zoraya-fs-service --test socket` (`a_socket_node_needs_bind_on_the_connection_that_makes_it`, `a_subdirectory_binds_only_if_opened_with_bind`, `a_socket_node_does_not_leave_the_places_with_bind`) and, from a raw-call program with no `libposix`, `socket_nodes_need_bind` in `t_storage` (run by the kernel test `storage_from_a_process`, in `make test-qemu`).
- **INV-0466-2.** `BIND` is never gained: `rights::ALL` does not contain it, and a `clone`, `share`, `narrow` or `open` of a connection without it that asks for it is `ACCESS_DENIED`; a clone or narrowing of a connection with it that does not ask for it has none; an own-directory child (`READ` and `CLONE` only) has none. *Check:* `cargo test -p zoraya-proto-file --test bind_sites` (`ordinary_rights_do_not_contain_bind`), `cargo test -p zoraya-fs-service --test socket` (`bind_is_never_gained_by_cloning_sharing_or_narrowing`, `bind_is_not_gained_by_share_narrow_names_or_open`) and `t_storage`'s `socket_nodes_need_bind`.
- **INV-0466-3.** `BIND` is named in the source at a short list of places only (the protocol, the service, the trusted hand-overs of a root, the shell's own-directory and `/tmp` connections, `libposix`'s `/tmp` clone, the tests); a new place is a change to the list and to this ADR. *Check:* `cargo test -p zoraya-proto-file --test bind_sites` (`bind_is_named_only_where_the_adr_says`).

## Open questions

- **For the architect.** Whether a connection with `BIND` should be limited to the node's own name when it is narrowed (it is: the
  narrowed name needs `BIND` for itself), and whether an own directory should refuse `BIND` below its first level; neither is done.
- The shell's `Held` entries of a window shell (a table of write entries) carry `BIND` when the root has it, so the shell can make
  an own directory below them; they hand it on only through `own_connection`. A second check that no `+w` grant carries it, from the
  QEMU side (a program given `+w` that tries a raw bind), is the `t_storage` case (a), at the service; the shell's part is the list of INV-0466-3.
- Per-room counts of nodes and listeners remain F11's (ADR-0465).

## Revisit triggers

- A second kind of node (a FIFO's creation) that needs a place rule: it could take its own bit or share this one.
- The service learns paths (Option C) for another reason.

## Architect's decision

(Left empty for the architect.)
