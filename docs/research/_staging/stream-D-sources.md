# Stream D sources (S400–S499)

[S400] Gaurav Banga, Jeffrey C. Mogul, Peter Druschel, A scalable and explicit event delivery mechanism for UNIX, USENIX Annual Technical Conference, 1999, https://www.usenix.org/legacy/events/usenix99/full_papers/banga/banga.pdf, primary, checked 2026-10-19
  - supports: DM-023 (select/poll restate interest set every call; cost scales with descriptors; declare_interest/get_next_event; select ~25% CPU in proxy)
[S401] Jonathan Lemon, Kqueue: A generic and scalable event notification facility, Proceedings of the FREENIX Track, USENIX Annual Technical Conference, 2001, https://www.read.seas.harvard.edu/~kohler/class/aosref/lemon01kqueue.pdf, primary, checked 2026-10-19
  - supports: DM-023 (select/poll do not scale and cannot watch signals/fs/AIO; knotes deleted on close; RT-signal queues give stale events and overflow; /dev/poll limitations)
  - supports: DM-130 (single generic queue for heterogeneous event sources; signal-based notification unsuitable for libraries)
[S402] FreeBSD Project, kqueue(2) manual page, FreeBSD, current, https://man.freebsd.org/cgi/man.cgi?query=kqueue&sektion=2, primary, checked 2026-10-19
  - supports: DM-023 (close() removes kevents referencing the descriptor; changelist applied in same call as wait)
  - supports: DM-029 (EVFILT_READ on vnodes reports offset to EOF, unlike poll which always fires for regular files)
  - supports: DM-130 (EVFILT_PROC/PROCDESC/SIGNAL/TIMER/VNODE/USER filters)
[S403] Linux man-pages project, epoll(7), man7.org, current, https://man7.org/linux/man-pages/man7/epoll.7.html, primary, checked 2026-10-19
  - supports: DM-023 (interest key is fd plus open file description; fd removed only after all duplicates closed; ET starvation and must-drain-to-EAGAIN rules)
[S404] Marek Majkowski, Epoll is fundamentally broken 1/2, idea.popcount.org (personal blog), 2017, https://idea.popcount.org/2017-02-20-epoll-is-fundamentally-broken-12/, weak, checked 2026-10-19
  - supports: DM-023 (multithreaded load balancing; thundering herd; EPOLLEXCLUSIVE/EPOLLONESHOT as later fixes)
[S405] Marek Majkowski, Epoll is fundamentally broken 2/2, idea.popcount.org (personal blog), 2017, https://idea.popcount.org/2017-03-20-epoll-is-fundamentally-broken-22/, weak, checked 2026-10-19
  - supports: DM-023 (events on already-closed fds; EPOLL_CTL_DEL impossible after close of original; illumos declined to copy)
[S406] Jonathan Corbet, Fixing asynchronous I/O, again, LWN.net, 2016, https://lwn.net/Articles/671649/, secondary, checked 2026-10-19
  - supports: DM-024 (native AIO: O_DIRECT only, can still block, few ops, not exposed by glibc; glibc POSIX AIO uses threads; syslets/fibrils/acall never merged)
[S407] Linus Torvalds, Re: [PATCH 09/13] aio: add support for async openat() (LKML message, mirrored by LWN), LWN.net, 2016, https://lwn.net/Articles/671657/, primary, checked 2026-10-19
  - supports: DM-024 (Torvalds calls AIO a "horrible ad-hoc design"; wants generic async syscalls instead)
[S408] Linux man-pages project, aio(7), man7.org, current, https://man7.org/linux/man-pages/man7/aio.7.html, primary, checked 2026-10-19
  - supports: DM-024 (glibc POSIX AIO in user space; threads expensive and scale poorly; kernel AIO not mature enough to back POSIX AIO)
[S409] Linux man-pages project, io_submit(2), man7.org, current, https://man7.org/linux/man-pages/man2/io_submit.2.html, primary, checked 2026-10-19
  - supports: DM-024 (no glibc wrapper; libaio wrapper with different prototype; RWF_NOWAIT retrofit)
