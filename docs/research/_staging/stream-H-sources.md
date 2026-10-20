# Stream H sources (S800–S899)

[S800] Linus Åkesson, The TTY demystified, linusakesson.net, 2008, https://www.linusakesson.net/programming/tty/, secondary, checked 2026-10-19
  - supports: DM-069 (line discipline in kernel; cooked/raw mode; "twisty little mess of special cases")
  - supports: DM-070 (session/foreground process group; SIGHUP, SIGTTIN/SIGTTOU, SIGWINCH roles)
[S801] Michael Kerrisk (maintainer), credentials(7), Linux man-pages project, n.d. (current), https://man7.org/linux/man-pages/man7/credentials.7.html, primary, checked 2026-10-19
  - supports: DM-070 (two-level session/process-group hierarchy; controlling terminal acquisition, O_NOCTTY; foreground job, SIGTTIN/SIGTTOU)
[S802] Michael Kerrisk (maintainer), setsid(2), Linux man-pages project, n.d. (current), https://man7.org/linux/man-pages/man2/setsid.2.html, primary, checked 2026-10-19
  - supports: DM-070 (EPERM for group leaders; fork-then-setsid idiom; SIGHUP on hangup and on session-leader exit)
[S803] Kees Cook (committed by Greg Kroah-Hartman), tty: Allow TIOCSTI to be disabled (commit 83efeeeb3d04), Linux kernel git (googlesource mirror), 2022, https://android-kvm.googlesource.com/linux/+/83efeeeb3d04b22aaed1df99bc70a48fe9d22c4d%5E%21, primary, checked 2026-10-19
  - supports: DM-072 (LEGACY_TIOCSTI Kconfig + sysctl, default y; "long history" of privilege escalation; OpenBSD removed, Android filters; earlier LSM-filter proposals stalled)
[S804] Hanno Böck, TTY pushback vulnerabilities / TIOCSTI (oss-security post), Openwall oss-security list, 2023, https://openwall.com/lists/oss-security/2023/03/14/2, primary, checked 2026-10-19
  - supports: DM-072 (su/sudo/bubblewrap affected; pty options in su/sudo are optional; kernel disable is the only real fix)
[S805] MITRE/CVE Program, CVE-2017-5226 (bubblewrap TIOCSTI sandbox escape), CVE record (cveawg API), 2017, https://cveawg.mitre.org/api/cve/CVE-2017-5226, primary, checked 2026-10-19
  - supports: DM-072 (sandboxed process escapes to parent session via TIOCSTI)
