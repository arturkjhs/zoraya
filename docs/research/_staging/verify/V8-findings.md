# V8 findings: GR-001 to GR-021 (held-up-well.md)

LWN pages were rate-limited (HTTP 429). They were checked through Internet Archive copies (`web.archive.org/web/2025id_/<url>`). The S946 PDF could not be text-extracted; it was checked against the author-hosted HTML version at https://9p.io/sys/doc/utf.html.

## Items

GR-001: FIXED — the "small system" goal reworded as the paper's stated achievement (it is framed as an achievement, not a goal); the pidfd claim corrected (pidfd built up over several releases, poll support in 5.3; "processes were the main object not yet represented" is not in S109); "arrived at the same pattern independently" removed, since no source supports it and S161 was wrongly cited for Windows/Zircon.
GR-002: FIXED — compiler-driver/ssh examples marked [unverified]; S900 kept only for the "program need not know" property.
GR-003: FIXED — added the S2702 caveat: per Microsoft, `DuplicateHandle` can in some cases give a handle *more* access than the original, so NT handles do not strictly attenuate. The "can be passed to a less-privileged process" claim is now sourced. The Zircon "no global ambient namespace" claim moved to S2700 (S911 does not say it).
GR-004: FIXED — Fuchsia namespace claim re-sourced to S2700 (per-component namespace, no global root, no ambient authority); S103 only supports "creator supplies the handles".
GR-005: DOWNGRADED — Classification: held-up-well → held-up-with-caveats; Confidence: high → medium. Reasons: DM-001 records Drepper's dissent (S101), which GR-005 left out; on Linux the measured posix_spawn speed comes from glibc's `clone(CLONE_VM|CLONE_VFORK)` (S2703), not from a separate spawn primitive; Baumann 2019 had been placed under "Why it was chosen" as if it were a designer rationale; "is costly and complicated" was not a verbatim quote (now paraphrased); "Fuchsia shipped with no fork" re-sourced to S2701.
GR-006: FIXED — "WinFS was cut from Vista" is not in S914 (marked [unverified]); "most serious attempt to replace it" softened, because WinFS was meant to sit beside the path namespace, not replace it; the S912 loop claim worded to match the source.
GR-007: FIXED — S916's fourth problem is "performance" (page-table contention, single-threaded eviction, TLB shootdowns), not TLB shootdowns alone. Successor-unification claims stay [unverified].
GR-008: FIXED — Capsicum tcpdump figure: the abstract says about ten lines (a two-line `cap_enter` plus rights narrowing); 22,000-line comparator added; "integrity and confidentiality rest on the capability model" was an inference and is now stated as what S919/S920 say. Contested balance: the anti-capability side has no source of its own (only revealed preference via DM-014/015/016). This is now stated in the entry. Still classified contested.
GR-009: FIXED — "considering disabling on GKE" → "investigating disabling by default on GKE Autopilot" (S220), and production-server disablement added; "extends it to most system calls" → "a growing set of operations"; the IOCP coverage list matched to S417; the buffer-ownership claim, previously [unverified], now cites S419 (weak) and DM-026. Consistent with DM-025/DM-026.
GR-010: FIXED — the a.out timeline matched to S928 (removal planned for 5.18; actual removal not confirmed by the source).
GR-011: FIXED — WaitOnAddress is documented as same-process only, unlike futexes (S933), so "same idea" became "similar idea" with that caveat; CVE-2014-3153 exploitation, previously [unverified], is now supported by the CISA KEV listing in the NVD record (S368).
GR-012: FIXED — DYNIX/ptx origin is now supported by S936 (no longer [unverified]); "unsuitable for update-heavy" is not in S935 (it says "optimized for read-mostly") and was reworded; the "over 20 years" claim tied to S936's "more than a decade" in 2012.
GR-013: FIXED — S940 is a patch posting, not proof of merge (merge marked [unverified]); "root-only in practice" softened; "checks safety, not intent" reworded to what S938 lists; the DTrace no-loops claim matched to S937 (forward branches only).
GR-014: FIXED — APFS: per S2704 (weak), APFS checksums metadata only, not user data, so it was moved to "partial" and the "checksummed ... designs are default" claim was corrected; the ZFS block-pointer wording matched to S941; the S942 study context (NetApp production systems) added.
GR-015: PASS — all claims checked against S946 (HTML mirror), S947, S951, S653.
GR-016: FIXED — added the S950 detail that portal sessions can persist via a restore token (the user is not always asked). Everything else checked against S829, S949, S954, S955. Consistent with DM-084 (fragmentation).
GR-017: DOWNGRADED — Classification: held-up-well → held-up-with-caveats. The entry lists four real limits itself, and DM-090 classifies A/B vs in-place as a `tradeoff` (storage cost, rigidity), so "held-up-well" contradicted the DM entry. Also: "developer mode with user consent" is not in S146 (replaced with what S146 says: no remote attestation, developers can run their own builds); dm-verity is now attributed to S134.
GR-018: FIXED — the 1992 priority inheritance was opt-in per S962; added the cross-reference to DM-018, where the L4 designers call synchronous rendezvous wrong for cross-core synchronisation. GR-018 had omitted this counter-evidence. S959/S960/S961/S968 claims all confirmed (boost at send, no revert on reply, `_NTO_CHF_FIXED_PRIORITY`, send-hierarchy rules, short-message double copy, lseek via MsgSend).
GR-019: FIXED — Singularity made hardware protection domains *optional* (S969); "kept hardware isolation as defence in depth" overstated it. Added the Site Isolation cost of about 450k LOC (S964).
GR-020: FIXED — AArch64 status made precise (see the known item below). The prior text was correct but incomplete: it omitted the EL2/hypervisor-only verified configuration, the missing capDL-initialiser proof, the board list without QEMU `virt`, and the stale FAQ wording. Relevance qualified accordingly.
GR-021: FIXED — S967 does not mention SmartNICs, vDPA, VDUSE or SEV (marked [unverified]; it says only that "some physical devices" implement virtio, and discusses TDX). "virtualization-only interface" contradicted S967 and was corrected. Added Lutomirski's generic-hardening argument.

