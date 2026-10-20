# TASK-01 index: candidate entries

Index phase output for [TASK-01](TASK-01-design-mistakes-research.md). Every row is a **hypothesis** to be
verified in the research phase. IDs are assigned here and are stable: an item that is dropped or merged keeps
its ID and is marked `dropped` / `merged into DM-NNN` in the catalog, never reused.

Source IDs in `sources.md` are allocated in blocks per research stream (S100–S199 for stream A, S200–S299 for
stream B, and so on) so parallel work never collides. Gaps in the numbering are expected.

## Design mistakes and tradeoffs (DM)

### 1. Process model and program execution (stream A)

| ID | Candidate | Note |
|----|-----------|------|
| DM-001 | `fork()` as the primary process-creation primitive | incl. fork in multithreaded processes, overcommit interaction |
| DM-002 | File descriptors inherited across `exec` by default (`O_CLOEXEC` retrofit and its races) | |
| DM-003 | PID as a reusable integer name; PID-reuse races; `pidfd` retrofit | |
| DM-004 | Threads added late to the Unix process model | signals, per-process vs per-thread credentials, `setuid` in threads |
| DM-005 | Zombies, `wait` semantics and reparenting to init | subreaper retrofit |
| DM-006 | Process-wide mutable state: cwd, umask, signal dispositions | thread-unsafe globals |
| DM-007 | Shared-library resolution: DLL search order / DLL hell, `LD_LIBRARY_PATH`, soname versioning | |
| DM-008 | `argv` as an unstructured string vector; quoting delegated to every shell and program | Windows passes one string — compare |

### 2. Security, authority and identity (stream B)

| ID | Candidate | Note |
|----|-----------|------|
| DM-009 | Ambient authority: a process may name and open anything its user can | confused deputy |
| DM-010 | Access control keyed on user identity, not on program or request (uid/gid DAC) | |
| DM-011 | `setuid`/`setgid` binaries | |
| DM-012 | Root as an all-powerful superuser | |
| DM-013 | Linux "POSIX capabilities" vs real capability systems (`CAP_SYS_ADMIN`) | |
| DM-014 | Sandboxing retrofitted as syscall filters (seccomp-bpf) vs pledge/unveil vs Capsicum | tradeoff |
| DM-015 | Mandatory access control retrofitted via LSM (SELinux/AppArmor) | complexity, adoption |
| DM-016 | Windows security descriptors/ACL complexity and token model | possibly mixed |

### 3. Inter-process communication (stream C)

| ID | Candidate | Note |
|----|-----------|------|
| DM-017 | Signals as a notification/IPC mechanism; async-signal safety | |
| DM-018 | Mach IPC performance and the L4 response | |
| DM-019 | System V IPC: separate key namespace, objects outside the fd model | |
| DM-020 | Untyped byte streams as the only composition medium | contested |
| DM-021 | Userspace message bus retrofitted (D-Bus; kdbus/bus1 rejected) | contested |
| DM-022 | Descriptor passing over Unix sockets (`SCM_RIGHTS`): API shape and in-flight GC bugs | |

### 4. I/O, asynchrony and event handling (stream D)

| ID | Candidate | Note |
|----|-----------|------|
| DM-023 | `select` → `poll` → `epoll` evolution; epoll's fd-vs-file-description semantics | |
| DM-024 | POSIX AIO and Linux native AIO limitations | |
| DM-025 | `io_uring`: performance win vs security attack surface | tradeoff |
| DM-026 | Readiness-based vs completion-based I/O models | contested |
| DM-027 | `EINTR` and restartable system calls ("PC loser-ing") | |
| DM-028 | Uninterruptible sleep (D state); hung NFS | |
| DM-029 | Regular files always "ready"; no non-blocking disk I/O in the readiness model | |

### 5. Memory management (stream E)

| ID | Candidate | Note |
|----|-----------|------|
| DM-030 | Memory overcommit and the OOM killer | |
| DM-031 | `mmap`'d files: errors surface as `SIGBUS`; truncation hazards | |
| DM-032 | Executable stacks/heaps by default; W^X retrofitted | |
| DM-033 | Kernel mapped into every user address space; Meltdown/KPTI | right-at-the-time? |
| DM-034 | No first-class memory-pressure signalling to applications (PSI, low-memory notifications) | |
| DM-035 | `brk`/`sbrk` legacy heap interface | may drop |

### 6. Filesystems, namespaces and storage semantics (stream F)

