# TASK-34 review: part F8 (the 2 GiB guest: the frame allocator at 2 GiB, `--mem 2G`, `make run-browser`)

Independent review by the `opus-review` subagent of `git diff origin/main...work/f8` at 08326f74, 2026-11-16, read
against `docs/tasks/TASK-34-foundations.md`, `docs/research/browser-engine/plan.md` row F8, ADR-0415 (item 5, its
architect's decision and the open-question note), ADR-0270 (INV-0270-1 and its Q2, Q6 notes), ADR-0021, the proposed
ADR-0436, and the bug classes of `TASK-34-review-f2.md`, `-f3.md` and `-f4.md`. Code read around the diff:
`FrameAllocator::alloc_counted`, `alloc_contig_step`, `new` (`crates/zoraya-pmm/src/lib.rs`), `kernel/src/memory/`
(`frames.rs`, `paging.rs` `direct_map_ranges`, `mod.rs` layout, `heap.rs`), `kernel/src/arch/aarch64/boot.rs` (TTBR0),
`mod.rs` (`save_and_mask_interrupts`, `restore_interrupts`) and `exception.rs` (`exception_dispatch`),
`kernel/src/steps.rs` (`mod masked`), the test kernel's `hoard_frames`/`unhoard_one`, the `Makefile`'s `RUN_GUI_CMD`,
`run-browser` and `test-keeper`, `zoraya-dtb`'s `MAX_REGIONS`.

Tests run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to files:

- `make check`: exit 0 ("make test: all checks passed").
- `cargo test -p zoraya-pmm`: exit 0; 17 + 5 tests passed (the 5 are `tests/two_gib.rs`).
- `make test-qemu-one` (128 MiB, HVF): exit 0, `PASS: 120 passed, 9 expected failures`; the new test printed
  `scanned 512 words in 7041 ns masked`.
- `make test-qemu-auto-smp4-2g` (2 GiB, 4 CPUs, HVF): exit 0, `PASS: 120 passed, 9 expected failures`;
  `scanned 8192 words in 93625 ns masked`.
- Not run: any TCG step, `make test-keeper` (so INV-0436-2's script was not run by the reviewer), `make run-browser`.
  `target/test-logs/tcg-smp4-gicv3.log` in the worktree is from an earlier version of the test (it prints
  `(host 514x HVF's fill)`, which the committed code does not): the 1.2 ms TCG figure in ADR-0436, design.md and
  the report was not measured with the code under review.

## Findings

### 1. error: INV-0436-1 and its test allow a masked interval four to eight times INV-0270-1's 2 ms under HVF

Where: `kernel/src/testing.rs:2419-2421, 2458-2459`; `docs/adr/0436-the-2-gib-guest-as-built.md` INV-0436-1;
`docs/adr/invariants.md` row INV-0436-1.

What: the limit is `words * 1 us + 200 us` on every host. At 2 GiB that is 8.39 ms, at the 4 GiB the test still accepts
(`MAX_SCAN_WORDS = 16384`) it is 16.58 ms. INV-0270-1 (accepted) bounds every masked interval at 2 ms under HVF. The new
invariant, written as "against ADR-0270's 2 ms", in fact replaces 2 ms by a limit that grows with RAM, and the only
in-guest check of the allocator's masked time cannot fail under HVF until the scan is ~90 times slower than measured.
That weakens an accepted invariant inside a proposed ADR without naming it as a deviation (CLAUDE.md, ADR-0001), and it
contradicts ADR-0436's own decision driver 1 ("a limit stated for one size is not a limit").

Scenario: a change makes the scan read the map bit by bit (64 times the work, the shape ADR-0270's Q6 note already had to
remove from the run search): under HVF at 2 GiB that is ~6 ms masked per allocation near exhaustion; the test passes
(6 ms < 8.39 ms) and INV-0270-1 is broken with the check green.

Fix: two checks, as ADR-0270 Q2/Q6 already split them. (a) Host-independent: the words the allocation read, from
`alloc_counted` through a test-only `frames::alloc_counted` (see finding 3), `<= alloc_scan_bound()` and close to it
(the worst case was reached). (b) Time: `<= 2 ms` when the host is HVF-speed (the calibration `t_bound` uses, or the
measured ns per word below a threshold), and off HVF only ADR-0270 Q6's 1 s. Write INV-0436-1 as "the scan reads at most
`alloc_scan_bound` words and INV-0270-1's 2 ms holds for it under HVF at 2 GiB", not as a new time bound.

### 2. warning: the time is read from a counter that is global across CPUs; at 2 GiB (only measured on 4 CPUs) it can under-report or report another CPU's interval

Where: `kernel/src/steps.rs:210, 267-289` (`SINCE`, `MAX_TICKS` are one static each, not per CPU), used by
`kernel/src/testing.rs:2447-2451`; `kernel/src/arch/aarch64/exception.rs:441-445` calls `mask_began`/`mask_ended` on
every exception of every CPU.

What: while CPU A runs the 8192-word scan, any exception taken and returned on CPU B (a timer tick, an IPI) runs
`masked::end()` on B, which swaps `SINCE` to 0 and records only the part of A's interval up to then; A's
`restore_interrupts` then records nothing. If B was already masked when A masked, A's `begin` is a no-op and the
recorded value is B's interval. The test can therefore read a short value or, after its reset at line 2448, even 0, and
pass. Both 2 GiB configurations (`auto-smp4-2g`, `tcg-smp4-gicv3`) run 4 CPUs; at 1.2 ms (TCG) an interrupt on one of
three other CPUs within the interval is likely. The infrastructure is older than F8, but the numbers ADR-0436 records
(94 us, 1.2 ms) and its check rest on it.

Fix: in this test, read `cntvct_el0` before and after `frames::alloc()` inside the masked section (one CPU, exact), and
assert on that. Record in ADR-0270's open questions (rule 6) that `masked::*` is global and wrong on SMP, or make it
per CPU.

### 3. warning: the in-guest test does not check that it reached the worst case, and its comment about where the search stands is wrong

Where: `kernel/src/testing.rs:2437-2446, 2457`.

What: after `hoard_frames`, the hint is where the last frame of the map was taken, which is just below where the search
stood before the hoard (next-fit goes to the top, wraps, and comes back up to the old hint), not "at the top of RAM".
The freed frame is the 131st most recently allocated; it is two words behind the hint only if those 131 frames were
contiguous. With a fragmented map near the old hint, or reaper frames at the head of the list (`frames::alloc` falls
back to `reaper::take`, which does not move the hint), the scan is shorter, and the test cannot tell: the kernel calls
`frames::alloc`, not `alloc_counted`, and the printed "scanned {words} words" is the map's size, not what was read.
The measured 93.6 us is consistent with a whole scan today, but nothing asserts it.

Fix: expose `alloc_counted` under `qemu-test` (`frames::alloc_counted`), assert `read >= alloc_scan_bound() - 3` and
`read <= alloc_scan_bound()`, print `read`, and correct the comment.

### 4. warning: a single-shot time bound off HVF reintroduces the flake ADR-0270 Q6 removed

Where: `kernel/src/testing.rs:2458-2459`; the TCG configurations of `make test-qemu` (`tcg-default`, `tcg-el2`,
`tcg-smp4-gicv2` at 128 MiB, `tcg-smp4-gicv3` at 2 GiB) on CI.

What: ADR-0270's Q6 note records CI TCG runs with memory work 20 to 40 times slower than in green runs, and as a result
`t_bound` checks time off HVF only against 1 s. This test is one measurement, not a best of three, with a limit of
712 us at 128 MiB. At the ~146 ns a word implied by 1.2 ms / 8192, 512 words are ~75 us; 20 to 40 times that is 1.5 to
3 ms, above 712 us. At 2 GiB, 1.2 ms x 20 = 24 ms against 8.39 ms. A slow CI runner fails the test with nothing wrong.

Fix: finding 1's split (words on every host, 2 ms only at HVF speed, 1 s otherwise); at least a best of three.

### 5. warning: "one loop scales with RAM" is not true; ADR-0270 already records another that scales with a process's size, and a 2 GiB guest lets a room be 1.2 GiB

Where: `docs/adr/0436-the-2-gib-guest-as-built.md` Context ("none states a RAM size. What does scale with RAM is one
loop"), `docs/design.md:221`, `docs/tasks/TASK-34-report.md` F8 notes.

What: ADR-0270's Q6 note (lines 258-262, "Still inline, recorded here for the architect") says `memory_map`'s
`reserve_step` and `adopt` copy the space's whole frame list under the space's lock, `N * 8` bytes in one interval,
"512 units at 1 GiB". At 128 MiB no room can reach that; on the 2 GiB guest the session is 1452930K and the F8 script
itself makes a room of 1200M, so a single process can hold ~300,000 frames and copy ~2.4 MB masked. That is a size
F8 newly makes reachable and the claim that the frame scan is the only RAM-scaled loop hides it. Also the heap's growth
(`GROW_SIZE`, 256 frames in one interval) runs up to 256 `frames::alloc` in one interval; near exhaustion each is a scan
of up to the whole map in turn when other CPUs free behind the search, so the per-interval bound is not "one scan".

Fix: name both in ADR-0436's Context and in design.md as known limitations at 2 GiB, and either measure the list copy at
the 2 GiB room size (a `t_bound` case at `--mem 2G`) or leave it explicitly to the architect.

### 6. warning: `make run-browser` is untested, duplicates `RUN_GUI_CMD`, and its 2 GiB default reads against ADR-0415's decision sentence

Where: `Makefile:194-201`; `tests/shell/browser-guest.script:6-7`; ADR-0415 `### Decision` ("there is no
`make run-browser` default of 2 GiB (the architect, 2026-11-14)").

What: (a) The plan's "done when" is "`make run-browser` boots a 2 GiB guest". No step boots that command line: the
keeper script runs `--test --keys --disk ... --mem 2G` without `--gui` or `--net`, so the script's header claim "The
line of `make run-browser`, with a disk and without a window" is not true, and `run-browser`'s copy of `RUN_GUI_CMD` can
drift from it (the GUI test boots `RUN_GUI_CMD` itself). (b) The architect's sentence in ADR-0415 can be read as rejecting
a 2 GiB default for `make run-browser`, while item 5 and the plan ask for exactly that. ADR-0436 does not mention the
tension; the Makefile comment cites the same decision as saying only that `run` and `run-gui` stay.

Fix: give `RUN_GUI_CMD` the memory as a parameter and define `run-browser` through it; boot that line in a test (the
GUI test's way, `--display-opts none`); add the ADR-0415 sentence to ADR-0436's Context as a question for the architect.

### 7. note: the `top --session` deviation is stated honestly, but the script does not check that the session is sized from RAM

Where: `tests/shell/browser-guest.script:20-27`; ADR-0436 Recommendation; `docs/design.md:224` against
`docs/design.md:1852`.

The deviation is stated in ADR-0436, design.md and the report, and is for the architect. The architect needs to decide:
(a) accept `top`'s `session` row plus the keeper's refusal above the session's ceiling as F8's "`top --session` shows its
memory", leaving ADR-0267's `top --session` (services' accounts) to when a service reports its use; or (b) require
`top --session` now. As evidence for (a), the script is weak: `top`'s expectations are `ROOM`, `tab`, `of 1200M` and
`session`, with no value; the 1200M/1800M pair would pass for any session ceiling between 1200M and 1800M, including
a fixed one. Expect the session's size (or the ceiling, 1452930K, or a value derived from the free frames) in the output.
design.md:1852 still says "the keeper has no session ceiling to give", which line 224 and the script contradict; one of
them is stale.

### 8. note: the keeper step's expected line is fixed while its memory is a person's variable

Where: `Makefile:1558, 1561`. `--expect-line "2048 MiB  RAM"` with `--mem $(BROWSER_MEM)`: `BROWSER_MEM=1G make
test-keeper` fails for no fault. Use a fixed `--mem 2G` in the test (the test is of the 2 GiB guest), and keep
`BROWSER_MEM` for `run-browser`.

### 9. note: the RAM limit is checked on `total / 64`, not on the bound the allocator states

Where: `kernel/src/testing.rs:2431`. With several regions `alloc_scan_bound` rounds up per region and can exceed
`total.div_ceil(64)`. Assert on `alloc_scan_bound()` (through the same test hook as finding 3).

### 10. note: mutation results

`build/mutants/cargo/mutants.out/missed.txt` has one survivor: `lib.rs:354:30 replace / with *` (`end_word` becomes
`first_index * 64`, clamped to `region.words()`, i.e. the old full reread of the start region). It is equivalent through
the public API: step 0 read words `first_word..words()` of the start region whole and nothing changed since, so a free
frame in them would already have been returned; any free frame left is in a word below `first_word` and is found, at the
same count, before the mutant's extra words are reached. It differs only when the map disagrees with `stats.free`, which
no public call can produce. The timeout (`353:36 == with !=`) makes step 0 empty and every allocation scan from word 0,
O(n^2) over a 2 GiB drain; it is effectively caught. The kernel list `tools/mutants/frames-f8.toml` has no mutant on the
time limit or on which frame is freed (e.g. 130 to 0, which makes the scan short), which findings 1 and 3 would kill.

## Checked and found sound

- The scan (`alloc_counted`): every word of every region is read at most once (step 0 from the hint's word to the end,
  other regions whole, the wrap-around from 0 to the hint's word exclusive, the hint's word having been read whole in
  step 0, including its bits below the hint); no free frame is missed and none is handed out twice (the 2 GiB drain and
  the multi-region drains check it); next-fit order and the common one-word cost are unchanged (step 0 is as before);
  the `.min(region.words())` keeps the end bound in range; indices and addresses are `u64`, so regions above 4 GiB and
  2 GiB exactly (`0xC000_0000` end) do not overflow; `alloc_scan_bound` is the per-region `div_ceil` sum the
  host tests compare against.
- The host tests: 2 GiB counts, reserved frames never handed out, the last frame and one past it, 128 MiB to 4 GiB in one
  region and in two with the second at 512 GiB, the exact worst case of 8192 words in one and two regions, bounded run
  search steps; `cap_memory!()` is installed.
- RAM-size assumptions in the kernel: the direct map is 128 TiB with an assert (`paging.rs:149`); TTBR0's 4 GiB identity
  map is used only before the switch and is disabled after; the frame map is placed by `place_metadata` and reached
  through the direct map; the heap window is 1 TiB and grows on demand; `MAX_REGIONS` is 8 regions, not a size; no
  fixed array sized by RAM found.
- `SAFETY` comment on the new `unsafe` write is sound (the frame is allocated again and owned); no change to
  `tools/run-qemu.sh`; the new script is in `tools/test-map.txt`; `make check` (adr-check: 840, host paths, test caps)
  passes. Host memory: the keeper's 2 GiB guest boots the ordinary kernel, which touches little; the test kernel's
  hoard already touched all of a 2 GiB guest before F8.

## Verdict

Fix first: finding 1 (the invariant and its check must keep INV-0270-1's 2 ms under HVF), and findings 2 to 4 (measure on
the CPU that scans, assert the words read, no single-shot time bound off HVF). Findings 5 and 6 are documentation and
one Makefile change plus a question for the architect; 7 is the architect's decision.

## Re-check

Re-check by the `opus-review` subagent, 2026-11-16, of `git diff 0673682f..ebd44b0c` (the review commit to the fixed
head) against the findings above. Run by the reviewer on the worktree, one at a time, `CARGO_BUILD_JOBS=2`, output to
files:

- `make check`: exit 0 ("make test: all checks passed").
- `cargo test -p zoraya-pmm`: exit 0; 17 + 5 passed.
- `make test-qemu-auto-smp4-2g` (2 GiB, 4 CPUs, HVF): exit 0, `PASS: 120 passed, 9 expected failures`; the test printed
  `the worst single allocation read 8191 of 8192 words in 104000 ns masked (a word of an array: 8 ns, HVF-speed)`.
- `make -n run-gui run-browser`: `--gui --net --gui-scale 2 --mem 512M --disk build/disk.img build/zoraya-kernel.elf`
  and the same line with `--mem 2G`; the other user of `RUN_GUI_CMD` (`test-hand-run`, two arguments) still gets 512M.
- Not run: any TCG step, `make test-keeper` (the changed `browser-guest.script` was not run by the reviewer).

Findings:

1. Fixed. The test asserts the words read (`frames::alloc_counted`, `read <= alloc_scan_bound()` and `read + 4 >= bound`),
   and the time against 2 ms where the host is HVF-speed, 1 s otherwise. INV-0436-1 (ADR and `invariants.md`, same text)
   no longer states a time bound of its own; it says INV-0270-1's 2 ms holds under HVF and how the check tells HVF from
   not. INV-0270-1 is not weakened. The hooks are `#[cfg(feature = "qemu-test")]`; only the test kernel's build
   (`Makefile:130`) and its clippy enable that feature, so they are not in the ordinary kernel.
2. Fixed in the test: `cntvct` read on the scanning CPU inside the masked section, around the call. The note that
   `steps::masked::*` is one counter for all CPUs went into ADR-0436's open questions, not ADR-0270's (rule 6), although
   it is INV-0270-1's own check (`t_bound`) that rests on that counter; see note 13.
3. Fixed: `read` is asserted near the bound (8191 of 8192 measured) and printed; the comment is corrected. A mutant that
   frees a frame nearer the hint (130 to 0) now fails the test.
4. Fixed for CI: CI runs TCG, the calibration there should be far above 50 ns a word (8 ns under HVF; the scan was about
   13 times slower under TCG), so CI checks only 1 s. The margin is about 2x and was not measured under TCG; see note 12.
5. Fixed: ADR-0436 Context and design.md name the frame-list copy and the heap growth as unbounded at 2 GiB.
6. Fixed: `run-browser` is `RUN_GUI_CMD` with a third argument; the ADR and the Makefile say no step boots it; the
   ADR-0415 sentence is put to the architect.
7. Fixed as asked: the script expects `above the session's 1452930K`, and design.md:1852 is no longer stale. The
   deviation stays the architect's.
8. Fixed: the step passes `--mem 2G` itself.
9. Fixed: the RAM limit is asserted on `alloc_scan_bound()`.
10. Unchanged (mutation results; the kernel list was not extended, but findings 1 and 3 now kill the mutants named).

New:

11. note: the time check under HVF is one measurement. `Where:` `kernel/src/testing.rs` (the call between
    `save_and_mask_interrupts` and `restore_interrupts`). `t_bound` takes the best of three for its large cases because a
    host can stall a vCPU; here masking stops only the guest's interrupts, not the host. 104 us against 2 ms leaves 19x,
    so a stall of about 1.9 ms of the vCPU thread during that one call (a loaded 8 GB host running `make test-par`) fails
    the test with nothing wrong. Fix: best of three (re-free the same frame, which the hoard makes cheap), as finding 4
    asked.
12. note: the speed threshold is 50 ns a word of an array, the HVF calibration measured 8 ns, and the TCG figure is
    inferred (no TCG run of this code). A fast CI runner that calibrates at 50 ns or less is then held to 2 ms on a
    single-shot TCG scan (1.2 ms measured once, with an earlier version of the test). Run `tcg-smp4-gicv3` once and record
    the calibration it prints in ADR-0436, or set the threshold from that figure.
13. note: ADR-0436's open question says `steps::masked::*` is global and can under-report on SMP, and the test avoids
    it; but INV-0270-1's check (`t_bound`, the masked-time counter) uses that counter on the SMP configurations, so the
    accepted invariant's check is the one that is unsound. Record it in ADR-0270's open questions (rule 6).
14. note: ADR-0436's Recommendation and design.md still give "1.2 ms under TCG" as measured; it was measured with an
    earlier version of the test (the review's "Not run" above). design.md says "(once)"; say which code measured it or
    measure again.
15. note: `browser-guest.script` now expects the exact 1452930K. It changes with any change to the frames reserved at
    boot (kernel image, metadata, the keeper's share), so it will break for a reason unrelated to F8; acceptable as the
    review asked for a value, but a range or a value derived from `mem` in the same output would last longer.

## Re-check verdict

Merge. Findings 1 to 9 are fixed; 11 to 15 are notes (11 and 12 are possible flakes, not seen; 13 is a rule-6 note for
ADR-0270).
