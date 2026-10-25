# TASK-01: Research catalog of operating system design mistakes

## Context

We are designing a new operating system from scratch. It is **not** a Linux clone and does not aim for Linux ABI compatibility in its native interfaces.

- Kernel and core system: Rust (`no_std`), target aarch64, runs under QEMU `virt` on Apple Silicon.
- Planned scope: kernel, own syscall ABI, filesystem, compositor-based GUI (Wayland-like model), ability to host a C compiler (via a sandboxed POSIX compatibility layer, not via native ABI).
- One human architect makes final decisions. AI agents do research and implementation.

The premise of the project: mainstream OSes carry design mistakes that are known but cannot be fixed because of backward compatibility, inherited 1970s Unix assumptions, or the inability of many contributors to agree. We have no users and no compatibility obligations, so we can avoid these mistakes — **but only if we know precisely what they are, why they happened, and what the alternatives cost.**

This task produces the evidence base. It does **not** make design decisions. Decisions come later, as ADRs (Architecture Decision Records) approved by the architect.

## Goal

Produce a verified, sourced catalog of design mistakes and tradeoffs in operating systems, plus the lessons from alternative designs that tried to fix them — including the ones that failed.

## Critical biases to counter

1. **Unix-by-default bias.** Your training data is dominated by Linux/POSIX. Do not treat POSIX behavior as the neutral baseline. For every entry, ask whether the problem exists because of the abstraction itself, not just its implementation.
2. **"Clean design wins" bias.** Many cleaner designs failed in practice (e.g. performance problems of first-generation microkernel IPC, adoption failures of research systems). Record *why* alternatives failed or stayed niche, not only that they look better on paper.
3. **Hindsight bias.** For every mistake, steelman the original decision: what constraints made it reasonable at the time (hardware, threat model, team size, deadlines)? A decision that was right in 1975 and wrong now is a different kind of entry than a decision that was wrong from the start.
4. **Mistake vs. tradeoff.** Classify honestly. Many "mistakes" are tradeoffs where the alternative has real costs. Say so.
5. **Contested topics.** Where experienced practitioners genuinely disagree (e.g. init systems, monolithic vs. microkernel), mark the entry `contested` and present both positions with sources. Do not resolve it yourself.

## Scope

### Systems to cover

- **Mainstream:** Unix/POSIX (as specification), Linux, BSDs, Windows NT, macOS/XNU (incl. Mach heritage).
- **Alternative and research systems** (as sources of alternatives and of failed attempts): Multics, Plan 9 / Inferno, Mach, L4 family and seL4, QNX, KeyKOS / EROS / CapROS, Singularity / Midori, Barrelfish, Fuchsia / Zircon (incl. Starnix), Genode, Redox, Haiku/BeOS, Theseus, Hubris, and others you find relevant.

### Categories (use these as the top-level structure)

1. Process model and program execution
2. Security, authority and identity
3. Inter-process communication
4. I/O, asynchrony and event handling
5. Memory management
6. Filesystems, namespaces and storage semantics
7. Kernel structure, drivers and fault isolation
8. Kernel–user interfaces and ABI design (error reporting, extensibility, typing)
9. Concurrency and synchronization primitives
10. Time, clocks and scheduling
11. Terminals, sessions and consoles
12. Networking APIs
13. Graphics, input and window systems
14. Configuration, observability and system management
15. Boot, update and recovery
16. Text, encoding and locale

### Seed list (hypotheses, NOT facts)

These are starting points only. Verify each one. Drop or reclassify any item whose evidence is weak. The list is not exhaustive — find items not on it.

- `fork()` as the primary process-creation primitive
- Ambient authority; uid/gid-based access control; setuid binaries
- POSIX "capabilities" in Linux vs. real capability systems
- Signals (async-signal safety, interaction with threads)
- PID reuse races; later `pidfd`
- `errno` and integer error codes
- `ioctl` as an untyped escape hatch
- Text-based `/proc` and `/sys` interfaces
- `select` → `poll` → `epoll` → AIO → `io_uring` evolution
- POSIX advisory record locks released on any `close()` of the file
- `fsync` error semantics (incl. the 2018 PostgreSQL "fsyncgate" discussion)
- `rename`/write ordering and application data loss on crash
- Symlink and path-based TOCTOU races
- Memory overcommit and the OOM killer
- Threads added late to the Unix process model
- TTY layer, process groups, sessions, job control
- BSD sockets API design
- X11 security model (any client can read input of others)
- Driver bugs as a dominant cause of OS crashes; fault isolation approaches
- Mach IPC performance and the L4 response
- Windows: registry, DLL search order / DLL hell, NT handle model and `CreateProcess` (likely positive examples — record them as such)
- Linux namespaces and cgroups retrofitted for containers; cgroups v1 → v2
- `time_t` 2038, leap seconds, wall-clock vs. monotonic misuse
- Case-insensitive filesystems and Unicode normalization of paths
- Environment variables as an implicit, global configuration channel

