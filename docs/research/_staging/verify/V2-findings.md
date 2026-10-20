# V2 findings (DM-016 – DM-030)

## Items

DM-016: FIXED — security-descriptor structure was cited to S225 (tokens only); recited to S226/S227. Token description made accurate (primary vs impersonation token). "AppContainer adds per-app SIDs" was unsourced, so it is replaced by what S226 says. S228 covers registry virtualization; wording now follows it ("interim" technology, apps typically ran as admin). The claim that Unix mode bits give "fewer misconfigurations" is not in S227 and is now marked as unmeasured.
DM-017: FIXED + DOWNGRADED — Classification: right-at-the-time-wrong-now → mistake. S303 (Brown) calls the original design "fatally flawed" from the start: one-shot re-arm races existed before threads, and the changed constraints only made it worse. The steelman now credits fault delivery as sound. "Any error is a memory-corruption bug" softened to corrupt-or-deadlock. Real-time signal queue and signalfd/exec wording corrected to match S302/S303. The "Windows has no general signals" claim is marked unverified.
DM-018: DOWNGRADED — Classification: contested → tradeoff. The primary sources (S304, S305, S306) agree Mach's IPC slowness was not inherent; the real dispute (is microkernel isolation worth ~5%) is DM-046. Mach kernel-call numbers corrected to S304: 18 µs ≈ 900 cycles, of which ≥800 are kernel overhead. "Out-of-line memory/port rights" and the XNU claims are not in the cited sources and are marked unverified.
DM-019: FIXED — "limits differ per OS" corrected: S313 says the parameter names are consistent and the setting methods vary. The POSIX-alternatives detail (path names, fds) is not in S309 and is marked. key_t is attributed to S310.
DM-020: FIXED — added Snover's own concession for the steelman (S317). The CVE-2013-7085 example is labelled as coming from a weak source (S318). The pro-text "values" are no longer attributed to S316 as if it stated them.
DM-021: FIXED — S323 shows only a delay; "abandoned" now rests on S326 ("the sad story of kdbus"). The binder-vs-bus1 argument in S326 comes from LWN reader comments only, and is now labelled weak. dbus-broker wording now follows S327 (it does not say "faster" or name its authors). Biederman/pull-request quotes marked as quoted within S322.
DM-022: FIXED — the 253-fd cap gets its pre-2.6.38 value. "Truncation closes excess fds" corrected: the man page says the list is truncated; fds over RLIMIT_NOFILE are the ones closed. CVE-2021-0920 history made exact per S330/S331 (2016 thread, patch not applied, in the wild from Nov 2020, CISA KEV). "Tarjan" does not appear in the cited commits, so now "dead SCCs over a graph of inflight sockets". Zircon wording follows S337/S338. "BSD 4.2-era" marked unverified.
DM-023: FIXED — dates added (epoll 2.5.44/2002 per S403; kqueue FreeBSD 4.1 per S402). The illogical "reasonable for its time: its cost grows…" sentence is repaired. The multithreading-design and library claims rest on Majkowski's blog (weak), and are now labelled. The close-cost claim is reworded to what S401 says.
DM-024: FIXED — the fsync claim made exact per S406 (async fsync did not work at all). "Every event loop kept a thread pool" is now attributed to S410's wording. "NT designed overlapped I/O in from the start" is not in S417 and is softened. The Torvalds "subtle and nasty bugs" quote is about io_uring, quoted in Axboe's slides (S411), not about AIO in general; attribution corrected.
DM-025: FIXED — S416 does not say seccomp "sees only io_uring_enter"; it now uses S416's wording ("no visibility", "almost certainly" bypass). The "second syscall table" claim is not in S410 (2019), so it is marked as our characterisation. Post/merge dates made exact per S411. The "a bit of pain" quote is restored to its context (usability of cBPF), and the unsupported "costs performance" is dropped.
DM-026: FIXED — the memory-per-idle-connection point is tied to S420. The wepoll claim is narrowed to what its README says (impractical for cross-platform software); labelled weak. The contested framing is kept: both sides are sourced, though the pro-readiness side rests mostly on weak practitioner sources (S419, S421).
DM-027: DOWNGRADED — Classification: right-at-the-time-wrong-now → tradeoff. Gabriel (S422) documents a deliberate implementation-vs-interface tradeoff that was disputed at the time. The alternative's cost (clean unwind or re-entry on every blocking path, S427) has not gone away, so no changed constraint is shown. The Go 1.14 quote is scoped to syscall-package users per S424, and the S426 quote is made exact. The NT alertable-APC claim was [unverified]; it is now sourced to new S2100.
DM-028: FIXED — "cancellation logic is a large source of bugs [S430]" is not in S430. It now says what S430 does: cancel routines are required; Microsoft steers drivers to cancel-safe queues and KMDF. The bug claim is marked as inference. Everything else verified (2.6.25, intr ignored, soft→silent corruption, Cox, Kirby's report).
DM-029: DOWNGRADED — Confidence: high → medium. The steelman ("disk treated as fast", select rationale) is unsourced and is now marked; S427 is added for the shared assumption. S432 corrections: Samba's read-first/thread-fallback was a proposed pattern, not existing behaviour. mincore is not mentioned; fincore is described as racy. RWF_NOWAIT wording now matches S433.
DM-030: FIXED — S503 (LWN news item) does not contain the fork/overcommit point in its body, and its comments could not be read. The claim is replaced by S100 (Baumann et al., HotOS'19), a primary source that makes it directly. The S501 knob list wrongly included oom_score_adj, which postdates the 2009 article; corrected to oom_adj, a cgroup OOM controller and name-based policies. "Child memory added" made exact (half of each non-sharing child). "OOM killer often acts too late" and "server fleets" are not supported; the sentence is rewritten to what S506/S504 say.

## Source directives

SRC S2100: new — [S2100] Microsoft, Asynchronous Procedure Calls, Microsoft Learn (Win32 docs), 2018 (updated 2025), https://learn.microsoft.com/en-us/windows/win32/sync/asynchronous-procedure-calls, primary, checked 2026-10-20 | supports: DM-027 (user-mode APCs run only when the thread enters an alertable wait; listed alertable wait functions)
SRC S100: note — also supports: DM-030 (fork encourages overcommit; conservative accounting would need double the virtual size; Linux overcommits by default)
SRC S503: unverified — article body does not mention overcommit or fork/ENOMEM accounting; reader comments not readable anonymously; citation removed from DM-030
SRC S225: note — covers access-token contents only, not security-descriptor structure (DM-016 now cites S226/S227 for that)
SRC S228: note — covers registry virtualization (calls it "interim"); file virtualization is not described in this page
SRC S303: note — also supports DM-017 (original design "fatally flawed" from the start; one-shot re-arm races; realtime queues limited so signals can be lost; signalfd 2.6.22/2007; synchronous faults handled well by original design)
SRC S305: note — supports Mach ports buffering only by contrast ("Unlike Mach ports, IPC endpoints do not provide any buffering"); says nothing about out-of-line memory
SRC S322: note — Biederman's "wrong" remark and the pull-request "almost unfixable races" text appear as quoted material inside Kroah-Hartman's reply
SRC S323: note — supports only the delay of the kdbus merge by one cycle, not its abandonment
SRC S326: note — the binder accounting/multicast criticism and the "proven on billions of devices" reply are in reader comments, not the article body (weak); the body supports bus1's 2026 Rust reimplementation and "the sad story of kdbus"
SRC S327: note — wiki states dbus-broker aims at high performance and reliability; does not state its authors or measured speed-up
SRC S336: note — commit message describes dead-SCC collection over a graph of inflight sockets; the word "Tarjan" does not appear in S335/S336
SRC S404: note — supports EPOLLEXCLUSIVE since kernel 4.5 and "not originally designed for balancing load" (weak, blog)
SRC S411: note — the Torvalds quotes in these slides concern io_uring (2019), not legacy AIO
SRC S416: note — article dated 2026-01-19; does not say seccomp sees only io_uring_enter; says seccomp has no visibility into io_uring operations
SRC S417: note — does not state that overlapped I/O was present from NT's first release
SRC S430: note — does not characterise cancellation logic as a source of bugs; recommends cancel-safe IRP queues / KMDF
SRC S432: note — Samba read-first/thread-fallback is described as a desired pattern (Lendecke), not existing behaviour; only fincore() (not mincore) is discussed, as racy/indirect
SRC S501: note — 2009 article; mentions oom_adj (not oom_score_adj, which postdates it); half of each non-sharing child's memory added to parent's score
SRC S308: unverified — fetch still fails (connection error) on 2026-10-20

## Summary

Counts (15 items): PASS 0; FIXED 11 (DM-016, 019, 020, 021, 022, 023, 024, 025, 026, 028, 030); DOWNGRADED 4 (DM-017, DM-018, DM-027, DM-029). Some downgraded items also had text fixes. FLAG 0.

Classification changes:
- DM-017: right-at-the-time-wrong-now → mistake
- DM-018: contested → tradeoff
- DM-027: right-at-the-time-wrong-now → tradeoff
- DM-029: confidence high → medium; label kept

All items still have ≥2 verified sources with ≥1 primary.

Most serious problems:
1. Several claims were attributed to sources that do not contain them: oom_score_adj in a 2009 article; "seccomp sees only io_uring_enter"; "Tarjan"; cancellation "a large source of bugs"; Samba behaviour described as existing; S503's overcommit point.
2. Contested-entry evidence that came only from LWN reader comments was presented as the article or authors' position (S326).
3. The Torvalds io_uring quote was used as if it were about AIO.
4. The right-at-the-time-wrong-now label was applied without showing changed constraints (DM-017, DM-027).
