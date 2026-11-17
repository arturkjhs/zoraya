# TASK-31 part 10g: third check (the second review's findings against the fix pass)

Reviewer: an agent (Opus), not the author. `git diff 405b51a5 origin/work/10g` (fixes 1d22eb26, eb1479e9, eab46d12)
against `TASK-31-review-10g-2.md` and the architect's rule in ADR-0462. Run at `eab46d12`: `make check` exit 0; `cargo
test -p zoraya-shell -p zoraya-panel -p zoraya-rng -p zoraya-settings` exit 0. No QEMU run (the dispatcher's).

## The second review's findings

1. **Fixed.** `ShareLauncher` (`zoraya-rng/src/lib.rs:304`, `:422`) only from a depth-0 non-launcher; `rngd.rs:142-164`
   records `Conn::launcher()`; `init.rs:793` gives it to the panel, so panel shells are depth 1; counts fall on close
   (`rngd.rs:99-105`). Test `zoraya-rng/src/tests.rs:338`.
2. **Fixed.** The room's words no longer join `line.nets` (`zosh.rs:1069-1092`); `netgrant.rs:139-145` takes them as the
   bound only; `cat x` gets `None` (`netgrant_tests.rs:1088`); at the account limit `posix_net` refuses with a hint
   (`zosh.rs:1165`; `netd` answers `OVER_QUOTA`, `zoraya-net/src/service.rs:493`). See new findings 1 and 2.
3. **Fixed.** No `ADR-0394` is left; 0462 is in TASK-31's reserved 0460 to 0469 (`TASK-31-report.md:29`), unique on main.
4. **Fixed** in `net_for` for both shells (`netgrant.rs:252-275`); the module comment is left (new finding 4).
5. **Fixed.** `own_conflict` refuses a covering `+w`, line or room (`grants.rs:896`); test `grants_tests.rs:1361`.
6. **Fixed.** `panel_connection` is `REACH | NARROW` (`init.rs:444`).
7. **Fixed (partly).** `tools/test-map.txt` sends the room script (`:121`, longest prefix), `panel.rs`, `zoraya-panel`,
   `init.rs`, `gui.rs`, `zoraya-settings`, `netgrant.rs`, `zoraya-rng`, `rngd.rs` to `test-apps`. The bad-word filter and
   `words_scope` stay in `panel.rs:509-521` with no host test. `user/src/random.rs` has no line (the disposition says
   it has; it runs every step, which is safe).
8. **Fixed as far as an agent may** (`invariants.md:620` says "partly"; the new text is queued for the architect).
9. **Fixed.** Said once and dropped in `enter_room` (`zosh.rs:3985`, a window's `--room` too); skipped by `net_for` and
   the panel; tested (`h:0`, `netgrant_tests.rs:1088`).
10. **Fixed.** `word_within` by the rules (`netgrant.rs:301`, `zosh.rs:4503`); test `netgrant_tests.rs:1211`.
11. **Done** (`TASK-31-part-10g.md`). 12. **Done** (`tools/test-sshd.sh:12-14`).
13. **Fixed in the shim, not in the ADR**: ADR-0393 line 59 still lists `socketpair` (new finding 5).
14. **Fixed.** ADR-0462 rewritten (Depends on, Options B and C with costs, Consequences); invariants Total 867.

## The architect's rule (ADR-0462)

Holds for reach: a line's every rule (`+net`, host or URL operand, web rule) must lie in a room word, else refused
naming them (`netgrant.rs:252-275`); no request, no connection and no account; a hint at the limit. Not for status (1).

## New findings

1. **warning.** `crates/zoraya-shell/src/netgrant.rs:278`: `needs net-status` (`user/describe/commands/net.cmd:7`) gets
   `STATUS` after the bound, which checks reach rules only. In the first terminal after `room r` (`init`'s root, ADR-0462
   item 6) `net status` gets the status in any room, also one with no words, against item 5 (no status in a room's
   terminal). In a window it fails at `narrow` with the misleading "+net words bound what a line may reach". **Fix:** in
   a room refuse a status request with item 5's wording; host test.
2. **warning.** `user/src/bin/zosh.rs:1145-1197`: the rule's fourth clause (refused with a hint at the limit) has no test
   of any kind. **Fix:** move the status-to-message choice into `zoraya-shell` with a host test, or add a QEMU step that
   holds four carved accounts and expects the refusal.
3. **warning.** `docs/design.md:2390-2391` still says the words "are also on each line as the person's own" (refused
   Option B); the index row of ADR-0462 lacks the bound and the added Depends on (0262, 0288, 0435, 0453). **Fix:** say
   that the words bound a line and give it nothing; align the row.
4. **note.** `crates/zoraya-shell/src/netgrant.rs:9-10` still says the room's words "are not built".
5. **note.** `docs/adr/0393-what-part-10g-decided.md:59`: drop `socketpair` from the names `libc.a` lacks.
## Trust boundary, checked and sound

No program output reaches the status row or the grant line: the new texts are shell refusals and words from
`/config/rooms`. Every panel terminal gets a random connection (`panel.rs:535`, a share of the launcher's), with or
without words. The test sshd is unchanged apart from its header (`ListenAddress 127.0.0.1`, `PermitUserRC no`,
`DisableForwarding yes`, `PermitTunnel no`, `ForceCommand`, no password or PAM); the guest is `restrict=on` with two
`guestfwd` relays (`tools/test-apps-ssh.sh:78-90`).

**Verdict:** all 3 must-fix and the 6 should-fix items are fixed (7 partly); none was fixed wrongly. Before merging,
fix new findings 1 to 3 or record them for the architect.
