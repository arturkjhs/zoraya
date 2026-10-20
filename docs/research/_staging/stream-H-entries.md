# Stream H entries — categories 11 and 13

## 11. Terminals, sessions and consoles

### DM-069: Kernel TTY layer and line disciplines

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Unix/POSIX (termios), Linux, BSDs, macOS/XNU
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** The kernel has a TTY subsystem between a character device (UART, virtual console, pty) and processes. A pluggable *line discipline* (default N_TTY) does canonical-mode line editing, echo, CR/LF translation, flow control (^S/^Q), and turns control characters into signals. Programs configure it through termios ioctls [S800]. Other line disciplines (for example n_gsm for GSM 07.10 multiplexing) can be attached with an ioctl [S819].

**Why it was chosen.** Terminals were teletypes and serial terminals. Handling characters as they arrived, echoing them, and editing lines in the kernel meant every program, even `cat`, got usable interactive input without its own editing code. Pseudo-terminals later reused the same machinery so terminal emulators and remote logins looked like serial lines [S800].

**Why it is a problem.** (1) *Complexity and bugs in privileged code.* Åkesson calls the subsystem "a twisty little mess of special cases" [S800]. CVE-2014-0196 was a race in `n_tty_write` that corrupted memory and could escalate privileges, and it is in CISA's Known Exploited Vulnerabilities catalog [S817]. (2) *Rarely used disciplines are reachable by unprivileged users.* The kernel autoloaded any requested ldisc module for any user. Greg Kroah-Hartman added a switch to stop this in 2019 but left the default on [S818]. CVE-2023-6546, a use-after-free in n_gsm that an unprivileged user can reach, shows the risk [S819]. (3) *Locking.* Removing the Big Kernel Lock from the TTY layer took repeated attempts, and the 2010 series touched 54 files [S820]. In 2009 the maintainer resigned after a TTY regression broke kdesu and Linus Torvalds objected to breaking user space [S821]. (4) *The editing is too weak to be used.* Serious interactive programs switch to raw mode and use readline-style libraries [S800]. Microsoft's console team makes the same point: advanced editing is only possible when the application does it itself [S815].

**Why it was not fixed.** termios and pty semantics are POSIX ABI used by shells, editors, ssh and every terminal emulator. Even the ldisc-autoload restriction defaults to the old behaviour "to not break working systems" [S818].

**Alternatives that exist.** Plan 9 kept only minimal erase/kill editing in `/dev/cons`. Raw mode is switched by writing `rawon` to a `consctl` file instead of using ioctls [S822]. The window system `rio`, a user-space file server, provides each window's `cons`/`consctl` [S823]. Windows hosts the console in user mode: it moved from CSRSS to conhost.exe in Windows 7 "for security and reliability reasons" [S816], and ConPTY added a pseudo-console [S814].

**Cost of the alternatives.** A user-space terminal service has to be on the path of every interactive byte. The Windows design carried its own large problems (see DM-073). Plan 9's model does not provide POSIX termios, so ported software needs an emulation layer.

**Relevance to our OS.** high — the POSIX layer must satisfy `isatty`/termios for compilers and shells, and we choose whether that is a kernel object or a sandboxed user-space service.

**Sources.** [S800], [S814], [S815], [S816], [S817], [S818], [S819], [S820], [S821], [S822], [S823]

### DM-070: Process groups, sessions, controlling terminals, job control, `SIGHUP`

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS/XNU
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Processes form a strict two-level hierarchy: sessions contain process groups (jobs). A session has at most one controlling terminal, and that terminal has one foreground group. Background reads raise SIGTTIN, and background writes raise SIGTTOU when TOSTOP is set. Keyboard signals go to the foreground group [S801]. On hangup or controlling-process exit, the kernel sends SIGHUP to the foreground group. A newly orphaned group with stopped members gets SIGHUP followed by SIGCONT [S802][S825].

**Why it was chosen.** A user had one physical terminal and needed to run, suspend and resume several jobs on it. When a dial-up line hung up, the user's processes had to be cleaned up. Process groups and SIGHUP did both with little kernel state. POSIX standardized a merge of System V (SIGHUP when the controlling process ends) and 4.2BSD (daemon plus `vhangup()`) behaviour [S825].