| ID | Candidate | Note |
|----|-----------|------|
| DM-036 | POSIX advisory record locks released on any `close()` of the file | OFD locks retrofit |
| DM-037 | `fsync` error semantics ("fsyncgate", 2018) | |
| DM-038 | `rename`/write ordering and application data loss on crash (ext4 delalloc, 2009) | |
| DM-039 | Path-based TOCTOU and symlink races | `openat2` `RESOLVE_*` retrofit |
| DM-040 | Hard links: security problems and semantic complications | |
| DM-041 | Single global mount namespace; mounting requires root | |
| DM-042 | `atime` updates on read | |
| DM-043 | Windows path limits (`MAX_PATH`) and reserved device names | |
| DM-044 | Windows share modes / mandatory locking blocking updates of in-use files | tradeoff |
| DM-045 | No transactional filesystem API (Windows TxF deprecated) | failed alternative |

### 7. Kernel structure, drivers and fault isolation (stream G)

| ID | Candidate | Note |
|----|-----------|------|
| DM-046 | Monolithic kernel vs microkernel | contested |
| DM-047 | Driver bugs as a dominant cause of OS crashes; fault-isolation approaches | |
| DM-048 | Memory-unsafe implementation language (C) for kernels | |
| DM-049 | No stable in-kernel driver API/ABI (Linux) | contested |
| DM-050 | Loadable modules / third-party kernel code with full privilege (e.g. July 2024 CrowdStrike outage) | |
| DM-051 | Kernel crash = system crash; no component restart | MINIX 3 reincarnation as alternative |

### 8. Kernel–user interfaces and ABI design (stream G)

| ID | Candidate | Note |
|----|-----------|------|
| DM-052 | `errno` and integer error codes | |
| DM-053 | `ioctl` as an untyped escape hatch | |
| DM-054 | Text-based `/proc` and `/sys` interfaces | |
| DM-055 | Unchecked flags and fixed structs: syscall extensibility (`clone3`/`openat2` extensible-struct retrofit) | |
| DM-056 | Fixed-width types baked into the ABI (`off_t`, `time_t`, `stat64` proliferation) | |
| DM-057 | Where the stable ABI lives: syscalls (Linux) vs libc/ntdll (BSD, Windows, macOS) | tradeoff |

### 9. Concurrency and synchronization primitives (stream C)

| ID | Candidate | Note |
|----|-----------|------|
| DM-058 | Big Kernel Lock; SMP retrofitted onto uniprocessor kernels | right-at-the-time |
| DM-059 | Thread cancellation (`pthread_cancel`) | |
| DM-060 | Non-reentrant libc interfaces with hidden global state (`strtok`, `localtime`, `errno` before TLS) | |
| DM-061 | Priority inversion; priority inheritance as an afterthought (Mars Pathfinder) | |
| DM-062 | Kernel memory model specified after the fact (LKMM) | |

### 10. Time, clocks and scheduling (stream E)

| ID | Candidate | Note |
|----|-----------|------|
| DM-063 | 32-bit `time_t` and the year 2038 | |
| DM-064 | Leap seconds in POSIX time | |
| DM-065 | Wall clock vs monotonic clock misuse; monotonic clocks added late | |
| DM-066 | Time zones via `TZ` and process-global state | |
| DM-067 | Fixed periodic timer tick (HZ) | right-at-the-time |
| DM-068 | General-purpose scheduler design churn (O(1) → CFS → EEVDF; sched_ext) | contested |

### 11. Terminals, sessions and consoles (stream H)

| ID | Candidate | Note |
|----|-----------|------|
| DM-069 | Kernel TTY layer and line disciplines | |
| DM-070 | Process groups, sessions, controlling terminals, job control, `SIGHUP` | |
| DM-071 | In-band escape sequences for terminal control; escape-sequence injection | |
| DM-072 | `TIOCSTI` input injection | |
| DM-073 | Windows console API vs VT sequences (ConPTY retrofit) | |

### 12. Networking APIs (stream D)

| ID | Candidate | Note |
|----|-----------|------|
| DM-074 | BSD sockets API design (`sockaddr` casts, protocol-specific structs) | |
| DM-075 | Name resolution in libc: blocking, no async, NSS | |
| DM-076 | Privileged ports below 1024 | |
| DM-077 | `SIGPIPE` as default on writing to a closed socket/pipe | |
| DM-078 | Network stack in the kernel vs user-space stacks | contested |

### 13. Graphics, input and window systems (stream H)

| ID | Candidate | Note |
|----|-----------|------|
| DM-079 | X11 security model: any client can read input and contents of others | |
| DM-080 | X11 network transparency and server-side rendering | right-at-the-time |
| DM-081 | Windows win32k: window manager and GDI in kernel mode | |
| DM-082 | Window messages across privilege boundaries (shatter attacks, UIPI) | |
| DM-083 | No frame synchronization in X (tearing); "every frame is perfect" | |
| DM-084 | Wayland's minimal core and protocol-extension fragmentation | cost of the alternative; contested |

