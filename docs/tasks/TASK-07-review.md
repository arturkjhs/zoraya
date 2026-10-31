# TASK-07 independent review

- **Reviewed:** `main` at `2b08e0a24ed98b7365643a0ae1f1ee9e4c700c96`. TASK-07's code is `e5d6f21..117361a` (5 commits on `fs/impl`), merged by `2b08e0a`.
- **Reviewer:** independent subagent that did not write the code (TASK-07, "Independent review").
- **Date:** 2026-10-31.
- **Environment:** Apple Silicon Mac, macOS (Darwin 25.4.0), Rust 1.99.1 (pinned), QEMU via Homebrew. All builds, mutations and probes ran in fresh clones under the scratch directory. No commit, edit or branch change was made in the real repository. Its working tree is clean. During the review its HEAD was seen at `1708967` on `fs/review-fixes`: that moved independently of this review, and the review stays on `2b08e0a`.
- **Reviewer's own tests:** `work/review.rs` and the mutation lists `work/muts.py`, `work/muts2.py` in `.../scratchpad/review7/`. They are not repository files.

## make test from a fresh clone

```
git clone --branch main /Users/svitlana/workspace/os clone && cd clone && make test
```

**Exit status: 0** (wall time 121 s, fresh `target/`). `decoder_fuzz` took **60.12 s** of that, and 60.57 s when run alone, so half the run is one test binary.

Summary lines of the filesystem targets, verbatim, in log order:

```
tests/basic.rs         test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
tests/crash.rs         test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.82s
tests/decoder_fuzz.rs  test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 60.12s
tests/faults.rs        test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
tests/integrity.rs     test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.78s
tests/model.rs         test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
tests/regressions.rs   test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
tests/space_time.rs    test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s
tests/units.rs         test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
tests/workload.rs      test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
tests/fsck.rs          test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
tests/mkfs.rs          test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s
adr-check: 23 ADRs checked against INV-0001-1..5
adr-check: 0 errors, 32 warnings
[auto-default] PASS: 68 passed, 9 expected failures
[auto-smp4-2g] PASS: 68 passed, 9 expected failures
[auto-el2] PASS: 68 passed, 9 expected failures
[tcg-default] PASS: 68 passed, 9 expected failures
[tcg-smp4-gicv2] PASS: 68 passed, 9 expected failures
[tcg-el2] PASS: 68 passed, 9 expected failures
[tcg-smp4-gicv3] PASS: 68 passed, 9 expected failures
[boot-auto] PASS: 0 passed, 0 expected failures
[boot-tcg] PASS: 0 passed, 0 expected failures
make test: all checks passed
```

(The file names are added here; the `test result` text is verbatim.)

Numbers the tests print, run with `--nocapture`:

```
crash test: 3200 requests, 181 commits, 402 windows (402 with at most k = 12 writes, all subsets; largest 11 writes), 14420 crash images in 4.776610542s
permuted crash test: 4388 crash images
targeted crash test: 406 crash images
fault injection: 414 fault points, 366 runs, 332 reached Failed, reasons {"DeviceFlush", "DeviceWrite", "MetadataRead", "MetadataRead at mount"}
metadata corruption: 80 runs, 36 reached Failed
model test: 19200 requests, 6984 succeeded, 12216 failed as the model predicted
random fuzz: 10000 images, 9201 mounted, peak decoder state 97683 bytes, at most 0.7 device reads per block
field-level fuzz: 1185 fields x 8 transformations, 9288 images, 8577 mounted, peak decoder state 97885 bytes, at most 0.7 device reads per block
```

## Findings