**Why it is a problem.** The lifetime of a process is tied to the lifetime of a terminal, and detaching takes ritual. The ritual is: fork, let the parent `_exit`, call `setsid` in the child (because `setsid` fails with EPERM for a group leader), and avoid getting a controlling TTY again by using `O_NOCTTY` [S801][S802]. Keeping a job alive after logout depended on ignoring SIGHUP or using `nohup`. When systemd 230 switched to cgroup-based logout cleanup with SIGTERM/SIGKILL, those idioms stopped working. screen and tmux broke, and distributions disabled the feature at build time [S824]. The opposite failure is a security one. Processes that keep the user's controlling terminal across a privilege change can inject input into it (DM-072). The fix for the SELinux sandbox escape was to create a new session [S807], but util-linux could not apply `setsid` to `su` without breaking it [S808]. The orphaned-group rules exist only to rescue stopped jobs that would otherwise "languish forever" [S825]. That is a sign of how much special-casing the model needs.

**Why it was not fixed.** The semantics are in POSIX and every job-control shell relies on them. Systemd's replacement is layered on top and did not remove them [S824].

**Alternatives that exist.** In Plan 9, windows replace job control. rio turns DEL and hangup into *notes* delivered to an explicitly named note group, and it has no foreground/background terminal arbitration [S823]. On Linux, systemd tracks sessions with cgroups instead of terminals [S824]. Windows console control events (`SetConsoleCtrlHandler`/`GenerateConsoleCtrlEvent`) are a separate signalling channel [S815].

**Cost of the alternatives.** Cgroup-based cleanup broke long-standing user workflows and needed an explicit "linger" opt-in [S824]. Plan 9 gives up ^Z-style suspension on a single terminal. Each alternative still needs some "this work belongs to this login" grouping.

**Relevance to our OS.** medium — the POSIX layer must emulate job control for shells, but native process lifetime should probably not depend on a terminal.

**Sources.** [S801], [S802], [S807], [S808], [S815], [S823], [S824], [S825]

### DM-071: In-band escape sequences for terminal control; escape-sequence injection

- **Category:** Terminals, sessions and consoles
- **Systems affected:** all VT/ECMA-48 terminals and emulators (xterm, iTerm2, mintty, rxvt, PuTTY, Windows Terminal, …); every OS that uses them
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Terminal control is done by control functions "embedded in character-coded data" (ECMA-48) [S813]. The terminal parses the program's output stream for ESC/CSI/OSC sequences that move the cursor, set colours and titles, and so on. Some sequences make the terminal *reply* by writing data into the input stream.

**Why it was chosen.** One byte stream carries everything. It works over serial lines, pipes, sockets and ssh without any RPC layer. Microsoft names this remoting property as the main reason it now recommends VT over its own console API [S815].

**Why it is a problem.** Any program that prints untrusted data, such as logs, HTTP responses, filenames or git metadata, passes commands to the terminal. In 2003 H D Moore showed that title-reporting sequences placed attacker text on the command line in xterm, dtterm, rxvt, Eterm and PuTTY, and that Eterm/rxvt "screen dump" could write arbitrary files. The delivery paths were syslog, Apache logs, and wget/curl/ftp output [S810]. Twenty years later Leadbeater found about ten more RCE-class CVEs of the same "echoback" kind [S811]. Examples: DECRQSS replies in iTerm2, mintty and SwiftTerm, where the bug was copied from erroneous xterm documentation; ConEmu title reporting; Windows Terminal OSC 9;9; urxvt input reaching Perl `eval`. CVE-2022-45063 is a concrete case: an xterm OSC 50 reply containing ^G ran commands in zsh vi mode [S812]. The root causes are structural. Parsers deviate from the VT state machine, risky queries are on by default, and the defence ("escape ESC in untrusted output") falls on every program that writes to a terminal [S811].

**Why it was not fixed.** The in-band channel *is* the protocol. Removing replies or sequences breaks terminal-aware software. Fixes are per emulator and per feature, and the same bugs come back [S811].

**Alternatives that exist.** The classic Windows Console API is out-of-band, with typed calls to a console host [S815][S816]. Other mitigations: mosh keeps the terminal state on the server side, tmux keeps `allow-passthrough` off, and emulators can disable reporting by default [S811].

**Cost of the alternatives.** An out-of-band API cannot be carried over a plain byte channel. Windows had to build ConPTY to translate its API back into VT, and the translation is imperfect [S814][S815]. Filtering at the emulator removes features that real applications use.

**Relevance to our OS.** high — our terminal and POSIX layer must speak VT for ported tools, but a native terminal protocol could separate data from control and make "reply into input" a distinct, deniable capability.

