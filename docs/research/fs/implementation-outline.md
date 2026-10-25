# Filesystem implementation outline (non-binding)

Part of [TASK-06](../../tasks/TASK-06-filesystem-design.md). **Everything here depends on the acceptance of ADR-0010
to ADR-0015, which are `proposed`.** If the architect changes a recommendation, this outline changes with it. Nothing
here is a decision; crate names, sizes and order are estimates for planning.

## Crates

All crates are `no_std` with host tests (`cargo test`) unless marked, follow the repository's rules (`zoraya-`
prefix, `// SAFETY:` comments, no external crates without an entry in `docs/dependencies.md`), and contain no code that
needs the machine.

| Crate | Contents | Depends on | Size estimate (lines, Rust, excluding tests) |
|---|---|---|---|
| `zoraya-blockdev` | The `BlockDevice` trait of ADR-0015 (geometry, read, write, flush, typed errors); `MemDevice` (a RAM device); `RecordingDevice` (logs every write, completion and flush, and produces the set of crash images ADR-0015 rule 2 allows, with a seeded choice of old/new/arbitrary content per unstable sector); `FaultDevice` (fails the n-th request) | none | 800–1,200 |
| `zoraya-fs-format` | On-disk format: root record, block pointer with checksum, metadata block header, B+tree node layout, attribute record (ADR-0014), directory entry and `Name` validation (ADR-0013), CRC32C (ADR-0012). Total decoder: `#![forbid(unsafe_code)]`, every decode returns `Result`, bounded allocation | none | 1,500–2,500 |
| `zoraya-fs-core` | The engine behind the trait: copy-on-write B+tree, allocator with deferred reuse and space reservation, commit groups and the commit protocol (ADR-0011), mount (root selection), the request API of ADR-0010 (create, mkdir, remove, rename with modes, set attributes, set size, write, read, readdir, link objects, unnamed objects and `publish`, `sync`), fail-stop error state, strictly increasing timestamps (ADR-0014 rule 3) | `zoraya-blockdev`, `zoraya-fs-format` | 6,000–10,000 (RedoxFS core about 6,500; littlefs about 7,700 lines of C; `survey.md`, Comparison) |
| `zoraya-fs-model` | Reference model of ADR-0010's contract: an in-memory namespace and file contents with a completion log; given a log prefix, the expected state; the "is this observed state a valid prefix?" check | none | 600–1,000 |
| `zoraya-fsck` (library + host binary) | Offline checker: every reachable block verifies, the namespace is a tree, no block is reachable twice, the allocator's free set matches, attributes are in range; optional repair later | `zoraya-fs-format`, `zoraya-fs-core` (read-only paths) | 1,000–2,000 |
| `zoraya-mkfs` (host binary, `std`) | Creates an image file; optionally populates it from a host directory (for the stage-4 "files survive reboot" test and for test images) | `zoraya-fs-core`, `zoraya-blockdev` (file-backed device) | 300–600 |
| filesystem service (stage 4, user space, not a host crate) | One process per volume: channel protocol to clients (needs the channel/message ADRs), virtio-blk driver binding, the volume object with its `FAILED` signal | `zoraya-fs-core` | depends on the IPC ADRs |
| `libposix` file layer (stage 6) | `stat` synthesis, `umask`, link-object following, `EILSEQ` mapping, `fsync`→`sync`, `O_TMPFILE`→unnamed + `publish` | IPC client | part of `libposix` |

Total for the host-testable part: about 10,000–17,000 lines plus tests, by the estimates above `[estimate]`.

## Test strategy