[S806] brynet (contributor), On the Insecurity of TIOCSTI, undeadly.org (OpenBSD Journal), 2017, https://undeadly.org/cgi?action=article;sid=20170701132619, secondary, checked 2026-10-19
  - supports: DM-072 (OpenBSD makes TIOCSTI return EIO; csh/mailx/ksh uses removed; de Raadt's rationale; Linux had rejected removal)
[S807] MITRE/CVE Program, CVE-2016-7545 (SELinux policycoreutils sandbox TIOCSTI escape), CVE record (cveawg API), 2017, https://cveawg.mitre.org/api/cve/CVE-2016-7545, primary, checked 2026-10-19
  - supports: DM-072 (sandbox escape via crafted TIOCSTI; fix was "create a new session for sandboxed processes")
  - supports: DM-070 (setsid used as the security fix for a shared controlling terminal)
[S808] Solar Designer, TIOCSTI / su-like programs and pty allocation (oss-security post), Openwall oss-security list, 2017, https://www.openwall.com/lists/oss-security/2017/06/03/9, primary, checked 2026-10-19
  - supports: DM-072 (Alan Cox NAKs of TIOCSTI-disable patch and his reasoning; util-linux CVE-2016-2779 left unfixed; recommendation to allocate new ptys)
  - supports: DM-070 (setsid would break su/runuser)
[S809] Flatpak developers, Flatpak CVE-2023-28100 advisory (TIOCLINUX), Openwall oss-security list, 2023, http://www.openwall.com/lists/oss-security/2023/03/17/1, primary, checked 2026-10-19
  - supports: DM-072 (TIOCLINUX copy/paste gives TIOCSTI-like injection on VTs; blocked by seccomp)
[S810] H D Moore, Terminal Emulator Security Issues (bugtraq post), Bugtraq (MIT archive), 2003, https://diswww.mit.edu/menelaus/bt/29027, primary, checked 2026-10-19
  - supports: DM-071 (title reporting onto command line; Eterm/rxvt screen dump writes files; delivery via logs, wget/curl, banners; affected emulators)
[S811] David Leadbeater, ANSI Terminal security in 2023 and finding 10 CVEs, dgl.cx (researcher's own disclosure write-up), 2023, https://dgl.cx/term, primary, checked 2026-10-19
  - supports: DM-071 (echoback class; DECRQSS bug copied from xterm docs; CVE list across iTerm2, mintty, SwiftTerm, ConEmu, Windows Terminal, urxvt, less; root causes; recommendations incl. mosh)
[S812] MITRE/CVE Program, CVE-2022-45063 (xterm OSC 50 font ops code execution), CVE record (cveawg API), 2022, https://cveawg.mitre.org/api/cve/CVE-2022-45063, primary, checked 2026-10-19
  - supports: DM-071 (concrete reply-into-input code execution with zsh vi mode)
[S813] Ecma International, ECMA-48 Control functions for coded character sets (5th ed.), Ecma International, 1991, https://ecma-international.org/publications-and-standards/standards/ecma-48/, primary, checked 2026-10-19
  - supports: DM-071 (control functions embedded in character-coded data)
[S814] Rich Turner, Windows Command-Line: Introducing the Windows Pseudo Console (ConPTY), Microsoft devblogs (Command Line), 2018, https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/, primary, checked 2026-10-19
  - supports: DM-073 (no PTY on Windows; Console API as IOCTLs via ConDrv; off-screen console screen-scraping and its instability; ConPTY API; no Win7 support)
  - supports: DM-069 (Windows' terminal plumbing as alternative to kernel TTY)
  - supports: DM-071 (out-of-band console API needed ConPTY to be remoted)
[S815] Michael Niksa (miniksa), Classic Console APIs versus Virtual Terminal Sequences, Microsoft Learn, 2018 (updated 2025), https://learn.microsoft.com/en-us/windows/console/classic-vs-vt, primary, checked 2026-10-19
  - supports: DM-073 (recommendation to move to VT; remoting; "wrong-way verbs" crossing privilege; direct window handle fragility; no plan to remove; pseudoconsole translation imperfect)
  - supports: DM-071 (VT advantages: carried over any byte channel; in-band)
  - supports: DM-069 (line editing better done in client than in host)
  - supports: DM-070 (Windows console signals via SetConsoleCtrlHandler/GenerateConsoleCtrlEvent)
[S816] Rich Turner, Windows Command-Line: Inside the Windows Console, Microsoft devblogs (Command Line), 2018, https://devblogs.microsoft.com/commandline/windows-command-line-inside-the-windows-console/, primary, checked 2026-10-19
  - supports: DM-073 (history: CSRSS, conhost/condrv in Win7 for security and reliability; object vs text philosophy; compatibility constraints; UCS-2)
  - supports: DM-069 (console host in user mode)
  - supports: DM-071 (out-of-band typed API as the alternative to in-band VT)
[S817] MITRE/CVE Program, CVE-2014-0196 (n_tty_write race), CVE record (cveawg API), 2014, https://cveawg.mitre.org/api/cve/CVE-2014-0196, primary, checked 2026-10-19
  - supports: DM-069 (line-discipline race giving memory corruption / privilege escalation; in CISA KEV)
[S818] Greg Kroah-Hartman, tty: ldisc: add sysctl to prevent autoloading of ldiscs (patch posting), kernel-hardening list (Openwall archive), 2019, https://openwall.com/lists/kernel-hardening/2019/01/21/6, primary, checked 2026-10-19
  - supports: DM-069 (any user can trigger loading of any ldisc module; default kept y for compatibility)
[S819] MITRE/CVE Program (Red Hat CNA), CVE-2023-6546 (n_gsm use-after-free), CVE record (cveawg API), 2023, https://cveawg.mitre.org/api/cve/CVE-2023-6546, primary, checked 2026-10-19
  - supports: DM-069 (obscure line discipline reachable by unprivileged user; UAF -> privilege escalation)
[S820] Arnd Bergmann, BKL conversion in tty layer (LKML patch series v2, reprinted), LWN.net, 2010, https://lwn.net/Articles/386190, primary, checked 2026-10-19
  - supports: DM-069 (TTY layer's dependence on BKL; second attempt; 54 files touched)
[S821] Linux Magazine staff, Cox Quits as TTY Maintainer, Linux Magazine, 2009, https://www.linux-magazine.com/Online/News/Cox-Quits-as-TTY-Maintainer, secondary, checked 2026-10-19
  - supports: DM-069 (maintainer resignation after TTY regression broke kdesu; compatibility pressure)
[S822] Plan 9 authors (Bell Labs), cons(3), Plan 9 manual (9p.io), n.d., https://9p.io/magic/man2html/3/cons, primary, checked 2026-10-19
  - supports: DM-069 (minimal console line editing; rawon/rawoff via consctl file instead of termios ioctls)
[S823] Plan 9 authors (Bell Labs), rio(4), Plan 9 manual (9p.io), n.d., https://9p.io/magic/man2html/4/rio, primary, checked 2026-10-19
  - supports: DM-069 (per-window cons/consctl served by user-space window system)
  - supports: DM-070 (DEL/hangup become notes to a named note group; no job-control mechanism described)
[S824] Jonathan Corbet, Distributors ponder a systemd change, LWN.net, 2016, https://lwn.net/Articles/690151/, secondary, checked 2026-10-19
  - supports: DM-070 (KillUserProcesses default flip; SIGHUP/nohup no longer sufficient; screen/tmux breakage; distro build-time opt-outs; Poettering's security rationale)
[S825] The Open Group / IEEE, _exit (POSIX.1-2024 System Interfaces), The Open Group Base Specifications Issue 8, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/_exit.html, primary, checked 2026-10-19
  - supports: DM-070 (SIGHUP to foreground pgrp on controlling-process exit; orphaned-pgrp SIGHUP+SIGCONT; SysV vs 4.2BSD history)
[S826] David P. Wiggins, Security Extension Specification, X Consortium (X11R7.7 docs), 1996, https://xorg.freedesktop.org/archive/X11R7.7/doc/xextproto/security.txt, primary, checked 2026-10-19
  - supports: DM-079 (trusted/untrusted clients; goals: stop keyboard theft and image reading; limitations incl. ICCCM focus breakage)
[S827] Eamon F. Walsh (NSA), X Access Control Extension Specification (XACE) v2.2, X.Org, 2009, https://xorg.freedesktop.org/archive/X11R7.7/doc/xorg-docs/Xserver/XACE-Spec.xml, primary, checked 2026-10-19
  - supports: DM-079 (LSM-like hooks; incomplete event-delivery coverage; GLX/DRI not secured; lifecycle problems)
[S828] Wayland project, Wayland FAQ, freedesktop.org, n.d., https://wayland.freedesktop.org/faq.html, primary, checked 2026-10-19
  - supports: DM-080 (remote rendering out of scope; separate remoting server)
  - supports: DM-084 (display server+WM+compositor in one; rendering API "whatever you want"; X as compatibility option)
[S829] Wayland project, Wayland Architecture, freedesktop.org, n.d., https://wayland.freedesktop.org/architecture.html, primary, checked 2026-10-19
  - supports: DM-079 (compositor picks input target from its scenegraph)
  - supports: DM-080 (X server-side rendering vs client-side buffers)
  - supports: DM-083 (clients send damage, compositor schedules page flips)
  - supports: DM-081 (user-space compositor as alternative to in-kernel window manager)
  - supports: DM-082 (compositor, not clients, routes input)
[S830] OpenBSD project, ssh_config(5), OpenBSD manual, n.d. (current), https://man.openbsd.org/ssh_config, primary, checked 2026-10-19
  - supports: DM-079 (keystroke monitoring via trusted forwarding; untrusted mode uses SECURITY extension; 20-minute timeout)
[S831] Joanna Rutkowska, The Linux Security Circus: On GUI isolation, The Invisible Things Lab blog, 2011, https://theinvisiblethings.blogspot.com/2011/04/linux-security-circus-on-gui-isolation.html, secondary, checked 2026-10-19
  - supports: DM-079 (xinput test keylogger demo across users/SELinux; XACE unused in practice; Qubes per-domain isolation)
[S832] XDG Desktop Portal project, XDG Desktop Portal documentation, flatpak.github.io, n.d., https://flatpak.github.io/xdg-desktop-portal/docs/, primary, checked 2026-10-19
  - supports: DM-079 (screenshot/screencast/global shortcuts/input capture/remote desktop moved to mediated portals)
  - supports: DM-084 (desktop-specific backends: GTK, GNOME, KDE, wlroots)
[S833] Nathan Willis, LCA: The ways of Wayland, LWN.net, 2013, https://lwn.net/Articles/536862/, secondary, checked 2026-10-19
  - supports: DM-080 (X server reduced to IPC bus; SHM/DRI2 not network-transparent; poor over Internet; core frozen; VNC-like remoting proposal)
  - supports: DM-083 (X frames assembled from separate requests -> inconsistent output; DRI2 limited)
  - supports: DM-084 (X itself accumulated four input stacks, five display-management extensions, four buffer models)
[S834] Robert W. Scheifler, X Window System Protocol, X Version 11 (X11R7.7), X Consortium / X.Org, 1986–2004, https://xorg.freedesktop.org/archive/X11R7.7/doc/xproto/x11protocol.html, primary, checked 2026-10-19
  - supports: DM-080 (protocol over any reliable byte stream; server-side resources; "mechanism, not policy")
[S835] Robert W. Scheifler, Jim Gettys, The X Window System (pre-publication draft of ACM TOG 5(2) 1986 paper), datamuseum.dk archive, 1986, https://datamuseum.dk/aa/dkuug/b0/b0c8c354e.html, primary, checked 2026-10-19
  - supports: DM-079 (access control only at connection time; per-resource control judged unnecessary)
  - supports: DM-080 (network transparency goal; server-side rendering to save bandwidth; async protocol for 5–50 ms RTT)
  - supports: DM-083 (server not required to retain window contents; 1 MB per full-screen image; clients redraw on exposure)
[S836] Manuel Stoeckl (author), waypipe(1), Arch Linux manual pages, n.d. (current), https://man.archlinux.org/man/waypipe.1.en, primary, checked 2026-10-19
  - supports: DM-080 (Wayland remoting via proxy with lz4/zstd and video encoding; security caveats)
  - supports: DM-084 (does not filter protocols, e.g. compositor screenshot protocols)
[S837] Microsoft, MS Windows NT Kernel-mode User and GDI White Paper, Microsoft TechNet (archived on Microsoft Learn), 1996, https://learn.microsoft.com/en-us/previous-versions//cc750820(v=technet.10)?redirectedfrom=MSDN, primary, checked 2026-10-19
  - supports: DM-081 (NT 4.0 moved USER/GDI/drivers to kernel; 60–70 µs vs 4–5 µs transitions; 256 KB–1 MB savings; CSRSS crash already fatal; claimed no security change)
[S838] Microsoft, PROCESS_MITIGATION_SYSTEM_CALL_DISABLE_POLICY structure, Microsoft Learn, 2018, https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-process_mitigation_system_call_disable_policy, primary, checked 2026-10-19
  - supports: DM-081 (DisallowWin32kSystemCalls; Windows 8 minimum)
[S839] Chromium authors, Sandbox (design document, docs/design/sandbox.md), Chromium source, n.d. (current), https://chromium.googlesource.com/chromium/src/+/main/docs/design/sandbox.md, primary, checked 2026-10-19
  - supports: DM-081 (renderers use Win32k lockdown; "significantly reduces the kernel attack surface")
[S840] James Forshaw, Breaking the Chain, Google Project Zero blog, 2016, https://projectzero.google/2016/11/breaking-chain.html, primary, checked 2026-10-19
  - supports: DM-081 (win32k as sandbox-escape surface; Hacking Team chain; DirectWrite/font/HDCP rework; ~10 months; Win10-only due to Win8.1 instability; Edge Win32k filter)
[S841] MITRE/CVE Program (Microsoft CNA), CVE-2021-1732 (Win32k EoP), CVE record (cveawg API), 2021, https://cveawg.mitre.org/api/cve/CVE-2021-1732, primary, checked 2026-10-19
  - supports: DM-081 (win32k EoP exploited in the wild; in CISA KEV)
[S842] Microsoft, ChangeWindowMessageFilterEx function, Microsoft Learn, 2018 (updated 2025), https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-changewindowmessagefilterex, primary, checked 2026-10-19
  - supports: DM-082 (UIPI blocks lower-IL senders; MSGFLT_ALLOW opt-out; messages < WM_USER always pass)
[S843] Microsoft, Using the UIPIEnableCustomMsgs Fix, Microsoft Learn (archived, Windows 7), 2010, https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-7/dd638320(v=ws.10), primary, checked 2026-10-19
  - supports: DM-082 (UIPI filter semantics; compatibility shim to re-open messages; advice to use other IPC)
[S844] Chris Paget, Shatter attacks announcement (bugtraq post), Bugtraq (MIT archive), 2002, https://diswww.mit.edu/menelaus/bt/26552, primary, checked 2026-10-19
  - supports: DM-082 (new attack class against Win32 messaging; present since NT 3.1; claimed unfixable by patch)
[S845] Microsoft, Impact of Session 0 Isolation on Services and Drivers in Windows, Microsoft Learn (archived), 2006 (page 2017), https://learn.microsoft.com/en-us/previous-versions/windows/hardware/design/dn653293(v=vs.85), primary, checked 2026-10-19
  - supports: DM-082 (pre-Vista services shared Session 0 with apps: security risk; Vista isolates services)
[S846] Microsoft Security Response Center, Microsoft Security Servicing Criteria for Windows, Microsoft, n.d. (current), https://www.microsoft.com/en-us/msrc/windows-security-servicing-criteria, primary, checked 2026-10-19
  - supports: DM-082 (UAC is defense-in-depth, not a serviced boundary; integrity levels not listed as boundaries)
[S847] Keith Packard, Implementing Vblank Synchronization in the Present Extension, keithp.com, 2013, https://keithp.com/blogs/present_sync/, primary, checked 2026-10-19
  - supports: DM-083 (Present design accepts occasional tearing to avoid blocking)
[S848] Jake Edge, DRI3 and Present, LWN.net, 2013, https://lwn.net/Articles/569265, secondary, checked 2026-10-19
  - supports: DM-083 (Present as VBLANK-synchronized retrofit; DRI2 pain)
[S849] Wayland project, Wayland protocol specification, Appendix A (wl_surface), freedesktop.org, n.d. (current), https://wayland.freedesktop.org/docs/html/apa.html, primary, checked 2026-10-19
  - supports: DM-083 (pending/double-buffered state applied on commit; frame callbacks)
[S850] Michel Dänzer, Ensuring steady frame rates with GPU-intensive clients, GNOME Shell & Mutter developers blog, 2023, https://blogs.gnome.org/shell-dev/?author=423, primary, checked 2026-10-19
  - supports: DM-083 ("every frame is perfect" motto; transactions; GPU-late buffers cause missed refresh; ~2 years to fix in Mutter)
[S851] Xaver Hugl (protocol author); wayland.app (renderer), tearing-control-v1 protocol, wayland.app, 2021 (protocol), https://wayland.app/protocols/tearing-control-v1, primary, checked 2026-10-19
  - supports: DM-083 (opt-in async/tearing for latency; compositor may ignore)
  - supports: DM-084 (support table: listed in KWin, Sway, Weston etc., not Mutter — support table is the site's compilation, treat as secondary)
[S852] wayland-protocols project, wayland-protocols GOVERNANCE.md (AOSP mirror, commit 8763980d), android.googlesource.com, n.d., https://android.googlesource.com/platform/external/wayland-protocols/+/8763980daa8a599e5735e32656824874862fb757/freedesktop.org/GOVERNANCE.md, primary, checked 2026-10-19
  - supports: DM-084 (project membership; wp/xdg/ext namespaces; 3 ACKs and single-NACK veto for xdg/wp; implementation requirements)
[S853] Chris Paget, Shatter Attacks – How to break Windows (white paper), security.tombom.co.uk, 2002, http://security.tombom.co.uk/shatter.html, primary, checked 2026-10-20, unverified (host no longer resolves; archive.org not reachable by this agent)
  - supports: DM-082 (technical mechanism of shatter attacks; not fetched — web.archive.org unavailable to this agent)
[S854] MITRE/CVE Program, CVE-2002-1230 (NetDDE Agent WM_COPYDATA + WM_TIMER shatter attack), CVE record (cveawg API), 2002 (published 2004), https://cveawg.mitre.org/api/cve/CVE-2002-1230, primary, checked 2026-10-20
  - supports: DM-082 (concrete shatter mechanism: WM_COPYDATA then WM_TIMER -> code exec as LocalSystem; MS02-071)
[S855] Joshua Ashton (misyltoad), frog-protocols README, GitHub, 2024, https://github.com/misyltoad/frog-protocols, primary, checked 2026-10-20
  - supports: DM-084 (protocols wait "months, to years"; parallel fast-iteration repository; frog-fifo-v1 for VSync/GPU starvation)
[S856] gihyo.jp Linux Daily Topics, Frog Protocols announcement report (Japanese), gihyo.jp, 2024, https://gihyo.jp/article/2024/10/daily-linux-241003, secondary, checked 2026-10-20
  - supports: DM-084 (launch 2024-09-24 by Valve engineer; Blumenkrantz "experimental" stage proposal; objections to bypassing upstream consensus)
[S857] wayland-protocols project; wayland.app (renderer), xdg-decoration-unstable-v1 protocol, wayland.app, n.d., https://wayland.app/protocols/xdg-decoration-unstable-v1, primary, checked 2026-10-20
  - supports: DM-084 (compositor may override client's decoration mode; still unstable; support table: not in Mutter/Weston — table is the site's compilation, treat as secondary)
[S858] Jonathan S. Shapiro, John Vanderburgh, Eric Northup, David Chizmadia, Design of the EROS Trusted Window System (abstract page), USENIX Security Symposium, 2004, https://www.usenix.org/legacy/event/sec04/tech/shapiro.html, primary, checked 2026-10-20
  - supports: DM-079 (window system is an IPC/trust mediator; X11 and Windows inherit Alto-era trust assumptions; EWS < 4,500 lines, ~10x smaller than Trusted X)
  - supports: DM-082 (window system as IPC mechanism relevant to security policy)
  - note: full PDF (https://usenix.org/event/sec04/tech/full_papers/shapiro/shapiro.pdf) was fetched but could not be text-extracted; only abstract-level claims are used.