**Sources.** [S810], [S811], [S812], [S813], [S814], [S815], [S816]

### DM-072: `TIOCSTI` input injection

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Linux (until opt-out in 6.2), historically all BSD-derived Unix; OpenBSD (disabled 2017)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `TIOCSTI` is an ioctl that pushes a byte into a terminal's input queue as if the user had typed it. Any process with a file descriptor for the terminal can call it on its controlling TTY [S803].

**Why it was chosen.** It made simple tricks cheap: a shell or mail program "typing ahead" for the user. OpenBSD removed such uses from csh, mailx and ksh before disabling the call [S806]. The threat model of the time assumed every process on a terminal belonged to the same user.

**Why it is a problem.** Holding the terminal becomes authority to run commands as whoever owns the shell reading it. A process that keeps the terminal after a privilege change, or after its parent shell takes over again, can type commands into that shell. Examples: sandbox escapes in SELinux `sandbox` (CVE-2016-7545) [S807] and bubblewrap (CVE-2017-5226) [S805], and "TTY pushback" against `su`/`sudo` when they do not allocate a new pty [S804][S808]. The kernel's own Kconfig help now calls it a malicious privilege-escalation mechanism with no meaningful real-world use [S803].

**Why it was not fixed.** Linux kernel developers rejected removal for years. Alan Cox NAKed Matt Brown's 2017 patches. He argued that privilege-changing programs should allocate a new pty anyway, that TIOCSTI is not the only attack path, and that the affected applications were already fixed. Solar Designer disputed the last point [S808]. util-linux shipped CVE-2016-2779 unfixed because `setsid()` would break `su`/`runuser` [S808]. Proposals for general ioctl-filtering LSMs stalled [S803]. Linux 6.2 (2022) finally added `CONFIG_LEGACY_TIOCSTI` and the sysctl `dev.tty.legacy_tiocsti`, but the default is still Y [S803].

**Alternatives that exist.** OpenBSD made TIOCSTI return EIO in 2017 [S806]. Android filtered it [S803]. Sandboxes block it with seccomp or create a new session (setsid) [S807][S809]. `su`/`sudo` can be configured to allocate a new pty [S804].

**Cost of the alternatives.** Some legacy tools lose a feature. Cox's warning was partly borne out: Flatpak's TIOCSTI filter was bypassed through `TIOCLINUX` copy/paste on virtual consoles (CVE-2023-28100) [S809]. Removing one ioctl does not fix a design in which sharing a terminal means sharing authority.

**Relevance to our OS.** high — a clear case for separating rights (write output, read input, inject input) on terminal handles instead of granting all of them through one file descriptor.

**Sources.** [S803], [S804], [S805], [S806], [S807], [S808], [S809]

### DM-073: Windows console API vs VT sequences (ConPTY retrofit)

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Windows NT family
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Windows console programs call a rich Win32 Console API (cursor, buffer, input records, history, aliases). Calls are serialized as IOCTLs through the ConDrv driver to a console host process (CSRSS before Windows 7, conhost.exe afterwards) [S814][S816]. Windows had no PTY. Until 2018 the console host was the only interface to a command-line application [S814].

**Why it was chosen.** NT follows an "everything is an object" philosophy. A typed API is easier to verify than escape sequences, and shared services such as history and aliases did not have to be reimplemented by every shell [S816]. The design dates from about 1989 and assumed a local GUI [S816].

**Why it is a problem.** Third-party terminals and sshd had to create a hidden console, simulate input and scrape its buffer. Microsoft says this caused instability, crashes, data corruption, extra power use and lost formatting [S814]. The API cannot be carried over a byte channel, so remoting needs a whole RPC layer [S815]. Some calls run in the "wrong" direction, injecting input or reading back output. Microsoft names these as a way to cross security and privilege levels, the same class as DM-072 [S815]. Giving console applications raw window handles became fragile as window security tightened [S815]. The API predates Unicode (UCS-2 internally, A/W variants) [S816].

**Why it was not fixed.** "Hundreds of millions" of applications and scripts depend on it [S816]. Microsoft states there are no plans to remove the API [S815].

**Alternatives that exist.** ConPTY (`CreatePseudoConsole`, 2018): conhost translates legacy API calls into VT text for any terminal over pipes, and Microsoft now recommends that new applications emit VT [S814][S815]. Unix-style ptys with in-band VT are the obvious alternative.

