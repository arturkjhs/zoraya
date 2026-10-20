### DM-076: Privileged ports below 1024

- **Category:** Networking APIs
- **Systems affected:** Unix/BSD, Linux, macOS (wildcard binds unrestricted since 10.14; binds to a specific address still restricted); contrast iOS, Windows
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Binding a TCP/UDP port below 1024 requires root, or on Linux `CAP_NET_BIND_SERVICE`. Linux makes the threshold configurable per network namespace with `net.ipv4.ip_unprivileged_port_start` (default 1024; 0 disables it) [S445].

**Why it was chosen.** On early multi-user hosts run by trusted administrators, a connection from a source port below 1024 proved that root on the remote host had sent it. BSD `rcmd`/`rlogin` built authentication on this: `rresvport` obtains a privileged port, and servers then trust `.rhosts`/`hosts.equiv` [S446]. The low-port rule also stopped ordinary users from impersonating well-known services on shared machines.

**Why it is a problem.** The proof only holds if every host on the network is administered by someone trustworthy, which stopped being true once anyone could run their own machine. Meanwhile the rule forces daemons that serve port 80 or 443 to start as root or carry a capability. That is ambient privilege for a trivial need, and it encouraged setuid daemons and root-run servers. It is also global per namespace and tied to the port number, not to any notion of which program should own the service.

**Why it was not fixed.** The default stays for compatibility, but it is now widely disabled. Linux added the per-namespace sysctl in 2017 so container managers could lift the restriction without giving up control of the network namespace [S447]. Docker has set it to 0 in containers by default since 2020, calling privileged-port binding a safe operation [S449]. Apple lifted the restriction in macOS Mojave without documenting it, and iOS never had it. The macOS change covers only wildcard binds: as of 2025, binding a low port on a specific local address still needs root [S448].

**Alternatives that exist.** Grant a specific port to a specific program by policy. One example is socket activation: the service manager creates the listening socket and passes the fd to the daemon at `exec` [S233]. Or drop the restriction entirely, as iOS, macOS (for wildcard binds) and Docker containers do [S448][S449].

**Cost of the alternatives.** With no restriction on a shared host, any local user can grab a well-known port before the real daemon starts. Review of the Docker change agreed that it must not be applied in the host network namespace, and a later runc change rejects that case [S449]. Per-port authority needs a policy store and an authority to manage it.

**Relevance to our OS.** low. With no compatibility obligations, we can make "bind port N" a capability like any other and not encode trust in port numbers.

**Sources.** [S445], [S446], [S447], [S448], [S449], [S233]

### DM-077: `SIGPIPE` as default on writing to a closed socket/pipe

- **Category:** Networking APIs
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; language runtimes (Python, Rust)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Writing to a pipe with no readers, or to a stream socket whose peer has gone away, sends `SIGPIPE`, and the default action kills the process. If the process ignores (or handles) the signal, `write` instead fails with `EPIPE` [S450][S452].

**Why it was chosen.** For shell pipelines this is useful: in `producer | head`, the producer dies as soon as `head` exits, even if it never checks write errors. Simple filters therefore need no error handling for this case [unverified as a historical rationale].

**Why it is a problem.** The same rule applies to sockets, where a peer closing a connection is a routine event, not a reason to kill a server.
- Every network program must disable the signal: process-wide with `SIG_IGN`, per call with `MSG_NOSIGNAL` (Linux 2.2; POSIX only from 2008) [S451], or per socket with BSD's `SO_NOSIGPIPE` [S453].
- Ignoring the signal is a process attribute that affects all threads [S451], so a library cannot change it without changing behaviour for the whole program.
- Runtimes override it, which creates inconsistency. Python ignores `SIGPIPE` at startup and turns the error into `BrokenPipeError`, so a plain `script | head` produces a traceback [S454]. Rust's startup code has ignored `SIGPIPE` since 2014. When the issue was filed this was undocumented process-wide state, Rust libraries called from a non-Rust `main` behave differently from Rust applications, and the default is still debated [S455].

**Why it was not fixed.** POSIX mandates the behaviour for both pipes and sockets [S452], and shell pipelines depend on it.

**Alternatives that exist.** Per-call and per-socket opt-outs [S451][S453]. Runtimes that make `EPIPE` an ordinary error [S454]. A design could treat a closed reader as an error return everywhere and let pipeline tools exit on it.

**Cost of the alternatives.** Without the signal, every producer in a pipeline must check write errors, or it keeps running after its reader has gone. Python's `BrokenPipeError` noise and Rust's debate show that both defaults hurt someone [S454][S455].