### 14. Configuration, observability and system management (stream B)

| ID | Candidate | Note |
|----|-----------|------|
| DM-085 | Environment variables as implicit global configuration | `setenv` thread safety, `LD_PRELOAD` |
| DM-086 | Windows registry | |
| DM-087 | Per-program text configuration syntax in `/etc` | |
| DM-088 | Init systems: shell-script SysV init vs systemd | contested |
| DM-089 | Unstructured text logging (syslog) | |

### 15. Boot, update and recovery (stream A)

| ID | Candidate | Note |
|----|-----------|------|
| DM-090 | In-place, non-atomic package updates vs image-based A/B updates | |
| DM-091 | Replacing in-use files requires reboot (Windows) / stale libraries in running processes (Unix) | |
| DM-092 | Firmware and boot chain complexity (UEFI, BootHole, LogoFAIL) | |
| DM-093 | Verified/measured boot absent by default | |

### 16. Text, encoding and locale (stream F)

| ID | Candidate | Note |
|----|-----------|------|
| DM-094 | Case-insensitive filesystems and Unicode normalization of paths | |
| DM-095 | Filenames as arbitrary byte strings (no encoding, control chars, leading `-`) | |
| DM-096 | UCS-2/UTF-16 in Windows NT and the duplicated A/W API | right-at-the-time |
| DM-097 | Locale as process-global state (`setlocale`, locale-dependent parsing) | |

## Held up well (GR) — stream I

| ID | Candidate |
|----|-----------|
| GR-001 | File descriptors as a uniform handle abstraction |
| GR-002 | Pipes and composition of small programs |
| GR-003 | NT handles and the object manager |
| GR-004 | Plan 9 per-process namespaces |
| GR-005 | Explicit process creation (`CreateProcess`, `posix_spawn`, Zircon `launchpad`/process builder) |
| GR-006 | Hierarchical filesystem namespace |
| GR-007 | Demand-paged virtual memory and `mmap` as a unified mechanism |
| GR-008 | Object capabilities (KeyKOS/EROS, seL4, Zircon handles, Capsicum) |
| GR-009 | Completion-based I/O (IOCP, io_uring ring model) |
| GR-010 | Linux "don't break userspace" ABI stability rule |
| GR-011 | Futexes |
| GR-012 | Read-copy-update (RCU) |
| GR-013 | Dynamic tracing (DTrace, eBPF) |
| GR-014 | Checksummed copy-on-write storage (ZFS, Btrfs, APFS) |
| GR-015 | UTF-8 |
| GR-016 | Compositor-owned input and client isolation (Wayland model) |
| GR-017 | Atomic A/B updates and verified boot (ChromeOS, Android) |
| GR-018 | Synchronous message passing with priority inheritance (QNX) |
| GR-019 | Separate address spaces as the default isolation unit |
| GR-020 | Formal verification of a kernel (seL4) |

## Alternative systems for the postmortem — streams J and K

- Stream J: Multics, Plan 9 / Inferno, Mach, L4 family / seL4, QNX, KeyKOS / EROS / CapROS, Singularity / Midori.
- Stream K: Barrelfish, Fuchsia / Zircon (incl. Starnix), Genode, Redox, Haiku / BeOS, Theseus, Hubris, plus
  MINIX 3, Amoeba / Sprite, Exokernel, Oberon (others added if relevant).

## IDs added after the index phase

Allocated from the per-stream reserve blocks during research. See `research-log.md` for the reasons.

| ID | Entry | Stream |
|----|-------|--------|
| DM-100 | No reliable, inescapable process-tree containment | A |
| DM-101 | Containers as an emergent assembly of retrofitted primitives | A |
| DM-110 | Unprivileged user namespaces expose root-only kernel attack surface | B |
| DM-111 | cgroups v1 multiple hierarchies and the v1→v2 redesign | B |
| DM-120 | Futex interface: one multiplexed syscall with accreted operations | C |
| DM-130 | Waitable events outside the descriptor model and their fd retrofits | D |
| DM-160 | Raw user pointers in syscall arguments and double-fetch races | G |
| DM-161 | Driver isolation without DMA isolation | G |
| GR-021 | virtio as a standard paravirtual device interface | I |

No candidate was dropped. Category placement in `design-mistakes.md` follows each entry's **Category** field, which
can differ from the stream that wrote it: for example, DM-101 is in category 2.