**Cost of the alternatives.** ConPTY keeps a simulated screen buffer and projects only its effects, so the translation is "not perfect" [S815]. It is unavailable on Windows 7, so applications keep both code paths [S814]. Moving to VT brings in the escape-injection class (DM-071). This entry and DM-071 are two sides of the same in-band vs out-of-band tradeoff.

**Relevance to our OS.** medium — shows the cost of a terminal model that is not a byte stream when remote access and ported Unix tools matter; a typed protocol needs a planned byte-stream translation.

**Sources.** [S814], [S815], [S816]

## 13. Graphics, input and window systems

### DM-079: X11 security model: any client can read input and contents of others

- **Category:** Graphics, input and window systems
- **Systems affected:** X Window System (X11, Xorg); Linux/BSD desktops on X
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** X checks access only when a client connects (host address or auth cookie). After that, a client can manipulate any resource on the server. Scheifler and Gettys judged per-resource access control unnecessary because accidental interference seemed unlikely [S835]. Any connected client can therefore read the keyboard, capture other windows and send synthetic events.

**Why it was chosen.** In 1986 the clients on a display normally belonged to one user on trusted workstations. Openness let window managers and input managers be ordinary clients that grab input on the root window [S835].

**Why it is a problem.** Rutkowska showed that an unprivileged `xinput test` logs every keystroke, including a root password typed into `su` in another terminal. It works across user accounts and SELinux domains, with no exploit needed, because it is how X is designed [S831]. Screen capture and keystroke injection work the same way. OpenSSH warns that trusted X11 forwarding lets a remote host perform keystroke monitoring [S830]. The EWS authors argue that X11 and Windows inherited Xerox Alto-era trust assumptions [S858].

**Why it was not fixed.** The SECURITY extension (1996) added "untrusted" clients that cannot grab the keyboard, read trusted windows or use others' resources. It also breaks ICCCM focus mechanisms and leaves property access implementation-defined [S826]. ssh uses it for untrusted forwarding, with a 20-minute token expiry [S830]. XACE (NSA, 2006–2009) added LSM-style hooks, but its own spec admits incomplete event-delivery coverage and unsecured GLX/direct rendering [S827]. Rutkowska reported that XACE was not used in practice [S831]. Any real fix breaks window managers, screenshot tools and input methods that depend on the open model.

**Alternatives that exist.** Under Wayland the compositor is the display server. It routes input itself from its scenegraph [S829], so clients do not see other clients' input. Qubes isolates GUI per VM domain [S831]. EROS EWS was a capability-based trusted window system in under 4,500 lines, about 10x smaller than Trusted X [S858].

**Cost of the alternatives.** Screenshots, screencasting, global shortcuts, input capture and remote desktop each need a mediated interface. On Linux these became xdg-desktop-portal D-Bus APIs with desktop-specific backends [S832], which feeds the fragmentation in DM-084. EWS stayed a research system [S858].

**Relevance to our OS.** high — the compositor is our input trust boundary; deciding who may observe or inject input (accessibility, automation, capture) has to be a capability design up front.

**Sources.** [S826], [S827], [S829], [S830], [S831], [S832], [S835], [S858]

### DM-080: X11 network transparency and server-side rendering

- **Category:** Graphics, input and window systems
- **Systems affected:** X Window System
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** X is a protocol over "any reliable byte stream" [S834]. Clients ask the server to draw lines, text and images into server-side resources (windows, pixmaps, GCs, fonts). The server holds the device-specific code [S834][S835].

**Why it was chosen.** The goals were network transparency (an application on one machine displays on another of any architecture), device independence, and "mechanism, not policy" [S835]. High-level drawing requests are compact on the wire. Asynchronous requests hide 5–50 ms round trips. Only the server needs porting to new display hardware [S835]. For time-shared machines and 1980s workstations this was a strong design.

**Why it is a problem.** Rendering moved to the client. Toolkits draw locally with GPUs and hand over finished buffers, which left the X server, in Daniel Stone's words, a "terrible, terrible, terrible" IPC bus between client and compositor [S833]. The fast paths (MIT-SHM, DRI2) are not network-transparent. Remote X over the Internet is slow because of synchronous round trips [S833]. In the X architecture every frame goes through the server twice, from client to server and from compositor back through the server [S829].