[S410] Jens Axboe, Efficient IO with io_uring (v0.4), kernel.dk (author's site; fetched via Internet Archive copy), 2019, https://web.archive.org/web/2023id_/https://kernel.dk/io_uring.pdf, primary, checked 2026-10-19
  - supports: DM-024 (AIO limitations: O_DIRECT only, blocking submission on metadata/request slots, 104 bytes copied per IO, two syscalls; attempts to extend AIO abandoned)
  - supports: DM-025 (io_uring design: shared SQ/CQ rings, registered files and buffers, polled IO)
  - supports: DM-026 (completion model, shared rings)
[S411] Jens Axboe, Faster IO through io_uring (slides), Kernel Recipes 2019, 2019, https://archives.kernel-recipes.org/wp-content/uploads/2025/01/kernel-recipes-2019-axboe.pdf, primary, checked 2026-10-19
  - supports: DM-024 (buffered AIO unsupported; quoted Torvalds objections about async code being hard and racing with fd close)
  - supports: DM-025 (io_uring merged 5.1, posted Jan 2019 merged Mar 2019)
[S412] Google (kCTF VRP team), Learnings from kCTF VRP's 42 Linux kernel exploits submissions, Google Security Blog, 2023, https://security.googleblog.com/2023/06/learnings-from-kctf-vrps-42-linux.html, primary, checked 2026-10-19
  - supports: DM-025 (existence, title and date of Google's post; body content verified via S413)
[S413] Tamás Koczka, Learnings from kCTF VRP's 42 Linux kernel exploits submissions (repost to oss-security), Openwall oss-security list, 2023, https://openwall.com/lists/oss-security/2023/06/17/2, primary, checked 2026-10-19
  - supports: DM-025 (60% of submissions exploited io_uring; disabled on ChromeOS and Google production servers; blocked for Android apps via seccomp; GKE Autopilot considering disabling)
[S414] Matteo Rizzo, [PATCH 0/1] Add a sysctl to disable io_uring system-wide, LKML, 2023, https://lkml.rescloud.iu.edu/2306.3/02831.html, primary, checked 2026-10-19
  - supports: DM-025 (many unprivileged-exploitable io_uring bugs; seccomp only per-process; only option was compiling it out)
[S415] Linux kernel documentation, Documentation for /proc/sys/kernel/ (io_uring_disabled, io_uring_group), docs.kernel.org, current, https://docs.kernel.org/admin-guide/sysctl/kernel.html, primary, checked 2026-10-19
  - supports: DM-025 (io_uring_disabled 0/1/2 and io_uring_group semantics)
[S416] Jonathan Corbet, Task-level io_uring restrictions, LWN.net, 2026, https://lwn.net/Articles/1054225/, secondary, checked 2026-10-19
  - supports: DM-025 (seccomp has no visibility into io_uring operations; per-ring restrictions; cBPF filter work in flux)
[S417] Microsoft, I/O Completion Ports, Microsoft Learn (Win32 docs), 2025, https://learn.microsoft.com/en-us/windows/win32/fileio/i-o-completion-ports, primary, checked 2026-10-19
  - supports: DM-026 (completion packets per handle; concurrency value; LIFO thread release; works for files, sockets, pipes)
  - supports: DM-024 (Windows offers one completion mechanism across files and sockets)
  - supports: DM-029 (overlapped I/O on files completes via the same port as sockets)
[S418] libuv project, Design overview, docs.libuv.org, current, https://docs.libuv.org/en/v1.x/design.html, primary, checked 2026-10-19
  - supports: DM-026 (one library over epoll/kqueue/event ports/IOCP)
  - supports: DM-029 (file I/O done on a thread pool because no usable platform async file primitives)
[S419] without.boats (Saoirse Shirley), Notes on io-uring, without.boats (personal blog of Rust async designer), 2020, https://without.boats/blog/io-uring/, weak, checked 2026-10-19
  - supports: DM-026 (completion I/O needs kernel-owned buffers; cancellation on drop unsound with borrowed buffers; AsyncRead mismatch)
[S420] liburing / Linux man-pages, io_uring_register_buf_ring(3), man7.org, current, https://man7.org/linux/man-pages/man3/io_uring_register_buf_ring.3.html, primary, checked 2026-10-19
  - supports: DM-026 (provided buffers chosen when data is ready; more operations in flight than buffers)
[S421] Bert Belder, wepoll README, GitHub, current, https://github.com/piscisaureus/wepoll, weak, checked 2026-10-19
  - supports: DM-026 (cross-platform software designed for readiness finds IOCP impractical; epoll emulation for Windows)
[S422] Richard P. Gabriel, The Rise of "Worse is Better" (from Lisp: Good News, Bad News, How to Win Big), dreamsongs.com (author's site), 1989/1991, https://www.dreamsongs.com/RiseOfWorseIsBetter.html, primary, checked 2026-10-19
  - supports: DM-027 (PC loser-ing problem; Unix returns error and makes caller retry; implementation simplicity over interface simplicity)
[S423] Linux man-pages project, signal(7), man7.org, current, https://man7.org/linux/man-pages/man7/signal.7.html, primary, checked 2026-10-19
  - supports: DM-027 (SA_RESTART lists; calls never restarted: poll/epoll_wait/select/nanosleep/SO_RCVTIMEO sockets; Linux EINTR after SIGSTOP/SIGCONT with no handler)
[S424] Go project, Go 1.14 Release Notes, go.dev, 2020, https://go.dev/doc/go1.14, primary, checked 2026-10-19
  - supports: DM-027 (async preemption sends more signals; programs see more EINTR and must loop)
[S425] Go project, os: write should handle EINTR (issue #38033), GitHub golang/go, 2020, https://github.com/golang/go/issues/38033, primary, checked 2026-10-19
  - supports: DM-027 (writes to cgroupfs failing with EINTR under Go 1.14 despite SA_RESTART)
[S426] Go project, os: write should handle EINTR [1.14 backport] (issue #39026), GitHub golang/go, 2020, https://github.com/golang/go/issues/39026, primary, checked 2026-10-19
  - supports: DM-027 (Ian Lance Taylor: 1.14 made existing EINTR problem occur more often; fix too risky to backport)
[S427] Jonathan Corbet, TASK_KILLABLE, LWN.net, 2008, https://lwn.net/Articles/288056/, secondary, checked 2026-10-19
  - supports: DM-028 (D-state unkillable hangs; Alan Cox: apps assume file writes not interruptible; Wilcox's killable sleep in 2.6.25; little adoption, NFS converted)
  - supports: DM-027 (interruptible sleeps require kernel cleanup paths and apps mishandle interrupted calls)
[S428] Linux man-pages / nfs-utils, nfs(5), man7.org, current, https://man7.org/linux/man-pages/man5/nfs.5.html, primary, checked 2026-10-19
  - supports: DM-028 (hard mounts retry indefinitely; intr ignored after 2.6.25; soft timeouts can cause silent data corruption)
[S429] Simon Kirby, Hung task detector versus NFS (TASK_KILLABLE), LKML, 2016, https://lkml.iu.edu/hypermail/linux/kernel/1603.0/06299.html, primary, checked 2026-10-19
  - supports: DM-028 (VFS paths take mutexes in uninterruptible sleep behind a killable NFS waiter; collateral D-state victims)
[S430] Microsoft, Canceling IRPs, Microsoft Learn (Windows driver docs), 2017, https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/canceling-irps, primary, checked 2026-10-19
  - supports: DM-028 (NT drivers with indefinitely queued IRPs must support cancel; IRPs cancelled on thread exit; 5-minute timeout disassociates IRP)
  - supports: DM-027 (NT models I/O cancellation as an explicit request operation rather than a signal side effect)
[S431] The Open Group, pselect, select (POSIX.1-2024, XSH), The Open Group Base Specifications Issue 8, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/pselect.html, primary, checked 2026-10-19
  - supports: DM-029 (regular files shall always select true for read, write and error)
[S432] Jonathan Corbet, Non-blocking buffered file read operations, LWN.net, 2014, https://lwn.net/Articles/612483/, secondary, checked 2026-10-19
  - supports: DM-029 (buffered reads block on page cache; thread pools as workaround; O_NONBLOCK on files cannot be repurposed because Samba/squid rely on it being ignored; fincore race)
[S433] Linux man-pages project, preadv2(2) (readv(2)), man7.org, current, https://man7.org/linux/man-pages/man2/preadv2.2.html, primary, checked 2026-10-19
  - supports: DM-029 (RWF_NOWAIT since 4.14, reads only, EAGAIN; 5.9/5.10 bug returning 0)
[S434] T. Pauly, B. Trammell, A. Brunstrom, G. Fairhurst, C. S. Perkins (eds./authors), RFC 9621: Architecture and Requirements for Transport Services, IETF / RFC Editor, 2025, https://www.rfc-editor.org/rfc/rfc9621.html, primary, checked 2026-10-19
  - supports: DM-074 (sockets: protocol-specific calls, single address/interface, stream-only TCP, try-and-fail async, racing reimplemented per app)
[S435] Dave Presotto, Phil Winterbottom, The Organization of Networks in Plan 9, USENIX Winter Conference 1993 (Plan 9 docs), 1993, https://plan9.io/sys/doc/net/net.html, primary, checked 2026-10-19
  - supports: DM-074 (/net clone/ctl/data files; dial() with textual addresses; only name resolution differs between networks, done by user-level cs)
  - supports: DM-075 (name resolution in a user-level connection server rather than linked into every program)
[S436] R. Gilligan, S. Thomson, J. Bound, J. McCann, W. Stevens, RFC 3493: Basic Socket Interface Extensions for IPv6, IETF / RFC Editor, 2003, https://www.rfc-editor.org/rfc/rfc3493.html, primary, checked 2026-10-19
  - supports: DM-074 (sockaddr_in cannot hold IPv6; sockaddr_storage aligned for casting; gethostbyname inadequate; minimal-change goal)
[S437] Linux man-pages project, accept(2), man7.org, current, https://man7.org/linux/man-pages/man2/accept.2.html, primary, checked 2026-10-19
  - supports: DM-074 (addrlen type history int -> size_t draft -> socklen_t; silent truncation of returned address)
[S438] Linux man-pages project, getaddrinfo(3), man7.org, current, https://man7.org/linux/man-pages/man3/getaddrinfo.3.html, primary, checked 2026-10-19
  - supports: DM-074 (getaddrinfo removes IPv4/IPv6 dependencies; returns sockaddr usable with connect/bind)
  - supports: DM-075 (synchronous interface; "potentially lengthy" lookups; RFC 3484 sorting via gai.conf)
[S439] Linux man-pages project, getaddrinfo_a(3), man7.org, current, https://man7.org/linux/man-pages/man3/getaddrinfo_a.3.html, primary, checked 2026-10-19
  - supports: DM-075 (GNU-only async lookup since glibc 2.2.3; signal or new-thread notification; in-progress requests cannot be cancelled)
[S440] Go project, package net (Name Resolution section), pkg.go.dev, current, https://pkg.go.dev/net, primary, checked 2026-10-19
  - supports: DM-075 (blocked C lookup consumes an OS thread; pure-Go resolver preferred; falls back to cgo for nsswitch features and on macOS; 500-lookup cap)
[S441] Linux man-pages project, nsswitch.conf(5), man7.org, current, https://man7.org/linux/man-pages/man5/nsswitch.conf.5.html, primary, checked 2026-10-19
  - supports: DM-075 (services provided by libnss_SERVICE.so.X shared libraries loaded by name)
[S442] musl libc project, Functional differences from glibc (Name Resolver/DNS), musl wiki, current, https://wiki.musl-libc.org/functional-differences-from-glibc.html, primary, checked 2026-10-19
  - supports: DM-075 (different resolver semantics: parallel nameserver queries; no DNS-over-TCP until 1.2.4)
[S443] Solar Designer, Re: The GNU C Library security advisories update for 2025-05-16 (CVE-2025-4802), Openwall oss-security list, 2025, https://seclists.org/oss-sec/2025/q2/161, primary, checked 2026-10-19
  - supports: DM-075 (static setuid binaries calling getaddrinfo/NSS dlopen could load attacker library via LD_LIBRARY_PATH, glibc 2.27-2.39)
[S444] Microsoft, GetAddrInfoExW function, Microsoft Learn (Win32 API), 2018, https://learn.microsoft.com/en-us/windows/win32/api/ws2tcpip/nf-ws2tcpip-getaddrinfoexw, primary, checked 2026-10-19
  - supports: DM-075 (async name resolution with OVERLAPPED/completion routine and GetAddrInfoExCancel, only from Windows 8)
[S445] Linux kernel documentation, IP Sysctl (ip_unprivileged_port_start, ping_group_range), docs.kernel.org, current, https://docs.kernel.org/networking/ip-sysctl.html, primary, checked 2026-10-19
  - supports: DM-076 (per-namespace first unprivileged port; root or CAP_NET_BIND_SERVICE; 0 disables; default 1024)
[S446] Linux man-pages project, rcmd(3), man7.org, current, https://man7.org/linux/man-pages/man3/rcmd.3.html, primary, checked 2026-10-19
  - supports: DM-076 (rcmd authentication "based on privileged port numbers"; rresvport; ruserok/.rhosts)
[S447] Krister Johansen (committed by David S. Miller), commit 4548b683b781: Introduce a sysctl that modifies the value of PROT_SOCK, Linux kernel git (android-kvm mirror), 2017, https://android-kvm.googlesource.com/linux/+/4548b683b78137f8eadeb312b94e20bb0d4a7141, primary, checked 2026-10-19
  - supports: DM-076 (sysctl added so container managers can lift privileged ports without giving up netns control)
[S448] Quinn "The Eskimo!" (Apple DTS) et al., Binding on priviledged ports on macOS, Apple Developer Forums, 2021, https://developer.apple.com/forums/thread/674179, secondary, checked 2026-10-19
  - supports: DM-076 (Apple engineer: iOS never had the restriction, lifted on macOS Mojave intentionally, never documented)
[S449] Justin Cormack, Add default sysctls to allow ping sockets and privileged ports with no capabilities (moby PR #41030), GitHub moby/moby, 2020, https://github.com/moby/moby/pull/41030, primary, checked 2026-10-19
  - supports: DM-076 (Docker sets ip_unprivileged_port_start=0 in containers by default; host-network caveat)
[S450] Linux man-pages project, pipe(7), man7.org, current, https://man7.org/linux/man-pages/man7/pipe.7.html, primary, checked 2026-10-19
  - supports: DM-077 (write to pipe with no readers raises SIGPIPE; EPIPE if ignored)
[S451] Linux man-pages project, send(2), man7.org, current, https://man7.org/linux/man-pages/man2/send.2.html, primary, checked 2026-10-19
  - supports: DM-077 (MSG_NOSIGNAL since Linux 2.2, per-call vs process-wide ignore; added to POSIX.1-2008)
[S452] The Open Group, write (POSIX.1-2024, XSH), The Open Group Base Specifications Issue 8, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/write.html, primary, checked 2026-10-19
  - supports: DM-077 (SIGPIPE sent for pipes with no reader and for SOCK_STREAM sockets no longer connected)
[S453] FreeBSD Project, setsockopt(2) manual page (SO_NOSIGPIPE), FreeBSD, current, https://man.freebsd.org/cgi/man.cgi?query=setsockopt&sektion=2, primary, checked 2026-10-19
  - supports: DM-077 (per-socket SO_NOSIGPIPE option)
[S454] Python Software Foundation, signal — Set handlers for asynchronous events, Python 3 documentation, current, https://docs.python.org/3/library/signal.html, primary, checked 2026-10-19
  - supports: DM-077 (Python ignores SIGPIPE at startup so errors become exceptions; BrokenPipeError when piping to head)
[S455] Josh Triplett et al., Should Rust still ignore SIGPIPE by default? (issue #62569), GitHub rust-lang/rust, 2019, https://github.com/rust-lang/rust/issues/62569, primary, checked 2026-10-19
  - supports: DM-077 (Rust startup ignores SIGPIPE since 2014; process-wide surprise; libraries vs applications differ)
[S456] Simon Peter, Jialin Li, Irene Zhang, Dan R. K. Ports, Doug Woos, Arvind Krishnamurthy, Thomas Anderson, Timothy Roscoe, Arrakis: The Operating System is the Control Plane, OSDI 2014, 2014, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/peter, primary, checked 2026-10-19
  - supports: DM-078 (kernel as control plane; applications get virtualized devices; 2-5x latency, 9x throughput vs Linux)
[S457] Adam Belay, George Prekas, Ana Klimovic, Samuel Grossman, Christos Kozyrakis, Edouard Bugnion, IX: A Protected Dataplane Operating System for High Throughput and Low Latency, OSDI 2014, 2014, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/belay, primary, checked 2026-10-19
  - supports: DM-078 (protected dataplane via virtualization; run to completion; up to 3.6x throughput vs Linux)
[S458] Michael Marty et al., Snap: a Microkernel Approach to Host Networking, SOSP 2019, 2019, https://research.google/pubs/snap-a-microkernel-approach-to-host-networking/, primary, checked 2026-10-19
  - supports: DM-078 (userspace networking service in production >3 years, >half of Google fleet; transparent upgrades; 3x Gbps/core)
[S459] DPDK Project, Programmer's Guide: Overview, doc.dpdk.org, current, https://doc.dpdk.org/guides/prog_guide/overview.html, primary, checked 2026-10-19
  - supports: DM-078 (poll-mode drivers without interrupts; run-to-completion model)
[S460] Toke Høiland-Jørgensen, Jesper Dangaard Brouer, Daniel Borkmann, John Fastabend, Tom Herbert, David Ahern, David Miller, The eXpress Data Path: Fast Programmable Packet Processing in the Operating System Kernel, ACM CoNEXT 2018, 2018, https://courses.grainger.illinois.edu/ece598hpn/fa2023/papers/xdp.pdf, primary, checked 2026-10-19
  - supports: DM-078 (bypass loses isolation, security and tooling; dedicated cores; reimplementing routing; XDP/AF_XDP middle ground)
[S461] Marek Majkowski, Why we use the Linux kernel's TCP stack, Cloudflare blog, 2016, https://blog.cloudflare.com/why-we-use-the-linux-kernels-tcp-stack/, weak, checked 2026-10-19
  - supports: DM-078 (practitioner view: bypass breaks tcpdump/netstat, monopolises NIC, lacks DDoS features; ~5-10% CPU gain)
[S462] Fuchsia project, Netstack3 – A Fuchsia owned rust based netstack (2021 roadmap), fuchsia.dev, 2021, https://fuchsia.dev/fuchsia-src/contribute/roadmap/2021/netstack3, primary, checked 2026-10-19
  - supports: DM-078 (Fuchsia replacing gVisor-derived Go netstack with Rust netstack; GC language and ownership concerns; performance parity a risk)
[S463] Microsoft, WaitForMultipleObjects function, Microsoft Learn (Win32 API), 2018, https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects, primary, checked 2026-10-19
  - supports: DM-130 (one wait over processes, threads, events, mutexes, semaphores, timers, change notifications; MAXIMUM_WAIT_OBJECTS limit)
[S464] Linux man-pages project, pidfd_open(2), man7.org, current, https://man7.org/linux/man-pages/man2/pidfd_open.2.html, primary, checked 2026-10-19
  - supports: DM-130 (Linux 5.3; process exit waitable via poll/epoll; avoids PID recycling; caveats with SIGCHLD SIG_IGN)
[S465] Linux man-pages project, signalfd(2), man7.org, current, https://man7.org/linux/man-pages/man2/signalfd.2.html, primary, checked 2026-10-19
  - supports: DM-130 (signals must be blocked with sigprocmask for signalfd; mask and pending signals persist across execve)
[S466] Fuchsia project, Port (Zircon kernel object), fuchsia.dev, current, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/port, primary, checked 2026-10-19
  - supports: DM-130 (port receives user packets, async waits on any bound handle, IPC delivery)
[S467] Fuchsia project, zx_object_wait_async, fuchsia.dev, current, https://fuchsia.dev/fuchsia-src/reference/syscalls/object_wait_async, primary, checked 2026-10-19
  - supports: DM-130 (any waitable object's signals delivered as packets to a port; one packet per wait; edge mode can miss transitions)
