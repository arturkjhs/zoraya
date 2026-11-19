# TASK-31 part 10j: independent review (installed programs)

Reviewer: the `opus-review` subagent. Diff `fffd7e0f..97c83e3a` (branch `work/10j`), read with the code around it.
Governing: ADR-0396, ADR-0397, ADR-0399 (accepted), ADR-0394 (proposed, what 10j decided).

Run in this review: `cargo test -p zoraya-settings -p zoraya-shell -p zoraya-describe` (exit 0, every suite ok).
Read, not re-run: `build/t-install.log` (`PASS: 198 steps`, `zoraya-fsck` clean). The log is dated 01:51; the head
commit (02:01) changed `user/src/bin/zosh/apps.rs` and `tests/posix/zosh-install.script` again, so that log may not
be of the head.

## Findings

### 1. warning: an upgrade whose rename fails leaves neither package

`user/src/bin/zosh/apps.rs:321-329`. When the record already holds `NAME`, `install` runs `remove_tree(&apps_dir, &name)`
first, then `rename(.new-NAME, NAME)`. If the rename fails, the error path removes `.new-NAME` as well. The old package
is then gone, and the record still holds the old hash. `remove_tree` is best effort and reports nothing: if it leaves
one entry behind (a file it cannot remove, a full journal), the rename onto a directory that is not empty fails, and
both packages are lost. The comment at line 273 ("a package that fails half way leaves the old one"), ADR-0394 item 4
("a failure removes the partial copy and leaves the old one") and driver 4 all say otherwise.
*Scenario:* `install ./ed-2` over `ed`; the rename returns an error; `ed` says `not what was installed` and
`/apps/ed` does not exist. *Fix:* rename the old package to `/apps/.old-NAME` first, rename the new one into place,
and only then remove `.old-NAME`. On a failure, rename `.old-NAME` back. Test it with a host-side fake or a forced
error.

### 2. warning: the package, not the person, chooses the name, and so which installed program it replaces