**Why it was not fixed.** Backward compatibility froze the core protocol. Features moved to extensions and clients instead. Stone counted four input stacks, five display-management extensions and four buffer models [S833].

**Alternatives that exist.** Wayland leaves remote rendering out of scope and recommends a separate remoting server or a compositor with remoting [S828]. Clients render into their own buffers [S829]. waypipe proxies Wayland like `ssh -X`, compressing buffers with lz4/zstd or video codecs [S836]. Stone proposed VNC-like compositor-to-compositor transport [S833].

**Cost of the alternatives.** Per-application remoting is no longer built in. waypipe is an add-on: it does not filter protocols, is not hardened against malicious peers, and contains "plenty of unsafe Rust" [S836]. Users who relied on `ssh -X` lost a convenience. Classed right-at-the-time-wrong-now rather than mistake because the 1986 constraints (no client GPUs, slow networks, shared hosts) were real [S835].

**Relevance to our OS.** medium — we target local QEMU, but whether remote display is a protocol property or a proxy affects the compositor protocol design.

**Sources.** [S828], [S829], [S833], [S834], [S835], [S836]

### DM-081: Windows win32k: window manager and GDI in kernel mode

- **Category:** Graphics, input and window systems
- **Systems affected:** Windows NT 4.0 and later
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Since NT 4.0 (1996), the Window Manager (USER), GDI and graphics drivers run in kernel mode as executive services (win32k), reached by system call from every GUI thread [S837].

**Why it was chosen.** In NT 3.51 these components ran in CSRSS and were called through Fast LPC, paired threads and shared buffers. Microsoft's white paper gives the motivation. A client/server transition cost 60–70 µs against 4–5 µs for a kernel transition on a Pentium 100. Moving into the kernel saved 256 KB–1 MB of working set and fixed visible slowness, such as Pinball being "virtually unplayable" on NT 3.51 [S837]. Microsoft argued reliability was unchanged because a CSRSS crash already brought down the system, and claimed security was unaffected [S837].

**Why it is a problem.** The security claim did not hold up. Win32k exposes over a thousand system calls to every process. Forshaw calls it far more error-prone than the NT calls, and it is a standard sandbox escape. The leaked Hacking Team Chrome chain used a win32k bug to break out [S840]. Win32k elevation bugs keep being exploited in the wild, for example CVE-2021-1732, which is in the CISA KEV catalog [S841]. The white paper's own reasoning ("if application code can crash the system, NT has a bug") puts a very large parser in the kernel's trusted computing base [S837].

**Why it was not fixed.** The Win32 GUI API and its drivers are the Windows compatibility surface. Microsoft instead added opt-out mitigations: `DisallowWin32kSystemCalls` from Windows 8 [S838], and a Win32k syscall filter used by Edge in Windows 10 Anniversary Update [S840].

**Alternatives that exist.** The NT 3.51 user-mode design itself [S837]. User-space display servers and compositors (X servers, Wayland compositors) [S829]. Chrome renderers run with win32k locked down, which "significantly reduces the kernel attack surface" [S839].

**Cost of the alternatives.** User-mode GUI paid real context-switch and memory costs on 1990s hardware [S837]. Retrofitting lockdown is expensive. Chrome spent about 10 months replacing GDI font code with DirectWrite and reworking Flash and HDCP paths. It could ship only on Windows 10 because of instability on 8.1 caused by injected third-party code [S840].

**Relevance to our OS.** high — the compositor and graphics drivers should be user-space services that sandboxed processes (including the POSIX layer) can reach only through granted handles; per-frame IPC costs on modern hardware are the open question.

**Sources.** [S829], [S837], [S838], [S839], [S840], [S841]

### DM-082: Window messages across privilege boundaries (shatter attacks, UIPI)

- **Category:** Graphics, input and window systems
- **Systems affected:** Windows NT family (pre-Vista fully; Vista+ partially mitigated)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Any process on a desktop can get a window handle and post messages to any window on that desktop. Some messages carry pointers or callback addresses. Before Vista, services that showed UI ran in Session 0 alongside the interactive user [S845].

**Why it was chosen.** Window messages were the GUI's general event and IPC mechanism, and the Win32 API dates to NT 3.1 (1993) [S844]. All windows on a desktop were assumed to share one trust domain. The EWS authors make the general point that window systems also act as IPC mechanisms, which makes them part of security policy [S858].