### Known item: seL4 AArch64 verification status (checked 2026-10-20)

- **Verified on AArch64** (docs.sel4.systems Verified Configurations, S920): configurations `AARCH64_*_verified` run seL4 as a hypervisor at EL2, with FPU. Proved properties are C-level functional correctness including the fast path, integrity and availability (access control), and confidentiality (information flow). Boards listed include MaaXBoard, Odroid-C2/C4, ROCKPro64, Raspberry Pi 4B/5B, Rock3b, STM32MP25, TQMa8XQP, TX1/TX2, Ultra96v2, ZCU102/106, i.MX8M Mini/Plus/Quad and i.MX93.
- **Announcement** (S921, seL4 Discourse, June Andronick, 24 Aug 2026): Proofcraft completed the AArch64 confidentiality proof, which completes the security-isolation proofs for AArch64 C code under the stated assumptions.
- **Not verified on AArch64:** binary correctness / translation validation (the compiler is trusted; binary proofs exist only for AArch32 and RISCV64 per the FAQ, S919); model-level capDL initialiser proofs (listed for AArch32 only); MCS configurations (proofs "in progress"); multicore/SMP (not verified on any architecture; the SMP kernel uses a big lock; multikernel verification is in progress per the FAQ); SMMU/IOMMU, debug/printing interfaces and kernel boot (excluded from all verified configurations); DMA (must be off or trusted); timing channels (excluded from the confidentiality property); non-hypervisor (EL1) AArch64 builds; QEMU `virt` (not on the verified board list).
- **Inconsistency:** the seL4 FAQ (S919) still says that AArch64 has functional-correctness proofs to C only and that security proofs exist for ARM and RISCV64. That text is out of date relative to S920/S921.

## Source directives