## What Unix and others got right

Create a separate list of design decisions that **held up well** and should be considered for keeping (in some form). Same evidence standard. Example candidates to verify: file descriptors as a uniform handle abstraction, pipes and composition, NT handles and object manager, Plan 9 per-process namespaces. This prevents throwing away good ideas together with bad ones.

## Entry format

Each entry in the catalog uses this exact structure:

```markdown
### DM-<NNN>: <short title>

- **Category:** <one of the 16 categories>
- **Systems affected:** <list>
- **Classification:** mistake | tradeoff | contested | right-at-the-time-wrong-now
- **Confidence:** high | medium | low

**What the design is.** Neutral description, 2–4 sentences.

**Why it was chosen.** Steelman of the original decision and its constraints.

**Why it is a problem.** The concrete mechanism of failure. Name specific failure modes: bug classes, CVE patterns, performance cliffs, API misuse patterns. Prefer concrete examples over general statements.

**Why it was not fixed.** Compatibility, cost, politics, lack of consensus — with evidence.

**Alternatives that exist.** Which systems did it differently, and how.

**Cost of the alternatives.** What the alternatives give up. What went wrong when they were tried.

**Relevance to our OS.** high | medium | low, with one sentence of reasoning.

**Sources.** Numbered references to `sources.md`, e.g. [S12], [S40]. At least 2 sources, at least 1 primary.
```

Use stable IDs (`DM-001`, `DM-002`, ...) and never renumber. Later ADRs will reference them.

## Evidence standards

- **Primary sources preferred:** peer-reviewed papers (SOSP, OSDI, HotOS, EuroSys, USENIX ATC, ASPLOS), kernel mailing list threads, official documentation and specifications, design documents by the system's authors, CVE records, retrospectives by original authors.
- **Good secondary sources:** LWN.net, well-known technical books, conference talks by practitioners.
- **Weak sources:** blog posts, forums, Q&A sites — acceptable only as pointers to primary sources or for anecdotal failure examples, and must be labeled as such.
- **Every URL must be fetched and checked** before it goes into `sources.md`. Record the date checked. If a source cannot be fetched, mark it `unverified` or drop it.
- **Never invent citations.** No paper title, author, year or URL from memory without verification. If you believe a source exists but cannot find it, write the claim as `[unverified]`.
- **Paraphrase.** Do not copy passages from sources. Short quotes (under 15 words) only where exact wording matters.

## Output files

All in `docs/research/`:

1. `design-mistakes.md` — the catalog, grouped by category, entries in the format above.
2. `held-up-well.md` — the "got it right" list, same format, IDs `GR-<NNN>`.
3. `sources.md` — bibliography: `[S<N>] Authors, Title, Venue/Publisher, Year, URL, type (primary/secondary/weak), checked <date>`.
4. `alternatives-postmortem.md` — for each alternative/research system listed in scope: what it tried to fix, what worked, why it did not achieve wide adoption (technical vs. non-technical reasons, with sources).
5. `candidate-principles.md` — a list of candidate design principles derived from the catalog. Each principle: statement, the DM/GR entries it is based on, what it costs, and open questions. **These are candidates for the architect, not decisions.** Mark conflicts between principles explicitly (e.g. capability-only access vs. ease of POSIX porting).
6. `research-log.md` — short log: what was searched, what was dropped and why, open questions you could not resolve.

## Process

1. **Index phase.** Build a list of candidate entries per category (target: 60–100 total, quality over quantity). Commit the index before deep research.
2. **Research phase.** Research entries category by category. Use parallel subagents per category where possible. Each subagent returns entries in the exact format plus raw source list.
3. **Verification phase.** A separate subagent that did not write the entries checks every entry: does each source actually support the claim it is cited for? Are classifications and confidence honest? Is the steelman fair? Fix or downgrade anything that fails.
4. **Synthesis phase.** Write `alternatives-postmortem.md` and `candidate-principles.md` from the verified catalog only.
5. **Report.** Summarize in `research-log.md`: number of entries per category and classification, number of sources by type, list of contested items, list of unresolved questions.

Commit after each phase with a clear message.

## Definition of done

- Every one of the 16 categories has at least 2 entries, or an explicit note explaining why none qualify.
- Every entry has at least 2 sources, at least 1 primary, all fetched and verified.
- Every alternative system in scope has a postmortem section.
- The verification phase has been completed by an independent subagent and its findings are recorded in `research-log.md`.
- `candidate-principles.md` lists conflicts between principles explicitly.

## Out of scope

- Writing any OS code.
- Making final design decisions or writing ADRs.
- Recommending Linux ABI compatibility as the native interface.
- Packaging, distribution and desktop-environment UX beyond the window-system level.