Each test family below is named after the ADR invariants it checks. All run under `cargo test` on the host, are
deterministic (fixed seeds, fixed iteration counts, like `zoraya-dtb`'s 30,000 mutations), and need no network.

1. **Model-based request tests** (INV-0010-1, -2, -5 and ADR-0010 rule 1's visibility; INV-0013-2, -4; INV-0014-2, -3). Random
   sequences of requests from several simulated clients, generated from a small grammar (create, write, rename-over,
   remove, mkdir, set attributes, `sync`, unnamed + `publish`, invalid names), run against `zoraya-fs-core` on a
   `MemDevice` and against `zoraya-fs-model`; after every request the observable states must match. Shrinking a
   failing sequence by hand-written minimisation (no external property-testing crate unless recorded).
2. **Crash injection** (INV-0010-1, -2, -4, -6; INV-0011-1 to -5; INV-0015-2). The same sequences on a
   `RecordingDevice`. For every commit: crash at every completion and flush boundary; between flushes, a bounded number
   of the allowed subsets of unstable writes (all subsets when there are at most `k` unstable writes
   `[assumption: k = 12]`, random subsets otherwise), with torn sectors chosen per sector. Every crash image is mounted,
   checked by `zoraya-fsck`, and compared with the model's prefixes. This is CrashMonkey's bounded black-box approach
   without kernel instrumentation (`survey.md`, Testing crash consistency).
3. **Ordering checks inside the recording device** (INV-0011-1, -2, -3). The device knows which blocks each root
   record makes reachable and fails the test on a write to a protected block, a reuse before the freeing commit is
   durable, or a root record written before its blocks are stable.
4. **Fault injection** (INV-0010-3; INV-0012-5; INV-0015-4). `FaultDevice` fails the n-th write, flush or read for
   every n over a request sequence; assert the fail-stop state and the replies.
5. **Image fuzzing** (INV-0012-1 to -4). From `mkfs` images that contain every structure kind: block-level garbage,
   field-level mutation of every field of every structure with XFS's eight transformations (clear, set, toggle high,
   middle and low bit, add, subtract, randomise; `survey.md`, Testing crash consistency), lost-write and
   misdirected-write substitutions; mount, traverse every file, run `fsck`. Assert no panic, bounded steps and
   allocation, and that every read returns either the original bytes or `Corrupt`.
6. **Unit tests** for `Name` (INV-0013-1), timestamps (INV-0014-4), CRC32C test vectors, the test device itself
   (INV-0015-1), and the attribute record shape (INV-0014-5).
7. **In-QEMU tests** (stage 4, `kernel/src/testing.rs` or a user-space test runner, whichever exists then): mount an
   image built by `zoraya-mkfs`, write, power off through PSCI without unmounting, reboot, check; run with
   `cache=writeback`, never `cache=unsafe`, for crash-related tests (ADR-0015 rule 5). Every QEMU command line goes
   through `tools/run-qemu.sh`.

## Order of work (estimate)

| Step | Work | Depends on | Rough effort |
|---|---|---|---|
| 1 | `zoraya-blockdev` with `MemDevice`, `RecordingDevice`, `FaultDevice`, and their own tests | ADR-0015 accepted | small |
| 2 | `zoraya-fs-model` | ADR-0010 accepted | small |
| 3 | `zoraya-fs-format`: names, attributes, CRC32C, node layouts, root record; decoder fuzz test | ADR-0012–0014 accepted | medium |
| 4 | `zoraya-fs-core`: B+tree, allocator, commit, mount; request API; model-based tests | steps 1–3, ADR-0011 accepted | large (most of the work) |
| 5 | Crash-injection harness on the full request API | step 4 | medium |
| 6 | `zoraya-fsck` and `zoraya-mkfs` | steps 3–4 | medium |
| 7 | Service process and virtio-blk driver (roadmap stage 4) | stages 2–3, channel/message ADRs | medium, outside this outline |
| 8 | `libposix` file layer (roadmap stage 6) | stage 6 | outside this outline |

Steps 1–6 need no kernel features and can proceed in parallel with stages 1–3 once the ADRs are accepted. Each step
ends with its ADR invariants' checks implemented and their rows in `docs/adr/invariants.md` set to
`checked: <command>` (ADR-0001 rule 6).

## Known limitations of this plan

- The IPC protocol between client and service is not designed (no channel/message ADRs yet); the request API in
  `zoraya-fs-core` is a Rust API that the service will wrap.
- Performance targets (commit cost, write amplification, read-path checksum cost) are unmeasured; the first
  measurements belong to step 7 on the prototype.
- The crash-state enumeration is bounded (`k` unstable writes); beyond the bound it samples.