**Relevance to our OS.** low. A native API without asynchronous signals returns an error. Only the POSIX layer needs to emulate `SIGPIPE`, for pipelines of ported tools.

**Sources.** [S450], [S451], [S452], [S453], [S454], [S455]

### DM-078: Network stack in the kernel vs user-space stacks

- **Category:** Networking APIs
- **Systems affected:** Linux, BSDs, Windows (in-kernel stacks); DPDK, Arrakis, IX, Google Snap, Fuchsia (netstack as a component); Linux XDP/AF_XDP as a middle ground
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Mainstream OSes run TCP/IP inside the kernel, behind the sockets API. The alternatives are:
- Kernel bypass: the application drives the NIC directly with poll-mode drivers, as in DPDK [S459].
- Control-plane/data-plane splits: the kernel sets up protection and hardware or virtualization enforces it, as in Arrakis and IX [S456][S457].
- A user-space networking service: Google's Snap [S458], or Fuchsia's netstack component [S462].

**Why it was chosen.** An in-kernel stack shares the NIC safely among many processes. It enforces isolation and firewalling, and it works with standard tools such as tcpdump and netstat [S460][S461]. When the sockets API was designed, NICs offered no hardware virtualization or multiple queues, so a shared, protected stack had to sit in the kernel [unverified as a sourced historical claim].

**Why it is a problem.** The two positions disagree.
- *Pro-bypass / user space.* Kernel crossings, copies and generic code paths limit performance. Arrakis reports 2 to 5x lower latency and 9x higher throughput than tuned Linux for a NoSQL store [S456]. IX keeps protection and reports up to 3.6x the throughput of a key-value store and more than 2x lower tail latency [S457]. Snap runs on over half of Google's fleet, gets more than 3x the Gbps per core of the kernel stack for RPC workloads, and can be upgraded without moving applications off the machine [S458]. Fuchsia treats the stack as a replaceable component [S462].
- *Pro-kernel.* Bypass gives up the OS's isolation and security mechanisms, breaks standard tooling, dedicates cores, and makes applications reimplement routing tables and higher-level protocols [S460]. Cloudflare estimated bypass would save only about 5 to 10% CPU for its proxy workload, while losing tcpdump, netstat statistics, iptables-based DDoS defences, and the ability to share the NIC with SSH [S461]. Cloudflare does use a partial bypass on a single RX queue for DDoS filtering, so even this operator uses both [S461].

**Why it was not fixed.** Not a defect to fix but an ongoing split. Linux's answer is XDP: programmable fast paths inside the kernel's driver path, with redirection to user space through AF_XDP sockets. In the 2018 XDP paper, true zero-copy AF_XDP delivery was still work in progress [S460].

**Alternatives that exist.** All of the above. XDP keeps the kernel in charge but, by its authors' own measurement, does not quite match DPDK's peak performance [S460].

**Cost of the alternatives.** Bypass and dataplane OSes need virtualized NICs or dedicated hardware queues and dedicated cores [S456][S457][S460], plus per-application stacks [S460]. A user-space service adds a hop between application and stack and depends on scheduling: Snap needed a kernel/user-space CPU scheduler co-design [S458]. A rewrite is a big job. Fuchsia replaced its Go netstack because of language and ownership concerns, and listed reaching performance parity as a risk [S462].

**Relevance to our OS.** high. As a capability-leaning, possibly microkernel-style OS, we will probably host the stack as a service. Snap, IX and Arrakis show this can perform well, but only with careful IPC and scheduling design.

**Sources.** [S456], [S457], [S458], [S459], [S460], [S461], [S462]

### DM-079: X11 security model: any client can read input and contents of others

- **Category:** Graphics, input and window systems
- **Systems affected:** X Window System (X11, Xorg); Linux/BSD desktops on X
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** X checks access only when a client connects (host address or auth cookie). After that, a client can manipulate any resource on the server. Scheifler and Gettys judged per-resource access control not worth its performance cost, because accidental interference between clients seemed very unlikely [S835]. Any connected client can therefore read the keyboard, capture other windows and send synthetic events.

**Why it was chosen.** The 1986 X paper (which describes X10) addressed accidental interference, not hostile clients, and access control was by host address [S835]. Openness let window managers and other desktop managers be ordinary clients, keeping policy outside the server [S835].