| ID | Area | Finding | Severity | Suggested fix |
|---|---|---|---|---|
| R-01 | Mount, commit protocol | **A second crash can destroy a durable generation.** `Volume::mount` (`volume.rs:272-334`) treats the selected root as durable and issues no flush. If a crash leaves root `g+1` written but not flushed, the device may show it after reboot, and it is still not stable: ADR-0015 rule 3 says "Nothing else makes a write stable". The next commit (`volume.rs:1620-1644`) reuses blocks `g+1` freed, which `g` still needs. Before that commit's first flush, a second crash may drop the unstable root. Reproduced (`a_double_crash_loses_synced_data`): boot 2 mounted generation 4 (`g` was 3), events `W11 W12 W14 F W1 W5 F`, no flush before the writes. The second crash image selected generation 3; `fsck`: `tree node at block 14: checksum mismatch`; mount: `Corrupt`. Generation 3 had been made durable by a `sync` that returned. This breaks INV-0011-1 ("newest durable generation") and INV-0010-1 (`sync` prefix). Real path: a guest crash with the disk's or host's cache alive, then power loss. The harness never crashes twice (R-22), so `make test` cannot see it. `repair` has the same hole (`check.rs:815`). | error | On a read-write mount, flush before the first write, and in `repair` too. Tried in a clone: one `dev.flush()` at mount makes the reproduction pass, and `crash`, `integrity` and `faults` still pass (X10). Put the rule in ADR-0016 or a new proposed ADR, because ADR-0011 rule 4 is silent. Add a two-crash test. |
| R-02 | INV-0012-1 | **Removing the metadata checksum check fails no test in the whole suite.** Mutant X1 deletes `ptr.verifies` from `check_meta` (`format.rs:312-314`): `basic`, `crash`, `faults`, `integrity`, `model`, `regressions`, `space_time`, `units`, `workload` and `decoder_fuzz` all pass. `bit_flips_never_return_wrong_data` (`integrity.rs:126-153`) flips only 3 random bits per block, and most land where structure checks catch them. **The engine itself is correct:** flipping every bit of the tree node of a small image gave `0 same, 32768 detected, 0 WRONG BYTES`; the same test under the mutant gave `467 same, 32124 detected, 177 WRONG BYTES`. The status `checked` is false for metadata. | error | Add the exhaustive flip of one image's metadata blocks (2.3 s here), and make `faults`' `corrupt_metadata_is_fail_stop` depend on the checksum. |
| R-03 | INV-0012-4 | **The memory bound is not checked in any useful sense.** Mutants that remove the cache's eviction (`io.rs:94`) and leak 4 KiB on every block read both pass `decoder_fuzz`. Measured peak is 97,885 bytes; the bound is `MAX_DEPTH × BLOCK_SIZE + CACHE_CAP` + 64 KiB slack, about 1.67 MB (`lib.rs:55`, `decoder_fuzz.rs:64,97`), 17 times the peak. Images are 96 and 1,000 blocks, so an unbounded cache never reaches it. The assertion covers only the read-only path (`decoder_fuzz.rs:94-100`), although the invariant says "any image" and the row says `checked`. | error | Test with a tree many times larger than 16 nodes, assert the live node cache size directly, tighten `CACHE_CAP`, and measure the read-write path or state in ADR-0012's open questions why not. |
| R-04 | INV-0010-4 | **The check does not use the crash harness the invariant names.** `idle_volume_loses_nothing` discards the log (`crash.rs:338-339`: `let _ = (base, events);`) and mounts the final image, in which every write landed. Mutant X6, a commit from `tick` that skips the flush after the root record, passes all of `crash.rs`. That mutant loses the completed requests in a crash, which is exactly what the invariant forbids. The engine as written does flush (`volume.rs:1644`). | error | Run `check_ordering` and the crash images on that log, and assert the final window allows only the full prefix. |
| R-05 | INV-0010-3, INV-0012-5 | Fail-stop after a read-only request is untested. Mutant `M0010-3c`/X4 makes `note_meta_error` (`volume.rs:865-869`) drop the failure, so a metadata error on `open`, `read`, `stat` or `readdir` does not enter `Failed`. The whole suite passes. `faults.rs:100-105` asserts only that the request failed, and the later mutating request fails again by itself on a persistent fault. A one-time read fault leaves the mutant healthy. | imprecision | After an injected read fault on a read-only op, assert `failure().is_some()` and `StorageFailed` on the next mutation. |
| R-06 | INV-0012-2 | The lost-write test does not reach the header checks. With the metadata checksum **and** the header generation off (X2), or the checksum and the header address off (X3), `lost_and_misdirected_writes_are_detected` still passes. Under copy-on-write the "previous version" at an address is usually another kind of block or zeros, so other checks catch it. The two header checks alone are redundant with the CRC (`M0012-2b`, `-2c`), which is expected, but no test shows they work. | imprecision | Craft the lost write: restore an old, same-kind node under a pointer with the checksum of that old block, so only the generation or address can catch it. |
| R-07 | INV-0010-5 | `Space::overhead` can return 0 (`space.rs:93-95`, mutant X5) and every test passes: the 64-block reserve always covers bitmap and allocator blocks, so "every commit succeeds" is not tested against the accounting. | imprecision | Add a test that takes `free` to the exact limit, or assert the accounting directly. |
| R-08 | INV-0016-3, ADR-0016 item 3 | The test barely constrains the reserve. Freeing requests held to the full reserve (`M0016-3a`) and `DELETE_RESERVE = 2` (`M0016-3c`) both pass; only 0 fails, on one seed. Also, item 3 calls rename a request that "only frees space", and it is not: 461 renames to longer names took a volume to 2,039 of 2,048 blocks used, spending 55 of the 64 reserved blocks. I could not break the invariant itself: from that image all 1,023 names were removable, singly and in sequence, and `create` got `NoSpace` with no failure. | imprecision | Fill exactly to the refusal, then remove. Correct item 3's wording. |
| R-09 | INV-0016-4 row | `invariants.md:82` and ADR-0016 say the checker "runs on every image of the model, crash and fault tests". `faults.rs` has no call to `check` (grep). Mutant `M0016-4b` is caught there by the model, not the checker. | imprecision | Run the checker after fault runs, or correct the row. |
| R-10 | INV-0015-1 | Per-sector independence is not tested. Mutant `M0015-1d` picks one choice per write instead of per sector, and `test_device_crash_model` passes: it compares per-sector sets, not joint choices (`units.rs:548-570`). Rule 2 and the invariant say "independently". | imprecision | Assert that one image mixes choices within a write. |
| R-11 | `fsck --repair` | Repair deletes far more than the damage. One wrong kind byte in the entry for directory `d` (`entry "d" in 1 says File but object 2 is Directory`) gave `entries_removed: 3, objects_orphaned: 3`, then `clean=true`, files 0: the directory and both files are gone, though the inode showed the right kind. A named file with a stray orphan entry is also deleted (`check.rs:654-662`, and the mount's `free_orphans` frees it). `check.rs:22-26` and `design.md` say "put on the orphan list" and "fixes namespace damage"; only a code comment at `check.rs:614` says the second mount frees them. There is no lost+found. It needs CRC-valid damage. | imprecision | Correct the entry's kind from the inode, drop the orphan entry of a named object, and state plainly that unreachable objects are deleted. |
| R-12 | `check.rs:77-82` | `ProblemKind::Namespace` says "Repairable", but `ctime after the last assigned time`, `at or above the id limit` and bad link targets have no fix: repair prints zeros and exits 1. Both first two are one root field to raise. A low `id_limit` is harmful: mount does not compare it with the tree, and `create` handed out id 2, which was in use (crafted image). Also, the test comment "repair refuses" for a bad data block (`fsck.rs`) is untrue: `repair` reads no data, returns `Ok`, prints `repaired:`, and exit 1 comes from the re-check. | imprecision | Raise `id_limit` and `last_assigned` in repair; say what is unrepairable; do not print `repaired:` when nothing was done. |
| R-13 | `check.rs:39-42, 599-600` | "A crash during repair leaves the volume as it was or repaired" is false: allocator rebuild, namespace fixes (more than one commit past 1,024 dirty blocks) and orphan freeing are separate commits. And "the old allocator's blocks are marked reachable" fails when a bitmap is undecodable: `allocator()` returns at the first bad block (`check.rs:285-292`), so later bitmaps are unmarked and `fresh` may overwrite blocks of the current root. By reading, not reproduced. | imprecision | Say "a state after any repair step", and mark every pointer of a decodable allocator root first. |
| R-14 | ADR-0016 Context | "Each place in the code that depends on it cites 'proposed ADR-0016, item N'" is false. The source cites items 1, 3, 4 and 6 only. Items 2, 5, 7, 8, 9, 10 have no marker (`rg 'ADR-0016' crates`), although the task asks for "a clearly marked boundary". | imprecision | Add the markers. |
| R-15 | ADR-0013 open question | "The `abi` crate does not exist yet" has been false since the merge: `crates/zoraya-abi` came in with stage 2, without `Name` or `RelativePath`. INV-0013-1 names the `abi` constructor, and the `checked` status rests on that sentence. | imprecision | Reword to "has no `Name` yet" (rule 6 note, allowed). |
| R-16 | `crash.rs:21-23` | The header says that past `k` the set is "the empty set, the full set and `random_subsets`". The code enumerates every log prefix (`crash.rs:183-185`), and `design.md` says so correctly. | imprecision | Correct the header. |
| R-17 | ADR-0010 rule 4 | The `durable` request flag is absent from the API and recorded nowhere (not in ADR-0016 or Known limitations). The rule defines it as "a request followed by `sync`", so a client can get the effect, but it is a silent gap against TASK-07's "every operation the accepted ADRs require". | imprecision | Record it in ADR-0016, or add it. |
| R-18 | INV-0012-3, `fuzz/` | `decoder_fuzz` and the fuzz targets only mount and walk, so `tree.rs` insert, remove and rebalance on crafted trees are never fuzzed (`walk.rs`). I added that: 37,521 CRC-valid crafted images, 29,483 mounted, 311,571 successful mutating requests, repair on every fourth, with no panic or hang. So no bug, but the gap should be closed. | note | Add mutating requests to the fuzz. |
| R-19 | `test` support | `engine_state` allocates from a disk field (`state.rs:53`): my first fuzz run used it on a crafted image and aborted, `memory allocation of 9075430549656937835 bytes failed`. It is test support, used on valid images only; the engine has no such path. | note | Clamp it. |
| R-20 | INV-0015-2 | `permuting` can be a no-op and `crash.rs` still passes (`M0015-2`). The invariant holds by construction: each batch's window is enumerated by subset, independent of order. An engine depending on order inside a batch is caught (`P5p`), also without permutation (`P5`). | note | Accept, or say so in the row. |
| R-21 | `check_ordering` | The `freed_pending` branch (`crash.rs:402-406`) adds nothing. With the `gens` check deleted and deferred reuse dropped (X7), the branch does not fire, because the reuse comes before the root that frees the block; only the crash images catch it. INV-0011-2 is in effect checked by the INV-0011-1 test. | note | Remove it, or record freed blocks when they are freed. |
| R-22 | Crash harness scope | Limits not stated with the bound: one crash per image; no crash inside mount's orphan commit or inside repair; no crash states after a failed write or flush (ADR-0015 rule 6); no commit from the dirty threshold or the `NoSpace` retry; the largest random window is 11 writes, on 384-block volumes. Only the targeted test passes `k`. | note | State them next to the bound; R-01 is the case that mattered. |
| R-23 | Dead test code | `model.rs:25,62-65` collects `all_ids` and asserts nothing, under a comment claiming INV-0014-3. The check is real elsewhere (the model's ids are compared; `M0014-3a` is caught). | note | Delete it. |
| R-24 | Self-rename | `RenameMode::NoReplace` onto itself returns `Ok` (`volume.rs:1061-1063`) although the name exists. ADR-0016 item 10 covers rename onto itself without naming the mode; Linux's `RENAME_NOREPLACE` gives `EEXIST`, from my knowledge. By reading. | note | Decide in item 10. |
| R-25 | Id commit | The id reservation runs before the request is judged (`volume.rs:533-537`), so `open` with `create` of an existing name, or a request that then fails, can commit. Item 10 says that case "changes nothing". Nothing visible changes, but the device is written. By reading. | note | Say so in item 10. |
| R-26 | `fsck` | One bad copy of the newest root record gives `clean=true` (`valid_copies` 5 of 8 is not reported; `check.rs:530`). The volume has lost its margin and nobody is told. | note | Report it as a warning. |
| R-27 | `zoraya-mkfs` | A failed `--from` exits 1 and leaves a valid partial image that `fsck` calls clean (`--size 310K`: `src/a: NoSpace`, then `clean`). | note | Remove the image on failure. |
| R-28 | `is_within` | The bound is `block_count × 256` (`volume.rs:1031`). A parent cycle took 308 ms at 2^16 blocks, so about 26 s at the largest volume by scaling. It ends, and it is linear, but one crafted rename stalls a serial service for that long. | note | Use a visited set or a depth limit. |
| R-29 | Process | The merge came before this review, while TASK-07's Definition of done orders them the other way. ADR-0016 bundles ten decisions in one record, so the architect cannot accept them singly except as a changed detail. | note | Architect's call. |

**Counts:** 4 errors, 13 imprecisions, 12 notes.

## Item 2: invariants against their tests

"Caught" means the row's named test command failed under the mutant. All mutations ran in a clone. "Weak" and "false" are the findings above.

| Invariant | Test | Verdict | Mutations and results |
|---|---|---|---|
| INV-0010-1 | `crash.rs::every_crash_state_is_a_prefix`, `model.rs` | True | `sync` does nothing: caught by `crash`, not `model`, as expected. Directory size wrong: caught. |
| INV-0010-2 | `rename_over_and_max_write_are_all_or_nothing`, harness | True | Commit inside a write: caught. Commit inside rename: caught by the targeted test. |
| INV-0010-3 | `faults.rs::every_injected_fault_is_reported_and_sticky` | **Weak (R-05)** | `writable` ignores Failed, failure not sticky, report repeated, `sync` Ok when Failed: caught. Read path drops the error: **survives**. Supervisor part left for stage 4: ADR-0010 open question 6 says so. |
| INV-0010-4 | `idle_volume_loses_nothing` | **False as checked (R-04)** | Tick never commits, twice the deadline: caught. Tick commit with no last flush: **survives**. |
| INV-0010-5 | `full_volume_refuses_cleanly` | **Weak (R-07)** | No space check: caught. Overhead not counted: **survives**. |
| INV-0010-6 | `objects_without_names_vanish_after_a_crash` | True | Orphans not freed, unnamed not listed, unlink ignores handles: caught. |
| INV-0011-1 | `check_ordering` in `crash.rs` | True | Deferred reuse dropped: caught (`write to block 8, reachable from generation 1`). R-01 is outside the test. |
| INV-0011-2 | same | True, through INV-0011-1's check (R-21) | Same mutant caught; `freed_pending` itself is not effective. |
| INV-0011-3 | `check_ordering` | True | Each flush removed, root first, one batch, flush before data: caught. See item 4. |
| INV-0011-4 | `check` on every crash image | True | Bitmap keeps freed bits: caught. |
| INV-0011-5 | `root_record_selection` | True | Ring check off, root CRC off, lowest generation: caught. |
| INV-0011-6 | `idle_and_read_only_sequences_write_nothing` | True | Clean commit writes: caught. |
| INV-0012-1 | `bit_flips_never_return_wrong_data` | **False for metadata (R-02)** | Data checksum off: caught. Metadata checksum off: **survives the whole suite**. Engine correct by exhaustive flip. |
| INV-0012-2 | `lost_and_misdirected_writes_are_detected` | **Weak (R-06)** | All checksums off: caught (data). Header address or generation off: survive, redundant. Metadata CRC plus either: **survive**. |
| INV-0012-3 | `decoder_fuzz`, `regressions`, `#![forbid(unsafe_code)]` | True, scope R-18 | Overflow back in `root.rs`, height 0 accepted, unchecked index: all caught, with panics. |
| INV-0012-4 | `decoder_fuzz` | **False as a check (R-03)** | Unbounded cache: **survives**. 4 KiB leak per read: **survives**. |
| INV-0012-5 | `corrupt_metadata_is_fail_stop` | **Weak (R-05)** | Failure not recorded; mutate path does not fail: caught. Read path: **survives**. |
| INV-0013-1 | `name_boundaries`, `name_validator_matches_reference` | True | U+007F allowed: caught. Tested on `zoraya_fs::name`, not `abi`: ADR-0013 says so, wording stale (R-15). |
| INV-0013-2 | `names_compare_byte_for_byte` | True | ASCII case folding: caught. |
| INV-0013-3 | `link_objects_are_never_followed` | True | Link followed in a path: caught. |
| INV-0013-4 | `namespace_stays_a_tree` | True | Own subtree allowed, second name by publish: caught. |
| INV-0013-5 | `requests_reject_dot_and_absolute_paths` | True | `..` accepted: caught. No request decoder yet: ADR-0013 says so. |
| INV-0014-1 | `times_and_ids_across_crash_remounts`, `model.rs` | True | No `+1`, not restored, stale value in root: caught. |
| INV-0014-2 | `reads_change_nothing`, `content_changes_assign_new_times` | True | Directory `mtime` unset, read assigns a time: caught. |
| INV-0014-3 | `space_time.rs`, `model.rs` | True (R-23) | Ids restart below the limit: caught. |
| INV-0014-4 | `timestamps_round_trip` | True | Bad nanos accepted, 32-bit seconds: caught. |
| INV-0014-5 | `attribute_record_fields` | True | `owner` added: caught. |
| INV-0015-1 | `test_device_crash_model` | **Weak (R-10)** | No arbitrary values, stable not advanced, choice omitted: caught. Correlated sectors: **survives**. |
| INV-0015-2 | `crash_states_with_permuted_completion` | True by construction (R-20) | No-op permutation: survives. Order dependence in the engine: caught. |
| INV-0015-3 | `no_flush_means_read_only` | True | Flush not required: caught. |
| INV-0015-4 | `device_errors_are_typed` | True | Write error, flush error ignored: caught. Driver part left for stage 4: ADR-0015 open question says so. |
| INV-0016-1 | `times_and_ids_across_crash_remounts`, `lost_ids_are_not_reused` | True | No durable reservation: caught, `id 2050 handed out before`. |
| INV-0016-2 | `invalid_tree_root_of_the_newest_generation_fails_the_mount` | True | Tree root unchecked: caught. |
| INV-0016-3 | `full_volume_refuses_cleanly` | **Weak (R-08)** | Reserve 0: caught. Reserve 2, or reserve held against freeing requests: **survive**. |
| INV-0016-4 | checker | **Row inaccurate (R-09)** | Checker disabled and size wrong: caught by the model. No checker in `faults.rs`. |

No row in `invariants.md` itself says a part is left for stage 4: each just says `checked`. The partial ones are stated only in the ADR open questions, which I confirmed for INV-0010-3, INV-0013-1, INV-0013-5 and INV-0015-4.

## Item 3: on-disk parsing

`#![forbid(unsafe_code)]` is at `crates/zoraya-fs/src/lib.rs:20`, the crate root, so it covers `format.rs` as ADR-0012 asks. The crate has no `unsafe`.

Checked and found correct by reading each file, plus attack:
- `format.rs`: all reads go through `Rd`, which is bounds-checked. `count` is clamped by `NODE_CAPACITY / KEY_FIXED_LEN`, so `Vec::with_capacity` is bounded. The bitmap count equals `bitmap_count(block_count)`. Pointers are checked for address, generation and high half. Keys are validated by kind. Padding is checked.
- `root.rs`: no overflow on the generation; `block_count` is limited by the device, so `Space` and the checker's vectors come from device size, not a free disk field.
- `io.rs`, `tree.rs`: reused cache entries are checked against bounds and level. Recursion is bounded by height ≤ 8. Interior loops are bounded by node size. A block shared under disjoint ranges cannot multiply a walk, since a multi-entry node fits only one key range, so traversal stays linear.
- `space.rs`: the `alloc` scan is bounded and safe on bad bitmaps.
- `check.rs`: `level - 1` runs only on interior nodes, with level above 0.
- Engine source has no `unwrap`, `expect`, `panic!` or `assert!` except one `debug_assert_eq!` on a constant encoder length (`format.rs:374`).
- Attacks: crafted images with mutating requests (R-18), random garbage, an empty file and a truncated image through `zoraya-fsck`: exit 1, no panic. Parent cycle: ends with `Corrupt` (R-28).

Known limits: a crafted bitmap marking live blocks free is not compared with the tree at mount, so a commit can overwrite that volume's own blocks. That stays inside the volume, as ADR-0012 driver 4 allows. `fsck` memory grows with the volume's content, which I accept for a host tool.

## Item 4: crash enumeration against ADR-0015

- **Torn sectors (rule 2):** old, new, zeros or a seeded pattern per sector (`crash.rs:233-241`). Matches, within the finite form the ADR allows. Independence is real in the code, untested (R-10).
- **Unstable writes (rule 3):** every write in the window before its closing flush. Earlier windows are stable. Matches.
- **Flush (rule 4):** a window closes at a successful flush. Matches. Failed flushes are not enumerated (R-22).
- **Reordering:** subsets cover any order. Past `k`, all prefixes plus 64 random subsets.
- **One window covers every crash point in it: correct for device states.** A crash earlier in a window leaves a subset of that window's writes, and an unissued write is the "old" choice. The oracle's upper end is the window's end, which is looser than the crash instant, but the state only changes at the root record, whose requests have all completed. The claim is about **one** crash: it does not cover a device left with unstable writes when the next boot begins (R-01).

**Would a wrong commit protocol be caught? Yes, and without `check_ordering` too.** Each mutant ran twice: normally, and with `check_ordering` disabled so that only crash images could catch it.

| Mutant in `volume.rs` | With `check_ordering` | Crash images alone |
|---|---|---|
| P1 flush before the root removed | caught | caught |
| P2 root written first | caught | caught |
| P3 flush after the root removed | caught | caught (`not an allowed prefix`) |
| P4 deferred reuse dropped | caught, `write to block 8, reachable from generation 1` | caught, `fsck` on a subset of window 0 |
| P5 root and data in one batch | caught | caught; also by the permuted test alone |
| P6 one root copy | caught | caught, by a torn image at seed 7; only `every_crash_state_is_a_prefix` failed, so this one depends on the seeds |
| P7 flush before the data | caught | caught |

## Item 5: mkfs and fsck

- Without `--repair`, the image is opened read-only and every write fails in `FileDevice`. `damage_is_reported_and_not_modified` covers this.
- **A good image is not damaged:** `--repair` on a clean image returns before writing, and the SHA-1 was identical before and after. A read-only file with `--repair` exits 2.
- Repair refuses on damaged root records or tree nodes: random garbage and a truncated image both refused, image unchanged in size.
- Repair overreaches on deletion (R-11), leaves problems it calls repairable (R-12), and overstates crash behaviour (R-13). It also has R-01's missing flush.
- `mkfs`: built a tree with a file, symlink, hard link (copied) and subdirectory, `fsck` clean. Rejected too-small sizes, a name with a newline and a FIFO with clear messages. 25 GiB is capped at 5,483,712 blocks, as ADR-0016 says. A failed populate leaves an image (R-27).

## Item 6: changes to `docs/adr/*` and `docs/design.md`, `e5d6f21..117361a`

All are in commit `117361a`. **TASK-07's "Touch only" list contains none of these files.** But TASK-07's Rules order the proposed ADR, and CLAUDE.md says "Keep the Status column of `docs/adr/invariants.md` true" and "Keep `docs/design.md` true when what exists changes". Neither rule can be followed inside that list, so the list is incomplete, not the work out of bounds.

| Change | Allowed? | Rule quoted |
|---|---|---|
| ADR-0010 (accepted): open question 6, check status | **Yes** | ADR-0001 rule 6: "record in the ADR's open questions why it cannot yet be checked". CLAUDE.md: change an accepted ADR only in "the open-question note of rule 6". The note also says how clients are simulated, a little more than "why", still within the section. |
| ADR-0013 (accepted): open-question note | **Yes**, same rule. Wording stale since the merge (R-15). |
| ADR-0015 (accepted): open-question note | **Yes**, same rule. |
| ADR-0016: new, status `proposed`, "Architect's decision" empty | **Yes** | CLAUDE.md: "requires a new ADR with status `proposed`". ADR-0001 rule 2: "Agents create ADRs as `proposed`". Rule 4: "before the code is merged", met, written before the merge. |
| ADR-0016's number | **Yes** | TASK-07: "use a free number in 0010–0019". 0016 was the first free one. CLAUDE.md: "Use the ADR number the task reserves for you". |
| `index.md`: ADR-0016 row | **Yes** | INV-0001-5: "`index.md` lists every ADR file with its status". |
| `invariants.md`: 31 Status cells to `checked`, first four columns unchanged | **Allowed**; content false or weak for the rows in item 2 | CLAUDE.md: "Keep the Status column ... true: `checked: <command>` or `not yet applicable (stage N)`". ADR-0001 rule 6. The allowance is for true statuses: INV-0012-1, INV-0012-4 and INV-0010-4 are not (R-02, R-03, R-04). |
| `invariants.md`: INV-0016-1 to -4 rows, count row, total 105 | **Yes** | The file's header: "Every invariant from every ADR". Rule 6: "listed in `invariants.md`". The total is right. |
| `design.md`: Platform, external crates line | **Yes** | CLAUDE.md, "Keep `docs/design.md` true". |
| `design.md`: Architecture as decided, Filesystem paragraph | **Yes**, it restates what exists and adds no decision. |
| `design.md`: new "Filesystem engine (as built)" | **Yes**, same rule. |
| `design.md`: Tests, Known limitations, Where things are | **Yes**, same rule, and CLAUDE.md: "Record known limitations where they apply". |
| Vision section, stage plan | Not touched (item 7) | CLAUDE.md: they "belong to the architect". |

No ADR status changed. No "Architect's decision" section changed. No `### Decision` sub-heading was added. No deviation of an accepted ADR was edited in place; deviations are in ADR-0016 (items 2 and 6), except R-17.

## Item 7: `docs/design.md` across the merge

SHA-256 of each section, `##` heading through the line before the next `##`:

| Commit | Vision (89 bytes) | Roadmap (896 bytes) |
|---|---|---|
| `f716793` | `7f4913e9925d8415…493540f` | `f96f9bd853db9580…7b2d9b5` |
| `e5d6f21` | same | same |
| `117361a` | same | same |
| `5336195` (`kernel/stage-2` before the merge) | same | same |
| `2b08e0a` | same | same |

**Both sections are byte-identical across all five commits** (full hashes `7f4913e9925d84157b3e60898c5f11bd6b95c6d71631ffba5976d8f39493540f` and `f96f9bd853db9580be468def7075c65dedf8af2af6f447277197edec37b2d9b5`).

Neither parent lost content. The merge has 13 sections, every section of both parents and none new. Sections each parent changed differ from that parent as they must. Line by line against the merge base `e5d6f21`:

| Parent | Lines it added to `design.md` | Absent from `2b08e0a` |
|---|---|---|
| `5336195` | 58 | 0 |
| `117361a` | 65 | 0 |

`Threads and processes (as built)` from `5336195` and `Filesystem engine (as built)` from `117361a` are each byte-identical in the merge. In other merged files, the old total line of `invariants.md` and the prose of `dependencies.md` were reworded, as expected of a merge; I did not review those further.

## Checked and found correct

- Every invariant test exists and, apart from the rows marked in item 2, fails under a mutation of what it claims. In all, 88 mutants ran, none failed to apply.
- `make test` passes from a clean clone. `Cargo.lock` has no `source =` lines; `fuzz/` is excluded from the workspace, and its crates are listed in `docs/dependencies.md`.
- `allow(` appears twice, each with a comment.
- The commit protocol matches ADR-0011 rule 3 step by step (`volume.rs:1533-1660`); root records go to both copies of pair `g mod 4`.
- `select_root` matches rule 4, including torn pairs at the ring's end, which do not trigger the read-only path.
- Rollback from a snapshot restores tree, ids, times and accounting, and commits happen only between requests.
- Deletion of large files stays cheap at commit, since emptied leaves are dropped from the dirty count, so the reserve is not eaten by a large unlink. By reading.
- ADR-0016 has three options, a Linux baseline and non-Unix precedents; `adr-check` passes it. Its line count (about 5,100) matches `wc -l` (5,114).
- The first `regressions.rs` test fails without its fix (`M0012-3a`). I did not mutate the second.

## Could not verify

- **Coverage-guided fuzz results in `fuzz/README.md`.** No nightly here, so neither the targets nor the numbers were run or compiled. My stable-toolchain fuzz (R-18) is a partial substitute.
- **The Linux cloud session** in which TASK-07 ran, and whether the commits' identity `Claude <noreply@anthropic.com>` was the identity configured there. The repository's other recent commits are by `Artur`.
- **R-01 on a real device.** It follows from ADR-0015's contract and is shown on the test device's model; I did not reproduce it under QEMU, and there is no virtio driver yet.
- **Stability of the test timing** on a slow CI host; `make test` ran once. `decoder_fuzz`'s 60 s was on 10 CPUs.
- **`model.rs` and `ops.rs` of the test kit** were read only in part. I judged them by what the mutants showed.
- **ADR-0016's evidence** was checked by `adr-check` and spot checks of two citations, not as a full ADR-0001 rule 7 review.
- **R-13, R-24 and R-25** come from reading, as marked.

## Resolution

The implementing agent applied these after the review, on branch `fs/review-fixes`. The review above is unchanged, and it reviewed `2b08e0a`, not the branch. The rows say what was done, and "Open" means nothing was changed.

| ID | Resolution |
|---|---|
| R-01 | Fixed. A read-write mount flushes before it reads the roots. The review's reproduction is now `a_double_crash_keeps_synced_data` in `regressions.rs`, and it also asserts that the flush is the first event. Fails with the flush removed. |
| R-02 | Fixed. New test `every_metadata_bit_flip_is_detected` flips every bit of every tree node of a small image. Fails with the checksum check removed. |
| R-03 | Fixed. New test `large_tree_stays_within_the_decoder_bound` asserts the live cache size on a tree far larger than the cache, and the peak on a large image. Fails with eviction removed. The read-write path is not held to the bound, because it also holds the allocator bitmap, as the test's comment says. `CACHE_CAP` was not tightened. |
| R-04 | Fixed. `idle_volume_loses_nothing` now uses the log: `check_ordering`, and an empty last window, so only the full prefix survives. Fails with the commit's last flush removed. |
| R-05 | Fixed. New test `read_only_request_that_meets_a_read_fault_fails_the_volume`. Fails under X4. |
| R-06 | Fixed. New test `header_checks_stand_without_the_checksum` uses a valid checksum, so only the header can catch the block. Fails with the generation check removed. |
| R-07 | Open. `Space::overhead` is still not tested against the exact limit. The engine was not shown wrong. |
| R-08 | Partly. The wording is recorded in ADR-0016's open questions. The test still barely constrains the reserve. |
| R-09 | Partly. New test `volumes_left_by_faults_check_clean` runs the checker after write faults. The other fault runs do not call it. |
| R-10 | Fixed. New test `sectors_of_one_write_tear_independently`. Not run against `M0015-1d`. |
| R-11 | Fixed for both cases. Repair corrects an entry's kind from the object's own record, and removes a stray orphan entry of a named object. New test `repair_corrects_an_entry_kind_and_keeps_the_subtree`; the stray orphan case has no test. The header says plainly that unreachable objects are deleted. |
| R-12 | Partly. `ProblemKind` now says what has no fix. Repair still does not raise `id_limit` or `last_assigned`, mount does not compare `id_limit` with the tree, and `repaired:` still prints when nothing was done. |
| R-13 | Fixed: the comment states what a crash during repair leaves, and every bitmap pointer is marked before any is decoded. Not reproduced before or tested after. |
| R-14 | Fixed: items 2, 5, 7, 8, 9 and 10 now have markers. |
| R-15 | Open. The sentence the review quotes was not found in ADR-0013 by that wording, so nothing changed. To be checked by hand. |
| R-16 | Fixed. |
| R-17 | Recorded in ADR-0016's open questions. Not implemented. |
| R-18 | Open. The review's own run found no bug. Not added to the suite. |
| R-19 | Fixed: the test helper clamps the size. |
| R-20 | Accepted. |
| R-21 | Open. |
| R-22 | Fixed: the limits are in `crash.rs`'s header. |
| R-23 | Fixed. |
| R-24, R-25 | Recorded in ADR-0016's open questions. |
| R-26 | Open. |
| R-27 | Fixed: a failed `mkfs` removes the image. No test. |
| R-28 | Open: linear, and it ends. |
| R-29 | For the architect. |

