# TASK-23: Design for networking (research and proposed ADRs)

## Where and how this runs

Locally on the architect's Mac mini, **in its own git worktree**, at the same time as TASK-22 (stage 6e), which is the machine's one build-and-test agent. This task is **documents only**: no builds of the repository, no `make`, no QEMU, no mutation runs. The only code allowed is `cargo check` of a candidate crate for `aarch64-unknown-none` in a scratch directory outside the repository, with `CARGO_BUILD_JOBS=2`, to confirm a claim such as "builds without `std`"; record each such check in the survey. Needs web access.

## Why

Zoraya has no network, and no plan has one. Almost any real use needs it, and the isolation direction most of all: an AI agent in an untrusted room (ADR-0172) is useful only with some network, and dangerous with all of it. Networking is also where ambient authority is most entrenched elsewhere: on Unix any process may open a socket to anywhere. In Zoraya a program reaches only what it is given (ADR-0002); the network must follow the same rule, visibly, in the shell and on the desktop.

The architect's direction stands as for every part: simple and unsurprising (FreeBSD's POLA), one coherent design, native to the capability model, POSIX as a personality on top.

## Context

Read first: `CLAUDE.md`, `docs/design.md` (Vision, Known limitations), `docs/adr/index.md`, `docs/backlog.md`.

Read **in full**: ADR-0002, ADR-0006, ADR-0050, ADR-0051, ADR-0052, ADR-0053, ADR-0054, ADR-0055 (as the model of a service protocol), ADR-0056 (quotas, proposed), ADR-0100, ADR-0101, ADR-0104, ADR-0106, ADR-0107, ADR-0130, ADR-0153 (grants in the shell), ADR-0167 (granting in the GUI), ADR-0172 (rooms), ADR-0191 (the time base). Read the research catalog (`docs/research/design-mistakes.md`, `held-up-well.md`, `alternatives-postmortem.md`) for everything on networking, sockets, name resolution and network permissions, and the Plan 9, Fuchsia, Genode and KeyKOS/EROS sections. Read TASK-02's measured POSIX surface for any socket calls (`docs/research/task-02/`).

ADR numbers **0220 to 0239** are reserved for this task.

## Part 1: research

Write `docs/research/network/survey.md`, same evidence rules as TASK-01 (primary sources preferred, every URL fetched, never invent a citation, paraphrase):