**Why it is a problem.** Messages carry no authenticated sender. Paget's 2002 "shatter" paper described the class: a low-privilege process sends messages to a privileged process's window to plant data and redirect execution. He called it unfixable by a patch [S844]. CVE-2002-1230 is a concrete case: a local user sent WM_COPYDATA and then WM_TIMER to the NetDDE agent and ran code as LocalSystem (MS02-071) [S854]. The full mechanism in Paget's paper could not be fetched [S853].

**Why it was not fixed.** The message model is the Win32 API. Microsoft fixed its own services one at a time [S854]. In Vista it added two structural changes: services were isolated in a non-interactive Session 0 [S845], and User Interface Privilege Isolation (UIPI) blocks messages from lower-integrity senders. UIPI is leaky by design. Certain messages below `WM_USER` always pass [S842][S843]. Applications can open holes with `ChangeWindowMessageFilter(Ex)` [S842], and Microsoft shipped compatibility shims that re-enable custom messages [S843]. MSRC classes UAC as defence-in-depth rather than a security boundary, and integrity levels are not listed as boundaries [S846].

**Alternatives that exist.** Microsoft's own advice is to use a secured IPC mechanism instead of window messages [S843]. In Wayland, input routing is decided by the compositor rather than by clients posting events to each other [S829]. EWS proposed a capability-mediated window system [S858].

**Cost of the alternatives.** Session 0 isolation broke interactive services [S845]. UIPI needed per-application filter exceptions and shims [S843], so it is weaker than a boundary [S846].

**Relevance to our OS.** high — the GUI protocol must not become an ambient, unauthenticated IPC channel between clients of different trust.

**Sources.** [S829], [S842], [S843], [S844], [S845], [S846], [S853], [S854], [S858]

### DM-083: No frame synchronization in X (tearing); "every frame is perfect"

- **Category:** Graphics, input and window systems
- **Systems affected:** X Window System; Wayland (as the alternative)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** In core X, clients issue separate drawing requests (rectangles, pixmaps, text) that the server applies to the visible window as they arrive. There is no notion of a complete frame and no vblank-synchronized presentation [S833]. The server is not required to keep window contents, and clients redraw on Expose events [S835].

**Why it was chosen.** Memory was scarce. The X paper notes that one 1024×1024 8-plane image takes a megabyte. It rejects both server display lists and off-screen backing as too slow or too costly, and argues that applications can redraw from their own data [S835]. Double-buffering every window was not affordable.

**Why it is a problem.** Users see partly drawn frames and tearing. Synchronization was retrofitted in layers: compositing managers, DRI2, then the Present extension (2013) for VBLANK-synchronized updates [S848]. Even Present deliberately accepts "an occasional tearing artifact" so that one application's queued blit does not block another's [S847]. Stone describes DRI2 as limited by the core protocol [S833].

**Why it was not fixed.** The core protocol's immediate-mode semantics could not change. Each fix had to be an opt-in extension that toolkits, drivers and compositors all adopted [S833][S848].

**Alternatives that exist.** In Wayland, surface state is double-buffered. attach, damage and region changes stay pending until `wl_surface.commit` applies them atomically, and frame callbacks pace redraws [S849]. The compositor schedules page flips [S829]. GNOME developers summarize this as the motto "every frame is perfect" [S850].

**Cost of the alternatives.** Atomicity adds latency and new stall modes. If a client's GPU work is late, the compositor can miss a refresh. Mutter took about two years of part-time work to wait for buffers before applying transactions [S850]. Games and drawing tablets want tearing for lower latency, so Wayland added `tearing-control-v1` to opt back in, which compositors may ignore [S851].

**Relevance to our OS.** high — the compositor protocol needs atomic commits, explicit fences and a defined latency opt-out from the start.

**Sources.** [S829], [S833], [S835], [S847], [S848], [S849], [S850], [S851]

### DM-084: Wayland's minimal core and protocol-extension fragmentation

- **Category:** Graphics, input and window systems
- **Systems affected:** Wayland ecosystem (GNOME/Mutter, KDE/KWin, wlroots compositors, Weston, toolkits)
- **Classification:** contested
- **Confidence:** medium

**What the design is.** The Wayland core only moves buffers, input and surfaces. One process combines display server, window manager and compositor, and rendering is left to clients ("whatever you want it to be") [S828]. Desktop features come from extension protocols governed by wayland-protocols. Members are projects. `xdg`/`wp` protocols need 3 ACKs and are blocked by a single NACK. `ext` needs 1 ACK. Each tier has minimum open-source implementation requirements [S852].

