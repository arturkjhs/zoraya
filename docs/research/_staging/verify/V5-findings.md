# V5 findings (DM-061 – DM-075)

All 72 cited sources in the slice were fetched on 2026-10-20 (LWN pages S553, S820 and S824 needed a retry after HTTP 429). Each resolves to the stated work.

## Per-item results

DM-061: FIXED — corrected Pathfinder wording: Wind River left PI out of the default; JPL fixed it by patching a global configuration variable; reproduced in under 18 h. Chain-depth sentence now matches S359 (depth depends on all lock nesting). "Lock-free designs [S361]" replaced with Yodaiken's actual options (short atomic sections, contention removal, priority-ordered server). Locke's position now worded as "not new and generally correct", and his ceiling-emulation cost (priority change on every acquisition) added. Added a note that both authors were executives of competing RT-Linux vendors. Resolved the seL4 donation [unverified] with S2401.
DM-062: FIXED — Torvalds paraphrase softened ("hard to apply to", not "would not fit"). Rust [unverified] resolved: Rust atomics follow C++20 and omit "consume" [S2400]. "Formal models need specialist maintainers [S364]" was weakly supported; now cites S365 (model is its maintainers' collective opinion, not an oracle).
DM-063: FIXED — the FILETIME claim was cited to S541, which does not say it; now cites S2402. The "distributions treat it as a mass transition" claim was cited to S532, which does not say it; now cites the Debian t64 page S2403 (about 495 libraries changed ABI, about 5–6k packages rebuilt). Classification RATWN kept: the 1970s constraint is plausible and the change is documented.
DM-064: FIXED — the Cloudflare incident is now described as a negative time difference, as in S537. Smear list made specific (UTC-SLS 1,000 s, Bloomberg 2,000 s, Meinberg cosine) per S534. "Mixed smears disagree by 0.5 s" was an inference; it now says each smear is up to ~0.5 s from UTC and Google asks for one common smear. CGPM wording aligned with S535.
DM-065: FIXED — wrong claim: "CLOCK_MONOTONIC appeared as an option in POSIX.1-1996". S538 only dates `clock_gettime` to POSIX.1-1996 and says MONOTONIC became mandatory in 2024; reworded. The Go 29% figure now says "estimated uses that are subtractions/comparisons". Go 1.9 shipping date sourced (S2404). Go rationale quoted accurately (behave the same 99% of the time; leap second hits every machine at once). The "Arm Generic Timer gives a fixed-frequency counter [S541]" claim is not in S541; reworded to what S541 says (Windows bases QPC on the Generic Timer system counter). Classification `mistake` kept.
DM-066: FIXED — the "Single zone" bullet and "thread-safe variants only as extensions" now say only what NetBSD S546 states (setting TZ changes the whole program; only `tzset` is listed as POSIX-conforming). Chrono advisory wording aligned with S544.
DM-067: FIXED — the mainframe figure is hypothetical in S547 ("might find"); wording changed. Added from S547 that 1990s/early-2000s Linux could not omit ticks. Housekeeping wording now "at least one", and residual tick "about once per second".
DM-068: FIXED — both open [unverified] items resolved. O(1) history (merged in 2.5 development to fix 2.4 scalability; its interactivity estimator was repeatedly patched; RSDL, then CFS) is sourced to S2405 and S2407. sched_ext merged in 6.12 (released 17 Nov 2024), which is also when the EEVDF transition was completed [S2406]. Zijlstra's objection is now sourced (S2408: Torvalds had NAKed loadable schedulers before; this was that again plus BPF). "Torvalds planned to merge over the maintainers' objections" was reworded to what S554 shows: Torvalds planned to merge for 6.11 and Gleixner asked for three months. Classification `contested` kept, now with sourced arguments on both sides.
DM-069: FIXED — the ldisc autoload switch is now a 2019 patch posting [S818] that exists as `CONFIG_LDISC_AUTOLOAD`, default y [S803]. "Repeated attempts" at BKL removal changed to "more than one" (S820 says second attempt). Added that Plan 9 has little field experience (clean-design bias).
DM-070: FIXED — factual error. On hangup, SIGHUP goes to the session leader. It goes to the foreground group when the controlling process exits (S154). The rio "note group" claim was narrowed to what rio(4) says (notes go to an explicitly given pid). Poettering's security rationale for cgroup cleanup was added for balance [S824].
DM-071: FIXED — "about ten more RCE-class CVEs" changed to "about ten CVEs, several allowing code execution" (not all were RCE).
DM-072: FIXED — Linux 6.2 dating: committed 2022, released 2023, citing S804 for the 6.2 version. Otherwise verified (Cox's NAK reasoning, util-linux CVE-2016-2779, OpenBSD EIO, Flatpak TIOCLINUX).
DM-073: FIXED — "The API predates Unicode" is not supported (S816 says only that Windows uses UCS-2 internally); reworded to "built around UCS-2 (BMP only)". The ConPTY availability claim now matches S814 (first shipped autumn 2018 in Windows 10; not on Windows 7). The cross-reference to DM-072 now notes that Microsoft's page claims VT platforms do not allow wrong-way operations. TIOCSTI shows they do, but the attribution is kept clean.
DM-074: FIXED — "silently truncated" was wrong: per S437 the caller sees a larger returned length. TAPS "not to replace them" contradicted S434, which calls TAPS an enhanced replacement designed to coexist. The cost sentence now reflects S434's own "allow access to specialized features" requirement.
DM-075: DOWNGRADED — Classification: right-at-the-time-wrong-now → tradeoff. The steelman for in-process NSS (no resolver daemon, per-site configuration) still applies today, and the entry's own "Cost of the alternatives" names real costs of a resolver service, so the constraints have not gone away. Also FIXED: the CVE-2025-4802 range (introduced 2.27, fixed in 2.39; it had said "2.27 to 2.39"); the Go caps are concurrent lookups; macOS wording now matches S440 ("do not let programs make direct DNS requests").

## Source directives

SRC S362: note — guest editorial by TimeSys VP of Technology (a commercial competitor of FSMLabs); page 1 carries the "not new, generally correct" and "strawman" passages
SRC S361: note — author is FSMLabs (RTLinux vendor); argues from a vendor position
SRC S541: note — does not support the FILETIME (1601, 100 ns) claim previously attributed to it in DM-063; that claim is now cited to S2402
SRC S532: note — does not mention distribution-wide ABI transitions; DM-063 now cites S2403 for that
SRC S538: note — dates clock_gettime (not CLOCK_MONOTONIC specifically) to POSIX.1-1996; MONOTONIC mandatory only in POSIX.1-2024
SRC S547: note — the 1,500-guest mainframe figure is phrased hypothetically ("might find")
SRC S554: note — lore.jethome.com is a mirror of lore.kernel.org; message shows Torvalds planning to merge for 6.11 and Gleixner requesting three months; sched_ext actually landed in 6.12 (see S2406)
SRC S803: note — the same diff shows CONFIG_LDISC_AUTOLOAD (default y); also supports DM-069 (ldisc autoload switch exists, default on)
SRC S443: note — Solar Designer's reply quoting Carlos O'Donell's glibc advisory; vulnerable commit in 2.27, fix commit in 2.39
SRC S810: note — Bugtraq archive is quoted-printable encoded; content readable after decoding
SRC S2400: new — [S2400] Rust project, Module std::sync::atomic (Memory model for atomic accesses), doc.rust-lang.org, current, https://doc.rust-lang.org/std/sync/atomic/index.html, primary, checked 2026-10-20 | supports: DM-062 (Rust atomics follow C++20 rules, without the "consume" ordering)
SRC S2401: new — [S2401] seL4 Foundation, MCS Extensions tutorial, docs.sel4.systems, current, https://docs.sel4.systems/Tutorials/mcs.html, primary, checked 2026-10-20 | supports: DM-061 (passive servers run on clients' scheduling contexts, donated on seL4_Call)
SRC S2402: new — [S2402] Microsoft, FILETIME structure (minwinbase.h), Microsoft Learn (Win32 API), current, https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ns-minwinbase-filetime, primary, checked 2026-10-20 | supports: DM-063 (64-bit count of 100 ns intervals since 1 January 1601 UTC)
SRC S2403: new — [S2403] Debian project, ReleaseGoals/64bit-time, Debian Wiki, 2023–2024, https://wiki.debian.org/ReleaseGoals/64bit-time, primary, checked 2026-10-20 | supports: DM-063 (t64 ABI transition settled by June 2024; ~495 library packages change ABI; ~5,063–5,975 dependent packages rebuilt)
SRC S2404: new — [S2404] Go project, Go 1.9 Release Notes, go.dev, 2017, https://go.dev/doc/go1.9, primary, checked 2026-10-20 | supports: DM-065 (Go 1.9 adds transparent monotonic time support)
SRC S2405: new — [S2405] KernelNewbies, Linux 2.6.23 (CFS process scheduler section), kernelnewbies.org, 2007, https://kernelnewbies.org/Linux_2_6_23, secondary, checked 2026-10-20 | supports: DM-068 (O(1) scheduler by Molnar merged during 2.5 to fix 2.4 scalability; interactivity-estimator corner cases; Kolivas's RSDL; CFS chosen over RSDL amid controversy)
SRC S2406: new — [S2406] KernelNewbies, Linux 6.12, kernelnewbies.org, 2024, https://kernelnewbies.org/Linux_6.12, secondary, checked 2026-10-20 | supports: DM-068 (6.12 released 17 Nov 2024; first pieces of sched_ext merged; EEVDF transition completed)
SRC S2407: new — [S2407] Linux kernel developers, CFS Scheduler (Documentation/scheduler/sched-design-CFS), kernel.org, current, https://docs.kernel.org/scheduler/sched-design-CFS.html, primary, checked 2026-10-20 | supports: DM-068 (CFS by Molnar merged in 2.6.23, replacing the previous vanilla scheduler's SCHED_OTHER interactivity code; CFS making room for EEVDF)
SRC S2408: new — [S2408] Jonathan Corbet, The extensible scheduler class, LWN.net, 2023, https://lwn.net/Articles/922405/, secondary, checked 2026-10-20 | supports: DM-068 (Zijlstra on first sched_ext posting: Torvalds NAKed loadable schedulers several times; this is that again plus BPF)

## Summary

- Counts: 15 items. PASS 0, FIXED 14, DOWNGRADED 1 (DM-075), FLAG 0.
- Classification changes: DM-075 right-at-the-time-wrong-now → tradeoff. All other classifications were kept. RATWN was checked for DM-062, 063, 066, 067, 069, 070 and 074; each has a stated period constraint that has since changed.
- Open [unverified] items resolved: DM-068 O(1) history (S2405, S2407) and sched_ext merge version (6.12, S2406); DM-068 Zijlstra's argument (S2408); DM-061 seL4 donation (S2401); DM-062 Rust model (S2400). Remaining [unverified]: DM-066, "languages with explicit zone objects".
- Most serious problems:
  - DM-070: wrong SIGHUP target on hangup.
  - DM-065: wrong POSIX history for CLOCK_MONOTONIC.
  - DM-063: two claims cited to sources that do not contain them (S541, S532).
  - DM-074: misstatements of S437 ("silently truncated") and S434 (TAPS "not to replace").
  - DM-075: wrong CVE version range.
  - DM-073: unsupported "predates Unicode".
  - DM-068: overstated characterisation of the sched_ext dispute.
- Every item still has ≥2 verified sources with ≥1 primary, so no confidence changes were needed.