- **How systems grant network access**, and what went wrong: the Unix baseline (any process, any socket); OpenBSD `pledge("inet")` and `unveil`; Capsicum and Casper's network services; Plan 9's `/net` and dial strings; Fuchsia's socket provider and its capabilities; Genode's NIC router and per-component network sessions; WASI sockets (pre-opened sockets, allow-lists); Android's `INTERNET` permission (too coarse, and why); macOS sandbox network entitlements; how container and agent sandboxes limit egress today (allow-lists of hosts, proxies). What a grant names in each: an interface, a protocol, a host, a port, a name.
- **Name resolution as authority:** DNS can carry data out of a sandbox; how systems scope it.
- **TCP/IP stacks usable here:** smoltcp (Rust, `no_std`), lwIP (C), others you find; licence, `no_std` and allocation needs, TCP features (window scaling, retransmission, congestion control), IPv4/IPv6, DHCP and DNS clients, maturity, known bugs. A stack as one service, or as a library per process with a multiplexer in front (Genode's model): costs and isolation of each.
- **TLS:** what a Zoraya program needs to fetch over HTTPS; rustls (its `no_std` status, record exactly what it requires), BearSSL, mbedTLS; certificate validation needs **a real wall clock** (today's time base is seconds since the volume's first use, ADR-0191) and **entropy** (virtio-rng, FEAT_RNG `RNDR` and whether HVF on the architect's Macs exposes it).
- **The device:** virtio-net under QEMU `virt` as a user-space driver (ADR-0054, `dma_pin`), multiqueue or not, offloads to ignore.
- **QEMU networking for development and CI:** user-mode networking (slirp) with `hostfwd`/`guestfwd`, `vmnet` on macOS, what GitHub's runners allow, packet capture (`filter-dump`), and how a deterministic network test runs in `make test` without the internet.
- **POSIX sockets:** which socket calls the programs Zoraya will want need (measure where TASK-02 did not: for example `curl` or `wget` built with tcc against a small TLS library, `git` over HTTPS later), with the same method as TASK-02 where you can run it, or from source reading where you cannot, stated as such.

## Part 2: decisions (proposed ADRs, one decision per ADR)

Template-exact, `proposed`, "Architect's decision" empty, at least three options each with a Unix/Linux baseline and a non-Unix precedent, steelmanned, with "Effect of the vision" and **examples of what a person types and sees** in `zosh` and on the desktop where the decision shows there. At least:

1. **The network capability.** What a program is given to reach the network: an interface, a protocol, a set of destinations, a listening port; how it is narrowed and passed on; what a program without it can do (nothing, not even resolve a name). How it fits ADR-0002's rule and the handle rights of ADR-0057.
2. **Where the stack lives.** One network service (`netd`) owning the NIC, or stacks per process behind a multiplexer; the driver as its own process (ADR-0054); restart and failure behaviour; how a dead `netd` looks to its clients.
3. **The protocol between programs and the stack:** connections as byte streams (ADR-0101) or as messages; datagrams; non-blocking use and readiness (with `pselect` from 6d); a protocol crate as ADR-0052 makes them.
4. **Name resolution:** who resolves, under which capability, caching, `/etc/hosts`-like local names, and how a grant names a host by name when its addresses change.
5. **Grants in the shell and the desktop:** the `zosh` syntax next to `+r` and `+w` (ADR-0153), what `grants` prints, what a command's description can declare; network in rooms (ADR-0172): what an untrusted room may reach, how the compositor shows it in text (rule 3), what the person sees when a program is refused (ADR-0153's REPORT hint).
6. **POSIX sockets:** the tier of socket calls `libposix` implements, mapped onto the native capability; what `socket()` without a grant returns; `getaddrinfo`.
7. **Wall clock and entropy:** the source of real time (QEMU's PL031 RTC, later NTP) and how it relates to ADR-0191's time base for files; the entropy source and its service. TLS cannot validate certificates without both.
8. **TLS:** in each program, or as a service that holds no keys of the program; which library.
9. **Configuration:** addresses, DHCP, DNS servers in `/config/settings` (ADR-0157), one source of truth.
10. **Testing:** the network in `make test` and CI without the internet: QEMU user-mode networking, a test server in the runner, packet capture on failure; what a network test may assume.

Merge or split with reasons. Name the relation to ADR-0056: what network quotas (connections, buffers, bandwidth) it will have to cover.

For each ADR give the invariants a later implementation can check, and how.

Also write `docs/research/network/plan.md`: the implementation order in tasks of comparable size, each with a visible result (for example: the driver answers ARP; `netd` answers ping from the host; a native program fetches a page over HTTP from the runner's test server; a POSIX program does the same; HTTPS with a real clock), with dependencies on stage 6 and stage 7.

## Review

Independent review by a subagent that did not write the ADRs: citations fetched and accurate; options not strawmanned; consistency with accepted ADRs (list every tension explicitly, especially with ADR-0002, ADR-0153, ADR-0172 and ADR-0191); whether the capability is both safe and short to type; licences of every candidate crate and library. Findings and dispositions in `docs/research/network/review-notes.md`; fix them; one check of the changed lines; if it finds errors, fix and repeat that check at most once more, then report.

## Git

- Branch `adr/network-design` from the latest `main`, in its own worktree (for example `../zoraya-net`).
- Configured identity only.
- Commit after each part. **You may push this one branch and nothing else.** Never merge, never push other branches, never force-push.
- Touch only `docs/adr/0220-*` to `docs/adr/0239-*` and `docs/research/network/`. List the `index.md`, `invariants.md` and roadmap rows to add in `docs/research/network/rows-to-add.md` and in the final report.

## Definition of done

- Survey; proposed ADRs for decisions 1–10 (merged or split with reasons), each with "Effect of the vision", examples where a person sees it, and invariants.
- `plan.md`, review and the check of fixes done; branch pushed.
- Final report: each ADR's recommendation in one line; the tensions with accepted ADRs; what ADR-0056 must add for the network; the questions for the architect, first among them what the network capability names.

## Out of scope

Any code in the repository; drivers, `netd` and sockets themselves (implementation tasks after acceptance); servers beyond what the design must not preclude; wireless; IPv6 beyond the design not precluding it; firewalls as a separate concept (the capability is the filter).
