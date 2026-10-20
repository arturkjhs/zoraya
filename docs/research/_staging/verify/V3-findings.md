# V3 verification findings (DM-031 – DM-045)

All 68 cited URLs were fetched on 2026-10-20 (LWN through the Wayback Machine because of LWN rate limits; Apple docs through their JSON endpoints; PDFs extracted to text). No cited source turned out to be fabricated or unreachable.

## Per-item results

DM-031: FIXED — sealing-for-compositors claim re-cited from S512 to S513, which quotes the 2014 patchset's use case #1 (S512 only mentions graphics drivers). "Corrupted pages persisted silently" narrowed to pages corrupted in memory by pointer bugs. Added a note that MongoDB later reintroduced mmap in a limited role and that S510 is a position paper by buffer-pool advocates. "SIGBUS-handler hacks" changed to "relying on SIGBUS handlers". "POSIX leaves … unspecified" reattributed to the man page wording.
DM-032: FIXED — "(inferred)" removed from the trampoline claim, because S519 states it directly. Added that PaX provided non-executable pages on i386 in 2000 (S517, weak), which shows the hardware constraint was soft. OpenBSD opt-out corrected: it needs PT_OPENBSD_WXNEEDED *and* a wxallowed file system. Apple MAP_JIT single-region and entitlement limits apply only under the Hardened Runtime (S522). Classification kept: S514 gives evidence for the original hardware constraint, and NX is now universal.
DM-033: FIXED — "overhead paid only on affected hardware" is not in S525; replaced with `nopti` boot disable plus [unverified] for default-only-on-affected-CPUs. The E0PD claim "Linux can skip KPTI" overstated S527: the initial patch only states that aim and defers KPTI integration to later patches. Added the 5% typical and 30% loopback-test figures (S526). The speculation-assumption sentence is marked inferred.
DM-034: DOWNGRADED — Classification: right-at-the-time-wrong-now → tradeoff. The "why chosen" was only inferred, and Windows had waitable memory notifications since XP (S507), so there is no evidence the absence was a constraint-driven choice. "Forked by vendors" changed to "customized by device partners" to match S506.
DM-035: FIXED — "They come from 4.3BSD" misread the man page's standards line; brk/sbrk predate it (V7). Added new primary source S2200 (V7 brk(2)), which gives evidence for the PDP-11 segmentation constraint and supports the right-at-the-time label. "About 30% faster" qualified: early speed tests showed this, with less gain in the final version (S529).
DM-036: FIXED — the `/etc/passwd` example is not in S601; removed (S601's examples are library open/close and hard links). "Every serious user (Samba, Ganesha, SQLite, OpenJDK) had to build workarounds" was overstated; S601 says they expressed interest in OFD locks. "Lock families always conflict" corrected to conflicting combinations conflicting even within one process (S600, S601). Steelman now notes S601's pointer to Allison's account that the semantics were codified from an early implementation. Cleanup-on-exit rationale marked inferred.
DM-037: FIXED — OpenBSD's behaviour has since changed (S608); noted in Systems affected. S606 claim "in-memory and on-disk state diverge" replaced with S606's actual wording (page contents and reporting vary; none of the five apps handle it sufficiently). Added a sentence saying the "mistake" label applies to error reporting, while dropping the data is a tradeoff that FreeBSD resolved the other way (S604, S608).
DM-038: FIXED — "Firefox 3 fsync-on-close stalls" changed to "fsync performance problems of Firefox 3.0" (S611 wording). The append-then-rename example is now worded as S612's paper states it (ext3 ordered vs ext4 ordered unless an option is set). The Git case was verified in the full OSDI paper PDF.
DM-039: FIXED (minor) — noted that Porter et al. are proposing their own API when they argue TOCTOU is eliminable "only by changing the API" ("suggests" in the original). All CVE, runc and openat2 facts verified.
DM-040: FIXED — the 1996-onward history and the objection list in S621 relate mainly to the symlink half of the patch; wording narrowed. "Adopted in 2012" changed to "patches were posted in 2012" (merge year not stated in the sources).
DM-041: DOWNGRADED — Classification: right-at-the-time-wrong-now → tradeoff. The constraint (setuid programs trusting a shared tree) is partly self-inflicted and has not gone away. Plan 9 (c. 1995) already had per-process namespaces with no super-user. The entry already records real costs of the alternatives (S156, S161). Wording on S155 aligned ("privileged users", "set-user-ID-root").
DM-042: DOWNGRADED — Classification: right-at-the-time-wrong-now → tradeoff. "Metadata writes were modest" has no source, and S627 shows the cost was already rated Linux's worst I/O problem on ext3 in 2007, so the constraint did not change; copy-on-write and flash make it worse. "Hierarchical-storage decisions" is not in S627; removed. mutt attributed to S628.
DM-043: FIXED — "relative paths limited even with opt-in" overstated S632 (it says relative paths cannot use `\\?\`). "Group Policy" re-cited to S635. "8.3 aliases generated by default on many volumes" is not supported; softened to "may still generate unless disabled". "NT namespace has no such limits" marked inferred. "Git clone fails" changed to "can fail". Classification kept: S634 gives evidence of the DOS 1.0 constraint, which has since changed.
DM-044: PASS
DM-045: FIXED — the Btrfs deprecation reason is completed per S640 (Btrfs became unsuitable as a Ceph backend), with a note that this abandonment says little about transactions in general. Both contested positions remain sourced.

## Source directives

SRC S518: note — The message is from "sauron <unixlabs@noos.fr>" quoting Theo de Raadt; I could not verify the author name "Nicolas Brito". The de Raadt quote is primary (designer). The critic's bypass argument is a forum post and should be weighed as weak.
SRC S510: note — CIDR position paper by authors who advocate DBMS-managed buffer pools; it also reports that MongoDB reintroduced mmap in a limited form in 2020.
SRC S512: note — Supports peer-truncation/trust claims; mentions graphics drivers only generically. The compositor use case is quoted in S513, not here.
SRC S517: note — Also documents that PaX PAGEEXEC gave non-executable pages on i386 in Oct 2000, before OpenBSD W^X. Now also cited by DM-032 for this.
SRC S519: note — Also states that stack trampolines and unmarked assembly may need an executable PT_GNU_STACK (cited by DM-032).
SRC S522: note — MAP_JIT one-region limit and allow-jit entitlement apply only to apps using the Hardened Runtime. Apple silicon enforces per-thread W^X for all apps.
SRC S525: note — Does not say PTI is enabled only on affected CPUs; documents `nopti`/`pti=` boot options.
SRC S527: note — Initial E0PD patch does not yet integrate with KPTI; it states the aim of not using KPTI where E0PD is present.
SRC S529: note — Slides. The 30% speedup is from early speed tests and is stated to be lower in the final version.
SRC S601: note — Does not mention /etc/passwd. Samba/Ganesha/SQLite/OpenJDK are named as interested in OFD locks, not as having built workarounds.
SRC S604: note — Fetched via Wayback Machine (LWN rate-limited); all DM-037 claims confirmed.
SRC S608: note — Living community wiki page; OpenBSD behaviour changed after the original analysis (later fsync calls keep failing while references remain).
SRC S611: note — Ts'o is the ext4 maintainer writing about his own system (primary as designer commentary).
SRC S612: note — The presentation page carries only the abstract. Git and ext3/ext4 ordering details were verified in the paper PDF https://www.usenix.org/system/files/conference/osdi14/osdi14-paper-pillai.pdf
SRC S617: note — The authors propose TxOS; overhead figures are self-reported (OpenSSH/dpkg install ~10%).
SRC S621: note — The 1996–2010 history and the objection list in the posting concern mainly the symlink restriction.
SRC S627: note — Names "some mail tools", tmpwatch and backup tools; mutt is named in S628. No hierarchical-storage mention.
SRC S632: note — Does not mention Group Policy (S635 does). "Relative paths always limited" is stated in the context of the `\\?\` prefix.
SRC S640: note — Deprecation reason is that Btrfs is no longer suitable as a Ceph backend, hence no widespread legitimate use.
SRC S2200: new — [S2200] Bell Telephone Laboratories, brk(2), UNIX Programmer's Manual, Seventh Edition (man.cat-v.org archive), 1979, http://man.cat-v.org/unix_7th/2/brk, primary, checked 2026-10-20 | supports: DM-035 (brk/sbrk present in V7; break rounded to 64 bytes on PDP-11; fails if too many segmentation registers needed)

## Summary

- **Counts (15 items):** 1 pass (DM-044), 11 fixed, 3 downgraded (DM-034, DM-041, DM-042), 0 flagged.
- **Classification changes:** right-at-the-time-wrong-now → tradeoff for DM-034, DM-041 and DM-042. In each case there was no evidence that the original choice was constraint-driven, or that the constraint later changed. The label was kept for DM-032, DM-035 and DM-043, where primary sources show the historical constraint (S514, S2200, S634). No confidence levels changed. Every item still has at least 2 verified sources with at least 1 primary.
- **Most serious problems:**
  - **Details not in the sources:** `/etc/passwd` (DM-036), hierarchical storage (DM-042), Group Policy cited to the wrong source (DM-043), "8.3 aliases generated by default" (DM-043), PTI only on affected hardware (DM-033).
  - **Overstated attributions:** "every serious user built workarounds" (DM-036), "Linux can skip KPTI with E0PD" (DM-033), "sealing aimed at exactly the compositor case" cited to the wrong source (DM-031), and a misread origin for brk (DM-035).
  - **Steelmans:** none was unfair, but several "why chosen" sentences were unsourced inference and are now marked as such.