**Why it is a problem.** Rutkowska showed that an unprivileged `xinput test` logs every keystroke, including a root password typed into `su` in another terminal. It works across user accounts and SELinux domains, with no exploit needed, because it is how X is designed [S831]. Screen capture and keystroke injection work the same way. OpenSSH warns that trusted X11 forwarding lets a remote host perform keystroke monitoring [S830]. The EWS authors argue that X11 and Windows inherited Xerox Alto-era trust assumptions [S858].

**Why it was not fixed.** The SECURITY extension (1996) added "untrusted" clients that cannot grab the keyboard, read trusted windows or use others' resources. It also breaks ICCCM focus mechanisms and leaves property access implementation-dependent [S826]. ssh uses it for untrusted forwarding, with a 20-minute default timeout [S830]. XACE (NSA, 2006–2009) added LSM-style hooks, but its own spec admits incomplete event-delivery coverage and unsecured GLX/direct rendering [S827]. In a comment on her post, Rutkowska reported that XACE was apparently unused, and that SELinux's sandbox used a nested X server instead [S831]. Any real fix breaks window managers, screenshot tools and input methods that depend on the open model.

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

**Why it was chosen.** The goals were network transparency (an application on one machine displays on another of any architecture), device independence, and mechanism rather than policy [S835]. High-level drawing requests are compact on the wire. The protocol is asynchronous and was designed for 5–50 ms round trips. Applications need no rewriting or relinking for new display hardware [S835]. For time-shared machines and 1980s workstations this was a strong design.

**Why it is a problem.** Rendering moved to the client. Toolkits draw locally with GPUs and hand over finished buffers, which left the X server, in Daniel Stone's words, a "terrible, terrible, terrible" IPC bus between client and compositor [S833]. The rendering paths every application uses (MIT-SHM, DRI2) do not work over the network, and Stone said X's middleman role hits its worst case over the Internet [S833]. In the composited X architecture every frame goes through the server twice, from client to server and from compositor back through the server [S829].

**Why it was not fixed.** Backward compatibility froze the core protocol. Features moved to extensions and clients instead. Stone counted four input stacks, five display-management extensions and four buffer models [S833].

**Alternatives that exist.** Wayland leaves remote rendering out of scope and recommends a separate remoting server or a compositor with remoting [S828]. Clients render into their own buffers [S829]. waypipe proxies Wayland like `ssh -X`, compressing buffers with lz4/zstd or video codecs [S836]. Stone proposed VNC-like compositor-to-compositor transport [S833].

**Cost of the alternatives.** Per-application remoting is no longer built in. waypipe is an add-on: it does not filter protocols, is not hardened against malicious peers, and contains "plenty of unsafe Rust" [S836]. Users who relied on `ssh -X` lost a convenience, and commenters on the Stone report argued that per-application remote display is still a legitimate use case that X serves [S833]. Classed right-at-the-time-wrong-now rather than mistake because the 1986 constraints (no client GPUs, slow networks, shared hosts) were real [S835].

**Relevance to our OS.** medium — we target local QEMU, but whether remote display is a protocol property or a proxy affects the compositor protocol design.

**Sources.** [S828], [S829], [S833], [S834], [S835], [S836]

### DM-081: Windows win32k: window manager and GDI in kernel mode

- **Category:** Graphics, input and window systems
- **Systems affected:** Windows NT 4.0 and later
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Since NT 4.0 (1996), the Window Manager (USER), GDI and graphics drivers run in kernel mode as executive services (win32k), reached by system call from every GUI thread [S837].

**Why it was chosen.** In NT 3.51 these components ran in CSRSS and were called through Fast LPC, paired threads and shared buffers. Microsoft's white paper gives the motivation. A client/server transition cost 60–70 µs against 4–5 µs for a kernel transition on a Pentium 100. Moving into the kernel saved 256 KB–1 MB of working set and fixed visible slowness, such as Pinball being "virtually unplayable" on NT 3.51 [S837]. Microsoft argued reliability was unchanged because a CSRSS crash already brought down the system, and claimed security was unaffected [S837].

**Why it is a problem.** The security claim did not hold up. Win32k exposes over a thousand system calls to every process. Forshaw says NT system call bugs are "nothing compared to Win32k", and win32k is a standard sandbox escape. He traces it to the NT 4 performance decision, made before Microsoft's security push. The leaked Hacking Team Chrome chain used a win32k bug to break out [S840]. Win32k elevation bugs keep being exploited in the wild, for example CVE-2021-1732, which is in the CISA KEV catalog [S841]. The white paper's own reasoning ("if application code can crash the system, NT has a bug") puts a very large parser in the kernel's trusted computing base [S837].

