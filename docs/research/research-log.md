# TASK-01 research log

Log for [TASK-01](TASK-01-design-mistakes-research.md): what was done, what was dropped or changed, what the
independent verification found, and what remains unresolved. Dates are 2026-10-19 to 2026-10-20.

## Outputs

| File | Contents |
|---|---|
| [index.md](index.md) | Index-phase candidate list and ID allocation (stable IDs) |
| [design-mistakes.md](design-mistakes.md) | 105 DM entries in 16 categories |
| [held-up-well.md](held-up-well.md) | 21 GR entries |
| [sources.md](sources.md) | 655 sources with `supports:` audit lines and verifier notes |
| [alternatives-postmortem.md](alternatives-postmortem.md) | 19 alternative and research systems, with a summary table |
| [candidate-principles.md](candidate-principles.md) | 28 candidate principles (CP-01 to CP-28) and 15 explicit conflicts (X-1 to X-15) |
| `_staging/` | Raw stream outputs, verifier outputs and findings, merge and apply scripts (kept for audit) |

## Process

1. **Index** (commit `63a5d1d`). 97 DM and 20 GR candidates, and a list of systems for the postmortem. IDs and
   source-ID blocks were allocated per research stream, so parallel work could not collide.
2. **Research** (commit `74a3170`). Eleven parallel streams:
   - A–H: two DM categories each.
   - I: GR entries.
   - J and K: postmortems.
   Each stream fetched every source it cited. Its output and stream notes are in `_staging/stream-*`.
3. **Merge** (commit `210cec2`). A scripted merge:
   - grouped entries by category;
   - merged 43 duplicate source URLs into the lower ID (table at the end of `sources.md`);
   - checked mechanically that every cited ID exists and that every entry has ≥2 verified sources with ≥1 primary.
4. **Verification** (commit `6e035ed`). Ten independent verifiers, V1–V10, none of which wrote the entries:
   - V1–V7 covered DM entries, 15 each.
   - V8 covered the GR entries.
   - V9 and V10 covered the postmortems.

   Each re-fetched the cited sources and checked:
   - that each source supports its claim;
   - source type labels;
   - classification and confidence honesty;
   - fairness of the steelman;
   - quotes under 15 words.

   Corrections were applied by script (`_staging/apply_verify.py`). Per-item verdicts are in
   `_staging/verify/V*-findings.md`.
5. **Synthesis.** `alternatives-postmortem.md` and `candidate-principles.md`, written from the verified files only.

**Interruptions.** Usage limits cut off the research and verification phases twice. Each agent was resumed with its
context intact, so no work was redone.

**Fetching.** When WebFetch was rate-limited, agents fetched pages with `curl` or used Internet Archive copies, and
extracted PDF text locally. Every source counted as verified was actually retrieved and read. Some LWN pages were
checked through archive copies because LWN returned HTTP 429.

## Entries per category and classification (after verification)

| Category | Entries | mistake | tradeoff | contested | right-at-the-time-wrong-now |
|---|---|---|---|---|---|
| 1. Process model and program execution | 9 | 0 | 3 | 0 | 6 |
| 2. Security, authority and identity | 10 | 2 | 7 | 0 | 1 |
| 3. Inter-process communication | 6 | 3 | 1 | 2 | 0 |
| 4. I/O, asynchrony and event handling | 8 | 3 | 3 | 1 | 1 |
| 5. Memory management | 6 | 0 | 4 | 0 | 2 |
| 6. Filesystems, namespaces and storage semantics | 10 | 3 | 5 | 1 | 1 |
| 7. Kernel structure, drivers and fault isolation | 7 | 0 | 3 | 2 | 2 |
| 8. Kernel–user interfaces and ABI design | 7 | 2 | 2 | 0 | 3 |
| 9. Concurrency and synchronization primitives | 6 | 1 | 1 | 1 | 3 |
| 10. Time, clocks and scheduling | 6 | 1 | 1 | 1 | 3 |
| 11. Terminals, sessions and consoles | 5 | 1 | 2 | 0 | 2 |
| 12. Networking APIs | 5 | 0 | 2 | 1 | 2 |
| 13. Graphics, input and window systems | 6 | 1 | 0 | 1 | 4 |
| 14. Configuration, observability and system management | 6 | 2 | 3 | 1 | 0 |
| 15. Boot, update and recovery | 4 | 0 | 3 | 1 | 0 |
| 16. Text, encoding and locale | 4 | 1 | 1 | 1 | 1 |
| **Total** | **105** | **20** | **41** | **13** | **31** |

