# V6 verification findings (DM-076 – DM-090)

## Items

DM-076: FIXED — macOS Mojave change narrowed to wildcard binds (specific-address binds still need root per S448); socket-activation claim now sourced to S233 instead of [unverified]; Docker wording ("safe operations") matched to S449.
DM-077: FIXED — EPIPE is returned when the signal is ignored or handled, not only ignored; unsourced "most 1970s filters did not check" claim softened and marked [unverified]; "libraries cannot safely change" re-tied to what S451 says (process attribute affecting all threads); Rust "undocumented" scoped to when the issue was filed.
DM-078: FIXED — IX figures corrected (3.6x is key-value store throughput, >2x tail latency; S457); Arrakis figure scoped to NoSQL store; AF_XDP zero-copy was work-in-progress per the 2018 XDP paper (S460), not delivered; SR-IOV/hardware-queue cost re-cited to S456/S457; added Cloudflare's own partial bypass for balance (S461); 1980s "only place" claim marked [unverified].
DM-079: FIXED — minor: 1986 paper describes X10; steelman reworded to what S835 says (accidental vs hostile interference); XACE "unused" attributed to Rutkowska's follow-up comment (S831); "implementation-dependent" wording matched to S826.
DM-080: FIXED — "slow because of synchronous round trips" not in S833; replaced with Stone's actual claim (middleman worst case over Internet; SHM/DRI2 not network-capable); added the counter-view from S833 discussion that per-app remoting remains legitimate.
DM-081: FIXED — Chrome timeline corrected per S840: renderer lockdown ~2 years (incl. DirectWrite); PPAPI/Flash lockdown ~10 months (incl. HDCP); Windows-10-only applied to PPAPI lockdown; "opt-out mitigations" → opt-in; Forshaw paraphrase tightened.
DM-082: DOWNGRADED — Classification: right-at-the-time-wrong-now → mistake. Paget (S844) dates the class to NT 3.1 (1993) and NT already ran privileged services on the user's desktop (S845); no evidence of a changed constraint. S853 (shatter paper) now verified via Wayback copy and cited for the mechanism.
DM-083: FIXED — minor: "every frame is perfect" attributed as the Wayland motto cited by a Mutter developer (S850); "two years of part-time work" → "almost two years, on and off".
DM-084: FIXED — Governance rules were stale: S852 is an old Android mirror. Current GOVERNANCE.md (new S2500): ext needs 2 ACKs, an experimental `xx` tier was adopted Oct 2024 (so "still being debated" was outdated), NACKs must be technically grounded (Nov 2024). "Mesa-side shipping without consensus" not supported by S856 (only a Mesa merge request); corrected. Added upstream fifo-v1 merge (new S2501) to Position A. Restructured "Why it is a problem" into two positions with sources on each side; steelman "stops one vendor imposing semantics" replaced with the governance doc's own NACK rationale.
DM-085: FIXED — run0 claim made precise per S211 (session inherits the service manager's environment; only TERM etc. copied); Windows "only via API calls" claim (unsourced) reduced to what S235 says.
DM-086: FIXED — "NT team designed the registry to fix these" → Chen's actual wording ("tried to address these concerns"); 50 CVEs scoped to the May 2022–Dec 2023 audit; "transactional updates" → "safe concurrent writes" (what S240 supports); per-package registry [unverified] replaced with new S2502 (MSIX docs), which also supports the uninstall-residue point.
DM-087: FIXED — added caveat that S242's misconfiguration data is format-independent (argues for validation, not against text per se); rollback claim sharpened per S244 (/etc tied to package versions); NixOS costs now sourced from S244 (hard-coded path patching; mutable /etc/passwd, resolv.conf, /var outside model).
DM-088: FIXED — S245 (TC resolution) does not mention a casting vote; that detail now cited to S246 (4:4 tie, chair's casting vote). 2019 GR described as seven proposals plus "further discussion". launchd mention given a source via S233 (systemd mimics launchd socket activation); critic's conflict of interest noted, and his concession that systemd targets real problems added for balance (S249). Socket activation claim made precise (manager creates sockets, passes at exec, S233). Positions split explicitly into A/B.
DM-089: DOWNGRADED — Classification: right-at-the-time-wrong-now → tradeoff; Confidence: high → medium. The steelman's historical constraints are unsourced, and text logs keep real advantages while the binary alternative is still contested (S255, S256). Also fixed: "on-disk format is not a stable interface" → S255's actual wording (code is authoritative; use the C API); "less transparent after corruption" not supported by S255 (which calls the format robust to corruption), replaced by the early-critic objection, labelled weak (S256 comments); kernel.org motivation attributed to the original announcement as quoted by S256 (the design doc later removed the example); RFC 3164 security problems tied to its own security section.
DM-090: FIXED — "OSTree notes that pure block-image replication is rigid" not in S135; replaced with what S135 says. Android fallback wording matched to S134; "Virtual A/B reduces storage cost" is not stated on S134 and was removed (S134 only says Virtual A/B replaced legacy A/B in Android 10).

## Source directives

SRC S2500: new — [S2500] wayland-protocols members, wayland-protocols GOVERNANCE.md (current main branch; history shows experimental-protocols section added 2024-10-31 and NACK restrictions 2024-11-06), freedesktop.org GitLab, 2024–2026, https://gitlab.freedesktop.org/wayland/wayland-protocols/-/raw/main/GOVERNANCE.md, primary, checked 2026-10-20 | supports: DM-084 (current rules: xdg/wp need 3 ACKs, 3 implementations, blocked by any NACK; ext needs 2 ACKs; experimental "xx" tier merged after two-week review unless NACKed; NACKs must be technically grounded and are a last resort against ecosystem harm)
SRC S2501: new — [S2501] wayland-protocols contributors, fifo-v1 staging protocol (commit "fifo-v1: Add new protocol", 2024-10-11; present at tag 1.38, absent at 1.37), freedesktop.org GitLab, 2024, https://gitlab.freedesktop.org/wayland/wayland-protocols/-/raw/1.38/staging/fifo/fifo-v1.xml, primary, checked 2026-10-20 | supports: DM-084 (upstream FIFO protocol merged Oct 2024 and shipped in wayland-protocols 1.38, weeks after frog-protocols launched)
SRC S2502: new — [S2502] Microsoft, Understanding how packaged desktop apps run on Windows (MSIX), Microsoft Learn, 2026 (page date 2026-09-30), https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes, primary, checked 2026-10-20 | supports: DM-086 (per-package registry.dat merged into HKLM\Software; HKCU writes copied to a per-user per-app location and removed on uninstall; uninstallers traditionally cannot clean HKCU)
SRC S831: type=weak
SRC S831: note — blog post (Rutkowska is Qubes's designer, so primary only for the Qubes claims); the xinput keylogging demonstration is reproducible; the XACE "nobody uses it" point is in her 2011-04-25 follow-up comment, citing Dan Walsh's choice of a nested X server.
SRC S853: note — original URL unreachable; full text verified via Wayback Machine copy https://web.archive.org/web/2002/http://security.tombom.co.uk/shatter.html on 2026-10-20; supports DM-082 (EM_SETLIMITTEXT, pasted shellcode, WM_TIMER callback against VirusScan running as LocalSystem)
SRC S852: note — this is an Android mirror of an old GOVERNANCE.md snapshot; the current upstream rules differ (ext needs 2 ACKs, experimental tier added 2024, NACK restrictions); see S2500.
SRC S851: note — wayland.app is a community-run protocol explorer; the protocol text mirrors upstream XML (primary), but the compositor-support table is community-maintained (secondary). Showed Mutter lacking tearing-control as of 2026-10-20.
SRC S857: note — same caveat as S851 (wayland.app); the support table showed Mutter, Weston and Muffin lacking xdg-decoration as of 2026-10-20.
SRC S848: note — LWN rate-limited curl; verified via WebFetch: Jake Edge, "DRI3 and Present", LWN, 2013-10-09.
SRC S846: note — Akamai blocks curl; verified via WebFetch on 2026-10-20 (UAC listed as defense-in-depth; integrity levels not named as a boundary).
SRC S245: note — the announcement (Don Armstrong, 2014-02-11) does not mention the casting vote; that detail comes from S246.
SRC S249: note — author (Laurent Bercot) maintains the competing s6 suite; the page concedes that systemd targets real problems that s6 also addresses.
SRC S256: note — the "no intention to standardize" journal FAQ is quoted by a commenter; the kernel.org quote is in the article body (from the original design document, which later removed the example; see S254 history).
SRC S255: note — says the systemd code is the only authoritative description of the format and asks readers to use the C API; it does not use the phrase "not a stable interface", and it describes the format as robust to corruption.
SRC S254: note — the document's history notes that the kernel.org tampering example was removed on 2011-11-23.
SRC S233: note — also supports DM-076 (socket activation: the manager creates listening sockets and passes them at exec) and DM-088 (systemd's native socket activation mimics launchd).
SRC S240: note — Chen writes that the registry "tried to address these concerns" (INI problems); his read-only-files point is about XML configuration files.
SRC S135: note — does not describe block-image replication as "rigid"; it calls plain replication reliable and predictable, contrasting OSTree's preserved /etc and /var.
SRC S134: note — does not state that Virtual A/B reduces storage; it says only that Virtual A/B replaced legacy A/B in Android 10.
SRC S457: note — the 3.6x figure is the throughput of a key-value store (with >2x lower tail latency), not general throughput over Linux.
SRC S460: note — the 2018 paper describes true zero-copy AF_XDP as ongoing work; it states XDP does not quite match DPDK's peak performance.
SRC S461: note — Cloudflare's post also describes its own partial kernel bypass (a single RX queue) for DDoS filtering.
SRC S244: note — also supports DM-087 costs (hard-coded paths patched per package; /etc/passwd, resolv.conf and /var outside the functional model).
SRC S840: note — the renderer lockdown took about two years (including DirectWrite); the PPAPI/Flash lockdown took almost 10 months and shipped first only on Windows 10.

## Summary

15 items: 0 PASS, 13 FIXED, 2 DOWNGRADED (DM-082, DM-089), 0 FLAG. Every item still has at least 2 verified sources, at least 1 of them primary. Three new sources (S2500–S2502).

Classification changes:
- DM-082: right-at-the-time-wrong-now → mistake. The flaw dates from NT 3.1, with no changed constraint.
- DM-089: right-at-the-time-wrong-now → tradeoff, and confidence high → medium.
- The remaining right-at-the-time-wrong-now labels (DM-076, 079, 080, 081, 083) have sourced constraints that later changed, so they stay.

Most serious problems:
1. DM-084 (contested) described outdated wayland-protocols governance from an old mirror. The "experimental stage still debated" claim was overtaken in Oct–Nov 2024, and "Mesa shipped protocols without consensus" was unsupported. Both positions are now sourced, including the upstream fifo-v1 merge.
2. DM-081: the Chrome win32k lockdown timeline was conflated.
3. DM-078: the IX numbers were misstated, and AF_XDP zero-copy was overstated.
4. Several claims went beyond their source: DM-090 (OSTree "rigid", Virtual A/B storage), DM-089 (journal format wording), DM-080 ("synchronous round trips").
5. DM-088 (contested) is now balanced: the casting vote is correctly attributed, the critic's conflict of interest is noted, and both positions are sourced.