**Why it was not fixed.** The Win32 GUI API and its drivers are the Windows compatibility surface. Microsoft instead added opt-in mitigations: `DisallowWin32kSystemCalls` from Windows 8 [S838], and a Win32k syscall filter used by Edge in Windows 10 Anniversary Update [S840].

**Alternatives that exist.** The NT 3.51 user-mode design itself [S837]. User-space display servers and compositors (X servers, Wayland compositors) [S829]. Chrome renderers run with win32k locked down, which "significantly reduces the kernel attack surface" [S839].

**Cost of the alternatives.** User-mode GUI paid real context-switch and memory costs on 1990s hardware [S837]. Retrofitting lockdown is expensive. Chrome's renderer lockdown took about two years, including replacing GDI font code with DirectWrite. Extending it to the Flash (PPAPI) process took almost 10 more months, including HDCP rework. That lockdown shipped first only on Windows 10, because of unexplained instability on 8.1 likely caused by injected third-party code [S840].

**Relevance to our OS.** high — the compositor and graphics drivers should be user-space services that sandboxed processes (including the POSIX layer) can reach only through granted handles; per-frame IPC costs on modern hardware are the open question.

**Sources.** [S829], [S837], [S838], [S839], [S840], [S841]

### DM-082: Window messages across privilege boundaries (shatter attacks, UIPI)

- **Category:** Graphics, input and window systems
- **Systems affected:** Windows NT family (pre-Vista fully; Vista+ partially mitigated)
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** Any process on a desktop can get a window handle and post messages to any window on that desktop. Some messages carry pointers or callback addresses. Before Vista, services ran in Session 0 alongside the interactive user's applications [S845].

**Why it was chosen.** Window messages were the GUI's general event and IPC mechanism, and the Win32 API dates to NT 3.1 (1993) [S844]. All windows on a desktop were assumed to share one trust domain. The EWS authors make the general point that window systems also act as IPC mechanisms, which makes them part of security policy [S858].

**Why it is a problem.** Messages carry no authenticated sender. Paget's 2002 "shatter" paper described the class and called it unfixable by a patch [S844]. In his worked example, a guest user found an edit box in a virus scanner's UI running as LocalSystem. He lifted its length limit with `EM_SETLIMITTEXT`, pasted shellcode in, then sent `WM_TIMER` with the shellcode's address as the callback [S853]. CVE-2002-1230 is a concrete case: a local user sent WM_COPYDATA and then WM_TIMER to the NetDDE agent and ran code as LocalSystem (MS02-071) [S854].

**Why it was not fixed.** The message model is the Win32 API. Microsoft fixed its own services one at a time [S854]. In Vista it added two structural changes: services were isolated in a non-interactive Session 0 [S845], and User Interface Privilege Isolation (UIPI) blocks messages from lower-integrity senders. UIPI is leaky by design. Certain messages below `WM_USER` always pass [S842][S843]. Applications can open holes with `ChangeWindowMessageFilter(Ex)` [S842], and Microsoft shipped compatibility shims that re-enable custom messages [S843]. MSRC classes UAC as defence-in-depth rather than a security boundary, and integrity levels are not listed as boundaries [S846].

**Alternatives that exist.** Microsoft's own advice is to use a secured IPC mechanism instead of window messages [S843]. In Wayland, input routing is decided by the compositor rather than by clients posting events to each other [S829]. EWS proposed a capability-mediated window system [S858].

**Cost of the alternatives.** Session 0 isolation broke interactive services [S845]. UIPI needed per-application filter exceptions and shims [S843], so it is weaker than a boundary [S846].

**Classification note.** Reclassified from right-at-the-time-wrong-now to mistake. The flaw did not come from a later change in constraints. NT 3.1 already ran privileged services and user applications on one desktop, and Paget dates the vulnerability class to 1993 [S844][S845].

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

**Alternatives that exist.** In Wayland, surface state is double-buffered. attach, damage and region changes stay pending until `wl_surface.commit` applies them atomically, and frame callbacks pace redraws [S849]. The compositor schedules page flips [S829]. A Mutter developer cites Wayland's motto "every frame is perfect" for this [S850].

**Cost of the alternatives.** Atomicity adds latency and new stall modes. If a client's GPU work is late, the compositor can miss a refresh. Making Mutter wait for client buffers before applying transactions took almost two years of on-and-off work [S850]. Games and drawing tablets want tearing for lower latency, so Wayland added `tearing-control-v1` to opt back in, which compositors may ignore [S851].