- Every category has at least 4 entries.
- DM confidence: 51 high and 54 medium, none low.
- GR classifications: 3 `held-up-well` (GR-012 RCU, GR-015 UTF-8, GR-019 separate address spaces), 17
  `held-up-with-caveats`, and 1 `contested` (GR-008 object capabilities).

## Sources by type

| Type | Verified | Unverified (not counted toward minimums) |
|---|---|---|
| primary | 552 | 2 (S308 Liedtke 1993, connection refused; S742 Palix project site, refused connections) |
| secondary | 82 | 0 |
| weak | 18 | 1 (S503, LWN reader comments not readable anonymously; citation removed) |
| **Total** | **652** | **3** |

Weak sources are used only for practitioner opinion or anecdote, and each such use is labelled in the entry. Sources
written by a system's own designers are labelled primary, and the entry notes the author's position where it
matters. Examples: the Biggs et al. seL4-group CVE study in DM-046; Key Logic's performance claims in the KeyKOS
postmortem; the s6 author's systemd critique in DM-088.

Inline `[unverified]` markers that remain: 48 in DM entries, 33 in GR entries, 23 in the postmortem. Each marks a
side claim. Unverified sources are excluded from the minimum source counts (checked mechanically). Verifiers were
instructed to downgrade any entry whose classification depended on an unverified claim.

## Contested items

DM-020 untyped byte streams · DM-021 userspace message bus · DM-026 readiness vs completion I/O · DM-045
transactional filesystem · DM-046 monolithic vs microkernel · DM-049 stable driver API · DM-061 priority
inversion protocol · DM-068 scheduler design and sched_ext · DM-078 in-kernel vs user-space network stack · DM-084
Wayland minimal core · DM-088 init systems · DM-093 verified boot by default · DM-094 case-insensitive names and
normalization · GR-008 object capabilities.

Each one states both positions with sources. None is resolved.

## Dropped, merged, added and changed

**Dropped:** none of the 97 indexed candidates. Weak candidates were narrowed or reclassified instead. For example,
DM-035 (`brk`/`sbrk`) was kept with low relevance.

**Added during research** (from the reserve ID blocks):
- DM-100 process-tree containment.
- DM-101 containers as an emergent assembly (added at coordinator request, then narrowed to avoid overlap with
  DM-110 and DM-111).
- DM-110 unprivileged user namespaces.
- DM-111 cgroups v1→v2.
- DM-120 futex multiplexer.
- DM-130 waitable events outside the fd model. Kept separate from DM-003 and DM-017 because it is about the
  missing unified waitable-object concept. Verifier V7 cut the repeated text to cross-references.
- DM-160 double-fetch races.
- DM-161 DMA without IOMMU isolation.
- GR-021 virtio.

**The index missed one seed item:** "namespaces and cgroups retrofitted for containers". Streams A and B both
noticed, and it is now covered by DM-101, DM-110 and DM-111.

**Changes made by verification** (checked against git history: 16 classification changes and 3 changes to
confidence only):
- *right-at-the-time-wrong-now → tradeoff:* DM-009, DM-012, DM-027, DM-034, DM-041, DM-042, DM-075, DM-089,
  DM-095.
- *right-at-the-time-wrong-now → mistake:* DM-017, DM-082. The flaw was present from the start; no constraint
  changed later.
- *mistake → tradeoff:* DM-047 (its own alternatives have large costs), DM-092 (it pays for the multi-vendor boot
  ecosystem).