**Why it was chosen.** X showed the cost of a protocol frozen in a central server. It accumulated four input stacks and several overlapping buffer and display models anyway [S833]. A small core lets desktops differ in policy. A NACK veto stops one vendor from imposing semantics on others [S852].

**Why it is a problem.** Behaviour applications rely on varies by compositor. Server-side decorations (`xdg-decoration`) are still unstable, the compositor may override the client's choice, and Mutter and Weston do not implement it. Applications on GNOME must draw their own frames [S857]. Tearing control is likewise absent from Mutter [S851]. Screenshots, screencasts, global shortcuts and remote desktop moved to xdg-desktop-portal with separate GNOME, KDE and wlroots backends [S832]. waypipe notes that compositor-specific screenshot protocols pass through unfiltered [S836]. On speed: in 2024 Valve's Joshua Ashton started "frog-protocols" as a parallel repository, saying basic protocols wait "months, to years" [S855].

**Why it was not fixed.** The process is consensus by design. The veto that protects diversity also slows convergence. Proposed process changes, such as an "experimental" stage, were still being debated [S856].

**Alternatives that exist.** *Position A (minimal core plus governance is right):* the FAQ and governance docs [S828][S852], and the evidence that X fragmented despite one server [S833]. *Position B (it under-delivers):* frog-protocols and Mesa-side shipping of protocols without upstream consensus [S855][S856]. Some developers objected that bypassing consensus is itself harmful [S856]. Portals are the de facto cross-desktop layer [S832].

**Cost of the alternatives.** A rich mandated core (X, win32k) freezes policy and grows a monolith (DM-080, DM-081). Fast-moving side repositories risk forks.

**Relevance to our OS.** high — with one compositor and no ecosystem we can define a richer core, but must decide what is protocol versus policy before third-party clients depend on it.

**Sources.** [S828], [S832], [S833], [S836], [S851], [S852], [S855], [S856], [S857]

## Stream notes

- **Searched:** man7 credentials/setsid; POSIX `_exit`; Åkesson TTY article; kernel TIOCSTI/ldisc-autoload commits and kernel-hardening posts; oss-security threads (2016–2023); CVE records via the cveawg.mitre.org JSON API (NVD pages render empty); HD Moore 2003 bugtraq; dgl.cx 2023; ECMA-48; Microsoft console devblogs and Learn docs; Plan 9 cons(3)/rio(4); LWN on systemd KillUserProcesses and Stone's LCA 2013 talk; X SECURITY/XACE specs; the Scheifler–Gettys 1986 paper (draft copy); Wayland FAQ, architecture and spec; waypipe; NT 4.0 kernel-mode USER/GDI white paper; Chromium sandbox doc; Project Zero "Breaking the Chain"; UIPI docs; Session 0 paper; MSRC servicing criteria; Packard Present blog; Dänzer's GNOME blog; wayland.app protocol pages; GOVERNANCE.md (AOSP mirror, since gitlab.freedesktop.org is behind Anubis); frog-protocols.
- **Dropped/merged:** none. TIOCLINUX (CVE-2023-28100) and "shared TTY across su/sudo" are folded into DM-072. Line-discipline autoload is folded into DM-069. Console "wrong-way verbs" are folded into DM-073.
- **New entries:** none (DM-170–179 unused). A candidate: "in-kernel VT/fbcon console" (TIOCLINUX, fbcon scrollback removal) — not researched enough to add.
- **Open questions:** (1) Paget's shatter paper could not be fetched (host is gone; archive.org is blocked for this agent), so the mechanism in DM-082 rests on CVE-2002-1230. (2) The EWS full PDF was fetched but could not be text-extracted; only abstract claims are used. (3) Distribution defaults for `CONFIG_LEGACY_TIOCSTI` after 6.2 are not verified. (4) I found no primary statement of GNOME's SSD position; the evidence is protocol support tables. (5) I found no primary source for Høgsberg coining "every frame is perfect"; it is attributed to the Wayland community via Dänzer.
- **Weak or secondary sources to watch:** S831 (Rutkowska blog, credible practitioner with a reproducible demo); S821 (Linux Magazine news); S856 (Japanese news summary); the support tables in S851/S857 are wayland.app's compilation, not the protocol text; S835 is a pre-publication draft, not the TOG version of record; S811 is a researcher blog but is the primary disclosure.