**Relevance to our OS.** high — the compositor protocol needs atomic commits, explicit fences and a defined latency opt-out from the start.

**Sources.** [S829], [S833], [S835], [S847], [S848], [S849], [S850], [S851]

### DM-084: Wayland's minimal core and protocol-extension fragmentation

- **Category:** Graphics, input and window systems
- **Systems affected:** Wayland ecosystem (GNOME/Mutter, KDE/KWin, wlroots compositors, Weston, toolkits)
- **Classification:** contested
- **Confidence:** medium

**What the design is.** The Wayland core only moves buffers, input and surfaces. One process combines display server, window manager and compositor, and rendering is left to clients ("whatever you want it to be") [S828]. Desktop features come from extension protocols governed by wayland-protocols, whose members are projects, not individuals [S852][S2500]. Under the current rules, `xdg`/`wp` protocols need 3 member ACKs and 3 open-source implementations, and a single NACK blocks them. `ext` protocols need 2 ACKs (1 in the earlier rules) plus one open-source client and one server. Since late 2024 an experimental `xx` tier is merged after a two-week review unless NACKed, and a NACK must be grounded in technical reasoning [S2500].

**Why it was chosen.** X showed the cost of a protocol frozen in a central server, and it accumulated four input stacks and several overlapping buffer and display models anyway [S833]. Leaving rendering out keeps Wayland small and feasible [S828]. A small core lets desktops differ in policy. The governance treats a NACK as the last resort against protocols that would harm the ecosystem [S2500].

**Why it is a problem.** The two positions read the same facts differently.
- *Position B (it under-delivers).* Behaviour that applications rely on varies by compositor. Server-side decorations (`xdg-decoration`) are still an unstable protocol, the compositor decides the mode, and Mutter and Weston do not implement it, so applications on GNOME must draw their own frames [S857]. Tearing control is likewise absent from Mutter [S851]. Screenshots, screencasts, global shortcuts and remote desktop moved to xdg-desktop-portal, with separate GNOME, KDE and wlroots backends [S832]. waypipe notes that compositor-specific screenshot protocols pass through unfiltered [S836]. In September 2024 Valve's Joshua Ashton started "frog-protocols" as a parallel repository, saying basic protocols wait "months, to years" (citing FIFO presentation, needed for VSync) [S855][S856].
- *Position A (minimal core plus governance is right).* Differences between compositors are partly deliberate policy choices that a small core allows [S828][S852]. X fragmented into overlapping extensions despite having one server [S833]. Portals give a de facto cross-desktop layer [S832]. The process also responded. The upstream `fifo-v1` protocol was merged in October 2024 and shipped in wayland-protocols 1.38 [S2501], and the experimental tier was adopted that same month [S2500].

**Why it was not fixed.** The process is consensus by design, and the veto that protects diversity also slows convergence. When frog-protocols launched, Mike Blumenkrantz's proposal for an "experimental" stage drew mixed reactions [S856]. It was adopted in October–November 2024 together with limits on NACK use [S2500]. Whether this fixes the speed problem is not yet established.

**Alternatives that exist.** *Position A:* the FAQ and governance docs [S828][S852][S2500], and the evidence that X fragmented despite one server [S833]. *Position B:* frog-protocols, and an Ashton merge request to support `frog-fifo-v1` in Mesa's Wayland WSI [S855][S856]. Some Wayland developers objected that adding support without upstream consensus is undesirable. Others said an iterative model gives useful feedback [S856].

**Cost of the alternatives.** A rich mandated core (X, win32k) freezes policy and grows a monolith (DM-080, DM-081). Fast-moving side repositories risk forks and duplicate protocols that clients must support during transition [S855].

**Relevance to our OS.** high — with one compositor and no ecosystem we can define a richer core, but must decide what is protocol versus policy before third-party clients depend on it.

**Sources.** [S828], [S832], [S833], [S836], [S851], [S852], [S855], [S856], [S857], [S2500], [S2501]

### DM-085: Environment variables as implicit global configuration

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows (per-process environment, but with a thread-safe API)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Each process has a flat list of `NAME=value` strings inherited across `fork`/`exec`. Any library may read any variable at any time with `getenv`, and many do (locale, time zone, loader paths, DNS, proxy settings, character conversion).

**Why it was chosen.** It is the simplest possible way to pass optional configuration through a chain of programs without changing argument conventions, and it costs nothing on a PDP-11. Inheritance is useful: setting `PATH` or `TERM` once configures a whole session.