- *contested → tradeoff:* DM-018. The primary sources agree that Mach's slowness came from its implementation; the
  real dispute is DM-046.
- *Confidence only:* DM-029, DM-048 and DM-059, high → medium. DM-089 also dropped from high to medium along with
  its reclassification.
- *GR:* GR-005 held-up-well/high → held-up-with-caveats/medium (it was missing Drepper's dissent, and glibc's
  posix_spawn speed comes from vfork-style clone). GR-017 held-up-well → held-up-with-caveats, for consistency with
  DM-090.

**Net effect.** `right-at-the-time-wrong-now` fell from 42 to 31 entries and `tradeoff` rose from 29 to 41. The label
was over-used before verification, as the verification brief anticipated.

## Verification phase findings

| Verifier | Slice | Pass | Fixed | Reclassified or downgraded | Flagged |
|---|---|---|---|---|---|
| V1 | DM-001–015 | 1 | 12 | 2 | 0 |
| V2 | DM-016–030 | 0 | 11 | 4 | 0 |
| V3 | DM-031–045 | 1 | 11 | 3 | 0 |
| V4 | DM-046–060 | 3 | 10 | 2 | 0 |
| V5 | DM-061–075 | 0 | 14 | 1 | 0 |
| V6 | DM-076–090 | 0 | 13 | 2 | 0 |
| V7 | DM-091–161 | 1 | 12 | 2 | 0 |
| V8 | GR-001–021 | 1 | 18 | 2 | 0 |
| V9 | Postmortem part 1 (7 systems) | 0 | 7 | — | 1 (QNX) |
| V10 | Postmortem part 2 (12 systems) | 0 | 12 | — | 0 |

The verifiers added 42 new sources (S2000 and up). After their fixes every entry still has ≥2 verified sources with
≥1 primary. This was re-checked mechanically after merging.

**Most serious problem classes found.** These matter for how far the unverified first drafts could have been trusted.

1. **Claims attributed to sources that don't contain them.** This was the most common defect, found in every slice.
   - S707's "default audio volume" example is about L4 driver VMs, not MINIX 3.
   - `oom_score_adj` was cited to a 2009 article that predates it.
   - Three items in "the Plan 9 authors' own list" are not in that paper.
   - The EROS paper explicitly says the checkpoint limitation is not an EROS problem; it was attributed to EROS.
   - Tanenbaum's letter does not say Intel gave "no feedback".
   - The exokernel paper never calls the libOS interface unstable.
   - A paraphrase was presented as a verbatim Go release-notes quote.
2. **Wrong facts.**
   - `CLONE_PIDFD` landed in Linux 5.2, not 5.3.
   - SIGHUP on hangup goes to the session leader, not the foreground group.
   - The POSIX history of `CLOCK_MONOTONIC` was wrong.
   - OpenBSD pledge *prevents setting* setuid bits; the draft said it ignored them.
   - futex2 supports only 32-bit today.
   - brk/sbrk do not originate in 4.3BSD (V7 Unix had brk).
   - A CVE version range was wrong.
3. **Overstatements.**
   - `cgroup.kill` called "race-free".
   - "E0PD lets Linux skip KPTI".
   - "Every serious user built workarounds" for POSIX locks.
   - "Hubris ships in Oxide's product" was cited to a talk given before it shipped; it now has a current source.
4. **Second-hand statistics presented as established.**
   - The "85% of Windows XP crashes are drivers" figure goes back only to a 2003 private communication cited in the
     Nooks paper. DM-047 now calls it an unpublished anecdote.
   - The "44% of exploits need unprivileged user namespaces" figure appears only in Canonical's attribution to
     Google. Google's post gives a 60% figure for io_uring and no user-namespace number. DM-110 marks the 44%
     figure second-hand.
5. **Out-of-date status for active systems.**
   - Fuchsia's status was cited to 2024 sources.
   - Genode's "current" release was stale.
   - The wayland-protocols governance rules came from an old mirror: the current rules require 2 ACKs, and the
     experimental stage was adopted in October 2024.
   - The seL4 FAQ lags its verified-configurations page.