SRC S919: note — FAQ text on per-architecture proof status is stale as of 2026-10-20: it says AArch64 has C-level functional correctness only, while the Verified Configurations page (S920) and S921 show integrity and confidentiality are also proved. Prefer S920 for status.
SRC S920: note — Verified AArch64 configurations are hypervisor-mode (kernel at EL2) only; binary correctness and the capDL-initialiser proof are not listed for AArch64; QEMU virt is not a verified platform (checked 2026-10-20).
SRC S921: note — Post dated 24 Aug 2026 by June Andronick, reporting Proofcraft's work; primary (verification team announcing its own result).
SRC S946: note — The PDF's fonts defeat text extraction; the author-hosted HTML version https://9p.io/sys/doc/utf.html has the same content and was used for verification.
SRC S109: note — Rate-limited on 2026-10-20; verified via https://web.archive.org/web/2025id_/https://lwn.net/Articles/794707/ (same applies to S923, S928, S929, S934, S939, S954, S955, S967).
SRC S940: note — This is the v3 patch posting, not the merged commit; it supports "proposed", not "merged".
SRC S938: note — Advocacy site of the eBPF Foundation; it describes verifier conditions but does not state "safety not intent".
SRC S967: note — Does not mention SmartNICs, vDPA, VDUSE or SEV; it supports "some physical devices implement virtio" and TDX-motivated hardening only.
SRC S914: note — Does not say WinFS was cut from Vista; supports only "no separate WinFS delivery; pieces folded into SQL Server (Katmai) and ADO.NET (Orcas)", June 2006.
SRC S202: note — Abstract gives "around ten lines of code" for tcpdump (Figure 6 shows the two-line cap_enter change); Chromium about 100 lines; comparator sandboxes up to 22,000 lines.
SRC S933: note — Also supports: WaitOnAddress is documented for threads within the same process only (GR-011).
SRC S368: note — The NVD record includes the CISA KEV listing (added 2022-05-25); supports "known exploited" for GR-011.
SRC S936: note — Also supports: DYNIX/ptx had an earlier RCU equivalent, first used in networking (GR-012).
SRC S101: note — Also supports: GR-005 (Drepper's dissent: fork without exec useful; no realistic general replacement offered).
SRC S419: note — Also supports: GR-009 (kernel keeps using buffers after cancellation; conflict with Rust cancel-by-drop); weak.
SRC S2700: new — [S2700] Fuchsia project, Fuchsia Namespaces, fuchsia.dev, n.d. (page updated 2025), https://fuchsia.dev/fuchsia-src/concepts/process/namespaces, primary, checked 2026-10-20 | supports: GR-003 (no global root namespace; no ambient authority), GR-004 (per-component namespace = table of path prefixes to handles received at start)
SRC S2701: new — [S2701] Fuchsia project, C Library (libc), fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/kernel/libc, primary, checked 2026-10-20 | supports: GR-005 (Zircon has no fork or exec; process creation via fdio)
SRC S2702: new — [S2702] Microsoft, DuplicateHandle function (handleapi.h), Microsoft Learn (Win32 API), n.d., https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle, primary, checked 2026-10-20 | supports: GR-003 (handles can be duplicated into another process with same or requested access; in some cases the new handle can have more access than the original)
SRC S2703: new — [S2703] Linux man-pages project, posix_spawn(3), man7.org, n.d., https://man7.org/linux/man-pages/man3/posix_spawn.3.html, primary, checked 2026-10-20 | supports: GR-005 (since glibc 2.24 posix_spawn starts with clone(2) using CLONE_VM and CLONE_VFORK)
SRC S2704: new — [S2704] A. Leventhal, APFS in Detail: Data Integrity, ahl.dtrace.org (personal blog; author is a DTrace/ZFS-era Sun engineer, not an APFS designer), 2016, https://ahl.dtrace.org/2016/06/19/apfs-part5/, weak, checked 2026-10-20 | supports: GR-014 (APFS checksums metadata but not user data, per conversations with APFS engineers at its 2016 introduction)

## Summary

- Counts (21 items): 1 PASS (GR-015), 18 FIXED, 2 DOWNGRADED (GR-005, GR-017), 0 FLAG.
- Classification changes:
  - GR-005: held-up-well/high → held-up-with-caveats/medium.
  - GR-017: held-up-well → held-up-with-caveats (confidence stays high).
  - No other classification changed. GR-008 stays `contested`, but its anti-capability side has no source of its own; the entry now says so.
- Source counts: every item still has at least 2 sources with at least 1 primary. GR-004 now has 3 (S161, S103, S2700).
- Most serious problems:
  1. **Over-credit of the "explicit spawn" design (GR-005).** The entry left out the dissent recorded in DM-001 (Drepper). It also did not say that Linux posix_spawn's constant cost comes from vfork-style `clone`.
  2. **NT handles presented as capability-like (GR-003).** Microsoft documents that `DuplicateHandle` can sometimes *raise* access, which the entry missed.
  3. **APFS listed as a checksummed copy-on-write filesystem (GR-014).** It checksums metadata only.
  4. **Claims not in the cited source.** Examples:
     - WinFS "cut from Vista" (GR-006).
     - "Developer mode with user consent" (GR-017).
     - SmartNIC/vDPA/VDUSE/SEV, and "virtualization-only" (GR-021).
     - "Unsuitable for update-heavy" (GR-012).
     - Merged default-off unprivileged BPF (GR-013).
     - "Independently arrived at" (GR-001).
     - "Costly and complicated" as a fake verbatim quote (GR-005).
  5. **seL4 AArch64 (GR-020).** The proofs cover the EL2 hypervisor configuration on listed boards only. There is no binary proof, MCS, SMP, SMMU or boot coverage, and QEMU `virt` is not a verified platform. The seL4 FAQ is stale on this.
- DM cross-check contradictions:
  - GR-017 vs DM-090 (tradeoff): resolved by the downgrade.
  - GR-018 vs DM-018: L4 designers on synchronous IPC across cores. Now cross-referenced.
  - GR-005 vs DM-001: Drepper's dissent. Now included.
  - GR-009 is now consistent with DM-025 and DM-026 (production-server disablement, buffer ownership).
