# TASK-24 review of 7a

An independent review by a subagent that did not write the code (TASK-24, "Review, for every part"), of
`git diff main..work/7a` at `6728d49`: hostile-input totality of `zoraya-text`, determinism, the compositor's trust
boundary and `term`'s key buffering, `unsafe`, the runner and `run-qemu.sh`, the invariant statuses, ADR-0240. The
reviewer reproduced findings 1 to 3 with a scratch test crate in debug builds. Dispositions by the main model; the fixes
are in the commit after `6728d49`, and the check of the changed lines is below.

| # | Severity | Where | Finding | Disposition |
|---|---|---|---|---|
| 1 | error | `zoraya-text` `raster.rs` | `-scale(p.y)` overflows when `scale` truncates to `i32::MIN` (1,024 points of delta −32768, units per em 16, px 16). | Fixed: `scale` saturates; the y flip is `saturating_neg`. Test: `hostile_fonts_and_sizes_draw_inside_and_never_panic`. |
| 2 | error | `zoraya-text` `lib.rs` shaper | The pen saturates at `i32::MAX`, then `pen + ONE / 2` overflows (huge advances, or about 2 M characters of real text). | Fixed: saturating adds. Same test, with 8,000 characters of advance 65535 at units per em 16. |
| 3 | error | `zoraya-text` `lib.rs`, `raster.rs` | A large px overflows `metrics`, `render_box`, and lets one glyph's raster grow without bound. | Fixed: `MAX_PX` = 512; every entry point clamps px to `1..=512` (a larger size draws at 512); `metrics` saturates. Same test, px up to `u32::MAX`. Recorded in `docs/design.md`. |
| 4 | error | `invariants.md` INV-0162-1 | "checked" while 1 to 3 broke it; the fuzz used one string and small sizes. | Fixed by 1 to 3 and the new tests: hand-built hostile fonts (extreme deltas, units per em 16 and 16384, advance 65535, saturating composites, a huge ascent), sizes 0 to `u32::MAX`, long text, and seeded random strings over the real fonts (`random_strings_through_the_real_fonts_stay_inside`). The status stays. |
| 5 | warning | box glyph | Its height followed the font's ascent, unbounded (a 12 × 37,272 box, 450 MB in a full cache). | Fixed: `render_box` clamps both sides to `max_side(MAX_PX)`; the hostile test asserts it. |
| 6 | warning | `make check-tokens` | Missed suffixed, byte-separated and alpha colour literals and `Duration::from_secs`; matched sizes. | Fixed: a literal of six or eight hex digits without separators, with or without an integer suffix, and any `Duration::from_*`; a size or mask is exempted with `// not a colour`. Run by hand on each form the review listed. |
| 7 | warning | `term.rs` `send` | Cut a key's bytes at `PENDING_MAX` (half an escape sequence or letter). | Fixed: a key's bytes go whole or are dropped whole and counted. |
| 8 | warning | `term.rs` `flush` | A write refused for no memory left `WRITABLE` asserted, so the loop would spin; an encode failure was not counted. | Fixed: only `WouldBlock` waits for `WRITABLE`; another refusal drops the waiting keys and counts them; the encode case counts. |
| 9 | warning | `zoraya-fonts` test | Checked only the hard-coded list, not every file of `fonts/`. | Fixed: the test lists `fonts/` and compares it with the list and the table. |
| 10 | note | `zoraya_term::read` | "No match" read as `?`, a real candidate; a solid non-default cell reads as the cursor; homoglyph folding weakens "exactly". | Fixed the first: no match is U+FFFD, not a candidate. The second is documented on `read` (a blank cell with an SGR background and the cursor read alike; no test relies on either). The third is by design and recorded in the report and in INV-0162-4's test, which pins the twenty classes. |
| 11 | note | runner | The label went unchecked into a file name and the QMP command; both runs wrote the same shot. | Fixed: the label is made file-safe; a shot is `build/shots/NAME-LABEL.ppm`. |
| 12 | note | glyph cache | A miss with the first way empty and the second full evicted needlessly. | Fixed: a free way is used first. |
| 13 | note | `Target::draw` | A caller's short `Bitmap` would index past its coverage. | Fixed: the index is bounds-checked (`a_short_bitmap_draws_what_it_has`). |

Nothing to report, per the reviewer: no floating point outside tests; the compositor change is colours only and does
not touch the trust boundary; the one new `unsafe` block (`term.rs`'s surface mapping) has a true SAFETY comment;
`run-qemu.sh`'s stdin change is right; ADR-0240 fills a gap and changes no accepted decision. The reviewer asked
whether INV-0240-1 should read "checked" while its ADR is proposed: it is checked by a command that runs, which is
what the column records; the architect decides the ADR.

## Check of the changed lines

The main model reread every changed line against its finding; `cargo test -p zoraya-text -p zoraya-term -p
zoraya-fonts -p zoraya-test-runner`, clippy with `-D warnings` on them and on `zoraya-user`, and `make check-tokens`
on the tree and on a file of the review's literal forms pass. The reference digests of INV-0162-2 did not change:
nothing within the old ranges draws differently. No open error remains.