`user/src/bin/zosh/apps.rs:210-257, 335-346`. `NAME` is the stem of the one `*.cmd` file in the package directory.
When the record already has that name, `taken` is skipped (`existing.is_some()`) and the install becomes an upgrade.
So any package whose description is `ed.cmd` replaces the installed `ed`. The person typed `install ./something`, not
`ed`, and is told only afterwards (`replaces ed`). With `--keep`, which is the habit for an upgrade, the new code also
inherits `ed`'s saved words (`+w ~/notes`, `+net ...`). ADR-0396 item 9 speaks of "`install NAME` again".
*Scenario:* `install ./downloaded-tool --keep`, where the tool's package carries `ed.cmd`; the person's `ed` is now
that code, with `ed`'s saved grants. *Fix:* replace an installed program only when the line names it (`install PATH
NAME`, or the package directory's own name equals `NAME`). Otherwise refuse with `ed is installed: install PATH ed
replaces it`. At the least, refuse `--keep` unless the typed name matches.

### 3. warning: room grants bypass ADR-0396 item 2's refusal of `+w /` and `+w /apps`

`user/src/bin/zosh.rs:1336-1342`. `refuse_apps_write` runs on the line's plan before `add_room_grants` adds the room's
entries, and `room new` accepts `+w /` or `+w /apps` (`is_grant_path("/")` is true, and `covers` is true for a root).
Every line in such a room then gives its program `Tree` write over `/apps` (reason `room NAME`). Item 2 says a line
that gives `+w` over `/apps` or `/` is refused, and this part has just changed `tests/shell/system-ro.script` because a
typed `+w /` is now refused.
*Scenario:* `room new work +w /`, `room work`, then `tbx w /apps/tbx/tbx` writes the installed program. Any program run
in that room can also rewrite another program's package, which is a denial of service until a reinstall.
*Fix:* run `refuse_apps_write` after `add_room_grants`, matching `reason` `room ...` as well as `+w`. Refuse `+w /`,
`+w /apps...` in `room new` / `room set`. Add a host test with a room line.

### 4. warning: ADR-0396 item 2's example refusal (a redirection or a designated write into a package) is not built, and the script relies on it

`crates/zoraya-shell/src/installed.rs:71-99` refuses only entries whose reason starts with `+w`. Writes into a package
through other routes are allowed: a redirection (`echo x > /apps/ed/ed`, the ADR's own example of the refusal), a
`create` or `remove` operand of a base-system command (`cp evil /apps/tbx/tbx`, `rm -r /apps/tbx`, `mkdir /apps/vi`),
or a `cwd write` description run with the cwd inside `/apps`. `tests/posix/zosh-install.script:183` uses
`echo x >> /apps/tbx/tbx.cmd` to simulate tampering, so it depends on the gap. INV-0396-1 holds as written ("unless
the person typed a word that names it"), and the launch hash catches the change. But item 2 and its example say these
lines are refused, and neither ADR-0394 nor the part report records that they are not.
*Fix:* apply the refusal to every plan entry and redirection target with write access at or below `/apps`, or record
the narrower reading in ADR-0394 (item 9) and the part report as a known limitation. Make the tamper step of the
script use a route that stays allowed (for example a `+w` over a copy, then `programs`).

### 5. warning: names from a glob count as typed words and can be saved

`crates/zoraya-shell/src/eval.rs:1266` gives each name a pattern expands to the mark of the pattern's word (`literal`
when the pattern was typed). `installed::literal_tokens` (`installed.rs:118-126`) accepts them. So
`programs grant ed +w ~/proj*` or `install pkg +w ~/*` saves one `+w` per directory that matches. Those names were
made by whatever wrote the home directory: a program holding `+w ~` can create `~/proj-x`, which then becomes a
standing grant. The line's own grant context refuses a pattern ("a grant names one path: a pattern is not one",
eval.rs:1249-1253). ADR-0397 item 2 says the saved words are the literal tokens of the typed line.
*Fix:* give a glob result a mark of its own, or have the builtin see the source word (a `Value` field that records
"from a pattern"). Refuse a saved word that came from one, as the grant context does. Add a test.

### 6. warning: INV-0397-1 is marked `checked`, but no test goes through the evaluator

`crates/zoraya-shell/src/installed_tests.rs:351-365` builds `Value`s by hand and calls `literal_tokens`. The
invariant's check asks for "a host test of the parser that feeds `install` and `programs grant` a word from a
variable, a substitution and a pipe". The new `Io.words` (`eval.rs:1842`) is never exercised on the host. Neither are
the marks it receives for a variable, `(...)`, a function parameter (which keeps the caller's mark), a glob (finding
5) or a `~` word. The in-QEMU script covers only `$w`. A change to how `Io.words` is filled would pass every host test.
*Fix:* a `zoraya-shell` host test that runs lines through `Shell` with a test `Host` whose `install` builtin records
`io.words`, one line for each source.

### 7. warning: `programs revoke --kill NAME` ends jobs of any command called NAME, installed or not

`user/src/bin/zosh/apps.rs:850-854, 885-918`. With `--kill` and no words, the record is not consulted
(`Ok((record_bytes.clone(), Vec::new()))`), so `NoSuchApp` is never returned. The instances are every job element
whose typed name is `NAME`. `programs revoke --kill vi` therefore kills the person's editor, and its unsaved work.
`programs revoke --kill ls` kills every `ls` job. INV-0399-2 says "the instances of `NAME`", which means an installed
program's. The same element match also counts a `shell.path` program that shadows an installed name (the path comes
first, ADR-0396 item 6).
*Fix:* refuse when `record.get(&name)` is `None`. Count as an instance only an element that was started as an
installed program, for example a flag set in `ready_from_volume` when `installed_now` matched. Test it in the host
instance list.

### 8. note: the saved word is not the typed word, and its meaning follows `shell.home`

`user/src/bin/zosh/apps.rs:122-134`. A typed `+w /home/notes` is stored as `+w ~/notes`, and every `~/...` word is
read again against the current `shell.home` at each launch. If `shell.home` changes, every saved grant now points at
another directory, without a word typed for it. `refused_place` still bounds where it can point. ADR-0397 driver 2
says the words are stored as typed. *Fix:* store absolute paths (show them with `~` when they are under home), or
keep the source text of the word. Record the choice in ADR-0394 item 5.

### 9. note: the description cache is not cleared by an upgrade, and `Host::description` does not check it against the record

`user/src/bin/zosh.rs:5690`, and `install_builtin` (it never touches `installed_cache`). After `install` replaces `ed`,
the evaluator's parse and completion use the old package's description until the next launch or `help ed`. It has no
authority (every kind is `text`), but it is not the checked copy ADR-0394 item 10 promises. *Fix:* in `install`,
`installed_cache.remove(&name)`. In `Host::description`, return the cache entry only when its hash equals the current
`installed_now`/record hash.

### 10. note: `programs`, `help` and completion re-hash whole programs on every call

`apps.rs:776-781` hashes every installed program for the table: up to 64 programs of up to 64 MiB, about 1 s each by
the part's own measurement. `installed_description` (`apps.rs:583-602`) re-hashes a changed program on every Tab and
every `programs NAME`, because a mismatch is never cached. *Fix:* cache (hash, file version) per name, or the
negative result for the session. ADR-0396 item 8 already names the (volume, object id, version) cache.

### 11. note: the launch check's soundness now rests on the file service not writing the object after `read_buffer`

`user/src/posix_child.rs:752-766`. The bytes hashed are the memory object, which the file service still holds
attached and writable. `image_create` copies the object (`kernel/src/process.rs` `image_from`) after the check. The
SAFETY comment says a stray write "would change the bytes `is_native` reads, not their validity". INV-0396-4 now also
depends on it. The service is trusted (ADR-0033), so this is sound, but the comment should state the new reliance.
Better, detach the object from the service before the check if the protocol allows it.

### 12. note: the line's `+net` words are ignored for an installed program, and a saved word wider than the shell's connection fails the launch

`user/src/bin/zosh.rs:1232-1234` returns the saved scope alone. ADR-0397 item 5 says "the line can add more". As
before for volume programs, a typed `+net` gives nothing, and nothing says so. When the saved scope is wider than the
shell's own connection, `narrow_room` refuses (`zosh.rs` `posix_net`) and the whole line fails. Item 5 asks for the
word to be dropped with `saved ... is not held here`. *Fix:* merge the line's words into the scope, and on
`narrow_room`'s refusal retry without the saved words, with the note. Or record both in ADR-0394 item 7.

### 13. note: a `#!` package is accepted, and nothing shows one runs or what is hashed for it

`crates/zoraya-settings/src/apps.rs:610` accepts `#!`. The launch hashes the script's bytes and hands an image of them
to `image_create`. No test installs a script. If a script runs through an interpreter that opens `/apps/NAME/NAME` by
path, which is how `zoraya-posix` `load_fd` runs one, the bytes interpreted are not the bytes checked. *Fix:* refuse
`#!` until a test shows the interpreter gets the checked bytes (for example as a memory-backed fd), or record the gap.

### 14. note: the hint after a refused write into a package suggests a line that is refused

`build/t-install.log`: `to allow it: +w /apps/tbx tbx w /apps/tbx/x`, which ADR-0396 item 2 refuses. *Fix:* no
`+w` hint for a path at or below `/apps`; say `installed programs are changed with install`.

### 15. note: `install --keep` silently drops typed words past 16

`apps.rs:347` `saved.truncate(MAX_WORDS)`: the old words are put first, so a typed word is what goes, with no line
saying so. `programs grant` refuses in the same case. *Fix:* refuse as `programs grant` does.

### 16. note: the record and the log are read whole before any cap, and a line put past 256 KiB is never read

`apps.rs:48-60, 82-85` use `read_all` with no bound. `apps::read` cuts at 256 KiB, but `put` appends after the end of
the whole file (`crates/zoraya-settings/src/apps.rs:432-437`), so a program added to an oversized record is written
but never found. Both need `+w /config` typed. *Fix:* read at most `MAX_SIZE + 1` bytes, and refuse `put` when the
file is over the cap.

### 17. note: `programs remove NAME --data` on a stray `/apps/NAME` can delete an image program's own directories

`apps.rs:1048-1095`. A stray directory (`mkdir /apps/vi`) passes as removable, because only `is_app_name` is checked.
`--data` then deletes `/data/*/vi`, the own directories of the image's `vi` if it has a `data` line. The person is
asked first. *Fix:* for a name without a record, refuse `--data`, or refuse names that `grants::data_names()` lists.

### 18. note: INV-0397-5 in a shell the room keeper starts rests on that shell not holding `/config`

`saved()` (`apps.rs:645-663`) uses `self.room` for "untrusted", and a shell started with a table has `room: None`
(`zosh.rs` `main`). Today such a shell cannot read `/config/apps`, so it finds no installed name. A room given
`+r /config +r /apps` would change that. Untrusted rooms do not run yet. *Fix:* when the start-up record says the room
is untrusted, set it in the shell, or skip installed names in any shell started with a table.

## Checked and found sound

- SHA-256: the padding for every `filled` (55, 56, 0), the 72-byte pad bound, the FIPS vectors and a million `a` in
  the tests, split independence. `identity`'s 8-byte length prefixes match on both sides (install
  `apps.rs:299-311, 478-485` and launch `sha256::identity`).
- Launch: the hash is of the object `image_create` copies, the description hashed is the one cached, a mismatch
  returns before any plan, net or own entry is made, a program by path gets no record or word (`installed_now` is
  keyed by the typed name and cleared by every lookup), and the lookup order makes image, builtin, function and path
  win.
- A hand-edited record: `refused_place` drops `/`, `/config`, `/apps`, `/data` at launch; `+net any` and words
  outside a room's `+net` are dropped by `saved_scope`; `+own` needs `~/`; the untrusted room applies nothing; saved
  entries open without following links (`File::open`), so a link at a saved path fails, not widens.
- Install's copy: links refused, depth, entries and bytes bounded, the program's length re-checked after the copy, the
  description written from the bytes linted, `.new-NAME` and every other non-name refused by `is_app_name` at lookup
  and remove. A directory opens with flags 0 (`zoraya-fs-service` `lib.rs:2255`), so the stray check works.
- The reader, the writers, the log and the table: no index or slice reachable by hostile bytes panics; `log_append`
  stays under 64 KiB and keeps whole lines; `put`, `remove` and `revoke` keep the other bytes, and a revoke of a word
  not saved changes nothing.
- `for_volume` resets every field of `Description` that carries authority; `volume_key_problem` fails closed where it
  and `parse` could differ.

## Verdict

Not ready to merge. Fix 1, 2, 3, 5 and 7 first, and add the evaluator-level test of 6. Then either build 4 or record it
in ADR-0394 and the part report. The notes may be fixed or recorded as known limitations.

## Dispositions (the implementation session)

| # | Disposition |
|---|---|
| 1 | Fixed: the old package is renamed to `.old-NAME`, the new one renamed in, the old put back if that fails. |
| 2 | Not changed; recorded in ADR-0394 item 12 (the output says `replaces`). |
| 3 | Fixed in the line: `refuse_apps_write` runs after the room's grants are added and looks at `room` entries too. `room new` accepting `+w /` is recorded. |
| 4 | Recorded in ADR-0394 item 12 (only a typed or room `+w` is refused). |
| 5 | Not fixed; recorded (a glob counts as typed for `programs grant`). |
| 6 | Not done: no host test goes through the evaluator for `Io.words`; the QEMU script covers a variable. Open. |
| 7 | Fixed: `programs revoke` checks the record first. |
| 8 to 18 | Notes, not acted on, except the stray directory check, which now opens with the directory flag (note 17 stays: the person is asked). |

`make check` exit 0 and `tests/posix/zosh-install.script` PASS (207 steps, HVF) after the fixes. No separate changed-lines check was run.