**Why it is a problem.** (1) Thread safety: glibc marks `setenv`/`unsetenv` MT-Unsafe and POSIX does not require reentrancy [S234]. Rust made `std::env::set_var` `unsafe` in the 2024 edition, because libc `getenv` is unsynchronized and libraries do not document which functions read the environment (DNS lookups do), so no program can guarantee soundness once threads exist [S235][S236]. glibc's attempt to make `getenv` thread-safe "in more cases" broke applications that managed `environ` themselves and needed a compatibility fix [S237]. (2) Security: the environment is attacker-chosen input to privileged programs; the loader must strip a long list of variables in secure-execution mode [S127], and PwnKit worked by smuggling `GCONV_PATH` back in [S206]. `LD_PRELOAD` silently injects code into every child [S127]. (3) Hidden coupling: Poettering cites heavy dependence on environment variables as one reason shell-based init scripts are unpredictable [S233]; `run0` sessions take the service manager's environment rather than the caller's, copying only a few variables such as `TERM` [S211].

**Why it was not fixed.** `environ` is a public global array in POSIX and C programs write it directly, so any locking scheme breaks someone [S237].

**Alternatives that exist.** Explicit configuration passed at spawn (Rust's recommended `Command::env` for children [S235]); service managers that construct the environment from configuration rather than inheritance [S211]; on Windows, Rust documents `set_var` as always sound, even with multiple threads [S235].

**Cost of the alternatives.** Losing ambient inheritance means every launcher must plumb configuration explicitly; the POSIX layer will still need an `environ` for the C compiler and its tools.

**Relevance to our OS.** high — the native process-start ABI decides whether an inherited mutable string table exists at all.

**Sources.** [S234], [S235], [S236], [S237], [S127], [S206], [S211], [S233]

### DM-086: Windows registry

- **Category:** Configuration, observability and system management
- **Systems affected:** Windows NT and successors
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A system-defined hierarchical database of keys and typed values, used by the OS and applications for configuration [S238][S239], stored in binary hive files and parsed in the kernel [S241].

**Why it was chosen.** Raymond Chen lists the problems with INI files: no Unicode, per-file rather than per-key security, lost updates from concurrent writers, whole-file parsing on each access, a 32 KB limit, only two levels, string-only values, a default location in the admin-only Windows directory, and hard central administration. He says the registry "tried to address these concerns" [S240]. These are real problems, and a central store also enables central administration.

**Why it is a problem.** (1) A shared mutable namespace for all programs: apps that assumed they could write `HKLM\Software` broke under non-admin users, so Vista added registry virtualization, redirecting writes to a per-user VirtualStore, which Microsoft calls interim and which breaks registry-as-IPC between a service and an app [S228]. (2) Kernel attack surface: a 20-month Project Zero audit (May 2022–December 2023) led to 50 CVEs. Hives are a binary format parsed entirely in kernel by 30-year-old C, and since Vista any user can load an arbitrary hive via `RegLoadAppKey` [S241]. (3) Opaque state: data is not owned by an application. Microsoft notes that uninstallers traditionally cannot clean `HKCU` for logged-out users [S2502], and warns not to edit data that is not yours [S238].

**Why it was not fixed.** Every Win32 application and Group Policy depends on it. The fix has been isolation layers: virtualization [S228], and for packaged (MSIX) apps a per-package `registry.dat` merged into `HKLM\Software`, with `HKCU` writes copied to a per-app location that is deleted on uninstall [S2502].

**Alternatives that exist.** Per-application text files (DM-087), which Chen says reopen INI problems once programs write them [S240]; declarative, immutable system configuration (NixOS, DM-087).

**Cost of the alternatives.** File-based configuration loses typed values, per-key ACLs and safe concurrent writes [S240].

**Relevance to our OS.** medium — argues for typed, per-component configuration owned by the component, and against a kernel-resident shared database.

**Sources.** [S238], [S239], [S240], [S241], [S228], [S2502]

### DM-087: Per-program text configuration syntax in `/etc`

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Each program defines its own configuration file format, usually line-based text in `/etc`, edited by hand or by ad-hoc scripts. There is no common schema, type system, validation, or transaction.

**Why it was chosen.** Text works with every editor and with `diff`, `grep` and version control, needs no shared infrastructure, and each program can evolve its own format. Chen notes that text configuration files that are only read sidestep many INI problems [S240].

**Why it is a problem.** Errors are syntactic and semantic and found late. Yin et al. studied 546 real misconfigurations from a commercial storage system and from CentOS, MySQL, Apache and OpenLDAP. In 70–85.5% of cases a parameter was set wrongly. Of those, 38–54% were illegal values a checker could catch, and 12–30% were inconsistencies between parameters [S242]. The study covers configuration in general, not text formats in particular. It argues for schemas and validation rather than against text as such. Automated management needs a parser per format; Augeas exists to turn many native formats into a tree because editing them with sed/awk is unsafe [S243]. Updates are destructive in-place edits, so rollback and reproducibility are hard. Putting `/etc` under version control does not fix this, because configuration is coupled to the installed package versions [S244].

**Why it was not fixed.** No central authority owns the format of third-party programs; every attempt at a common store (GConf/dconf, Elektra) covers only some programs [unverified].

**Alternatives that exist.** NixOS builds the whole system configuration from a purely functional specification, making it immutable, reproducible and rollback-able [S244]. Registry-style typed stores (DM-086). Config-editing layers (Augeas) [S243].

**Cost of the alternatives.** NixOS stores every package under hashed store paths with no `/usr` or `/lib`, so hard-coded paths must be patched per package. Mutable state such as `/etc/passwd`, `/etc/resolv.conf` and `/var` stays outside the functional model [S244]. Typed stores lose the universality of text and need tools to inspect [S240].

**Relevance to our OS.** medium — we control every native component, so a single schema-checked configuration model is feasible; the POSIX layer will still meet text configs.

**Sources.** [S242], [S243], [S244], [S240]

### DM-088: Init systems: shell-script SysV init vs systemd

- **Category:** Configuration, observability and system management
- **Systems affected:** Linux distributions; comparable: Solaris SMF, macOS launchd, s6/runit
- **Classification:** contested
- **Confidence:** medium

**What the design is.** SysV init runs ordered shell scripts per runlevel; each script starts a daemon that daemonizes and is tracked by a PID file. systemd replaces this with declarative unit files, dependency-based parallel start, socket activation and cgroup-based tracking, and has grown into a suite covering logging, login and other services.

**Why it was chosen.** SysV init was simple, transparent and editable in a language every admin knew. Poettering made the case for systemd in 2010: startup was serialized, the shell was slow (`grep` invoked at least 77 times in `/etc/init.d`), double-forked daemons escaped supervision, and shell scripts were fragile because they depend on environment variables. With socket activation, the manager creates listening sockets before starting daemons, which removes most ordering [S233].

**Why it is a problem.** The two positions disagree, and both are held by experienced practitioners.
- *Position A (systemd proponents).* Scripts cannot supervise, restart or reliably track services, and they serialize boot [S233]. Solaris SMF had already shown a declarative service model with dependencies and instances [S250].
- *Position B (critics).* systemd integrates too much into one project around PID 1, pushes other software to depend on its interfaces, and ignores portability to BSD and other Unixes. The s6 author calls it "technically unsustainable" [S249] and offers small supervision tools in the daemontools tradition instead [S251]. He also concedes that low-level userspace needed improving, and that s6 shares that goal [S249]. Note that this critic maintains a competing project. In Debian's 2014 dispute, Ian Jackson pushed for "loose coupling", so that no package could depend on a specific PID 1 [S246].

**Why it was not fixed.** It is a governance dispute as much as a technical one. Debian's technical committee chose systemd as the default for jessie [S245], after a 4:4 tie broken by the chair's casting vote [S246]. A 2014 GR on coupling resolved that no GR was required [S247]. A 2019 GR with seven proposals plus "further discussion" chose "systemd but we support exploring alternatives" [S248].

**Alternatives that exist.** SMF [S250], launchd (whose socket-activation model systemd copies [S233]; launchd's own documentation was not fetched), s6/runit supervision [S251], and systemd itself.

**Cost of the alternatives.** Minimal supervisors leave dependency ordering to separate tools (s6-rc in the s6 case) and offer fd-holding rather than full socket activation [S251]. An integrated manager concentrates complexity and policy in one project [S249].

**Relevance to our OS.** high — we will need a service manager; the contested question is how much belongs in it and which component tracks service lifetime.

**Sources.** [S233], [S245], [S246], [S247], [S248], [S249], [S250], [S251]

### DM-089: Unstructured text logging (syslog)

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, network devices
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Programs send free-form text lines tagged with facility and severity to a syslog daemon over a local socket or UDP; the daemon appends them to text files. RFC 3164 (2001) only documented existing practice [S252].

**Why it was chosen.** Text is human-readable and tool-independent, and a fire-and-forget UDP sender never blocks the program that is logging [S252]. That early systems had little to log and one administrator to read it is plausible but [unverified].

**Why it is a problem.** BSD syslog timestamps have no year or time zone, and messages were limited to 1024 bytes over UDP. The RFC's security section lists problems with authentication, message forgery and sequenced delivery [S252]. RFC 5424 notes that implementations shared little beyond the PRI field [S253]. The journal design document lists the consequences. Client-supplied metadata such as identity and PID is not verified. Analysis depends on fragile regular expressions that turn log strings into de facto ABI. Lookups are O(n), and log manipulation after an intrusion is usually found only by chance [S254]. The original announcement cited the 2011 kernel.org break-in as motivation for hash-chaining [S256].

**Why it was not fixed.** RFC 5424 added structured data, MSGID and RFC 3339 timestamps [S253], but the message body stays free text and old formats persist. The journald replacement was contested. Its initial FAQ said the binary format would not be standardized [S256]. Today's documentation says the code is the only authoritative description of the format and asks readers to use the C API rather than parse files directly [S255].

**Alternatives that exist.** systemd-journald: indexed, structured key/value entries, trusted fields derived from kernel credentials, Forward Secure Sealing, corruption handling by rotation [S254][S255]. Windows ETW: kernel-level structured tracing with providers and consumers, enabled dynamically in production [S257].

**Cost of the alternatives.** Binary logs need specific tools to read. Early critics objected to losing standard Unix text tools [S256, comments: weak]. Structured schemas need governance. Text logs keep real advantages (universal tooling, readability without special software), which is why this is classed as a tradeoff.

**Classification note.** Changed from right-at-the-time-wrong-now to tradeoff. The entry gave no sourced evidence about the constraints at the time, and the alternative (binary structured logs) has costs that are still disputed [S255][S256].

**Relevance to our OS.** medium — a new OS can make logs structured, with kernel-attested sender identity, from day one.

**Sources.** [S252], [S253], [S254], [S255], [S256], [S257]

### DM-090: In-place, non-atomic package updates vs image-based A/B updates

- **Category:** Boot, update and recovery
- **Systems affected:** Traditional Linux distributions (dpkg/rpm), Windows Update (servicing stack), most BSD package flows; alternatives in Android, ChromeOS, OSTree-based systems, NixOS
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** The running system is updated by unpacking new files over old ones, one package at a time, with maintainer scripts doing configuration. The system remains partly old and partly new during the update.

**Why it was chosen.** It is flexible (any subset of packages, local modification), uses little extra disk, and suits mutable, hand-administered machines. Before cheap storage, keeping a second full copy of the OS was unaffordable.

**Why it is a problem.** An update interrupted by a crash, power loss or failing script leaves an inconsistent system. dpkg has explicit "half-installed" and "half-configured" states and recovery commands (`--configure -a`, `--audit`) for exactly this [S137]. Updating a live system creates conflicts between the libraries and services already running and those on disk. systemd recommends rebooting into a special offline-update mode to avoid them, which gives up the "no downtime" benefit [S136] (cross-ref DM-091). The resulting state depends on the update path and on local edits, so machines drift apart and rollback is not first-class. The NixOS paper names destructive, imperative updating as the cause of difficult rollback and unreliable upgrades [S130].

**Why it was not fixed.** Package managers, maintainer scripts and FHS layouts form a huge ecosystem, and users value mutability. Image-based schemes need a read-only `/usr` and a clear split between OS and state.

**Alternatives that exist.** Android A/B: the update is written to the inactive slot while the system runs. The only downtime is the reboot. If the new slot fails to boot, or is not marked successful after several attempts, the bootloader falls back to the old one. Android 10 replaced legacy A/B with Virtual A/B [S134]. OSTree: complete, content-addressed trees, deployed as hardlinks and switched atomically; `/etc` gets a 3-way merge and `/var` is shared [S135]. NixOS: immutable store paths and atomic switching between system generations [S129][S130].

**Cost of the alternatives.** Extra storage (two slots, or several deployments); local changes must be confined to `/etc` and `/var` [S135]; a boot is still needed to activate the update; and package-level flexibility is lost or has to be layered back on. OSTree contrasts itself with plain image replication, which is reliable and predictable but often keeps no persistent writable state [S135].

**Relevance to our OS.** high — we can define the system image as immutable and content-addressed from the start, with state separated, which is very hard to retrofit.

**Sources.** [S129], [S130], [S134], [S135], [S136], [S137]