6. **Bias corrections.**
   - Drepper's dissent was added to DM-001 and GR-005.
   - Hardy's report that capability systems came out smaller was added to DM-009.
   - Business events presented as causes of non-adoption without a source are now marked as inference (L4/OK Labs,
     Shapiro, licensing timing).
   - Designer- and vendor-written sources are labelled as such.

## Unresolved questions

**Evidence gaps that remain:**
- **QNX (open flag, V9).** No source gives the technical reasons QNX did not spread beyond embedded. The section says
  so.
- **Starnix.** Whether it runs in any shipping product, and its current syscall overhead, is unverified.
- **Midori.** Why it ended rests only on Duffy's statement that its teams were moved elsewhere in 2012–2014.
- **KeyKOS.** McDonnell Douglas's role is unverified; the 1992 Dr. Dobb's article sits behind a bot wall.
- **BeOS.** The claim that the Apple deal failed over price rests on one weak source. Primary sources for BeOS's
  technical gaps at the time were not found.
- **Genode and Hubris.** No independent technical assessment was found; the sections state this limitation.
- **Capsicum on Linux.** Whether the port was ever merged is believed not, but rests only on secondary pointers.
- **ARMv8.5 E0PD.** Whether it fully blocks speculative access to the kernel half is unsettled (DM-033).
- **Linux memory-safety CVEs.** No Linux-specific breakdown of CVEs by memory-safety class was found (DM-048).
- **Zircon handle cycles.** Whether move-only handles can still form unreachable cycles is not addressed in the
  Zircon docs (DM-022).
- **APFS.** The current checksum scope is unverified; the 2016 source says metadata only (GR-014).
- **Liedtke 1993.** The SOSP paper could not be fetched; the 20× IPC claim is cited through S305.
- **Ritchie's history paper.** "Evolution of the Unix Time-sharing System" could not be fetched; the fork history
  is cited through Baumann et al.

**Questions for the architect that the evidence frames but does not answer.** These are the contested entries plus
the conflicts listed in `candidate-principles.md`. In particular:
- X-1: capability-only access vs POSIX porting.
- X-2: no fork vs the C toolchain.
- X-4: readiness vs completion I/O.
- X-7: microkernel isolation vs performance.

**Measurable but not measured here.**
- The exact POSIX surface needed by the chosen C compiler, linker, `make` and shell (CP-27). This bounds X-1, X-2
  and X-3.
- Whether QEMU `virt`, as we will run it, exposes virtio-iommu or SMMUv3 (CP-16).
- seL4's verified AArch64 configurations do not include QEMU `virt` (GR-020). This matters if seL4 is used as a
  reference or as a base.

**Resolved during verification:**
- sched_ext merged in Linux 6.12 (S2406).
- O(1) scheduler history (S2405, S2407) and Zijlstra's sched_ext objection (S2408).
- Accetta et al. 1986 Mach paper (S2800).
- QNX's 2007 source release under a hybrid licence (S2802).
- MINIX 3 inactive after 2018 (S2909).
- FreeBSD process descriptors (S2002).
- NT alertable waits and APCs (S2100).
- seL4 MCS scheduling-context donation (S2401).
- Rust atomics following C++20 (S2400).
- The WDF stable driver interface (S2300).
- Paget's shatter paper, via an archived copy.
- The Zircon futex and channel semantics (S2600, S2601).

## Definition of done

| Criterion | Status |
|---|---|
| Every category has ≥2 entries | Met: the minimum is 4 (categories 15 and 16) |
| Every entry has ≥2 sources, ≥1 primary, all fetched and verified | Met. Checked mechanically after verification; unverified sources are excluded from the count |
| Every alternative system in scope has a postmortem section | Met: 19 sections, covering every system in scope plus MINIX 3, Amoeba, Sprite, Exokernel and Oberon |
| Verification by an independent subagent, findings recorded here | Met: V1–V10, summarized above, with details in `_staging/verify/` |
| `candidate-principles.md` lists conflicts explicitly | Met: X-1 to X-15 |
