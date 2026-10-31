# ADR-0022: Kernel heap

- **Status:** proposed
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0020, ADR-0021
- **Evidence:** CP-14, DM-030, alternatives-postmortem.md#hubris, alternatives-postmortem.md#redox

## Context
`Box`, `Vec`, `BTreeMap` and `String` need a `GlobalAlloc`. A kernel is long-lived, so what matters is fragmentation over time, leaks (one kernel leak badly slowed a Redox build workload, alternatives-postmortem.md#redox), and what happens on failure. `GlobalAlloc::dealloc` receives the `Layout`, so blocks need no header.

## Decision drivers
1. Failure must be something a caller can handle wherever a user action could cause it (CP-14, DM-030).
2. Every returned block must satisfy its alignment, and live blocks must never overlap.
3. Fragmentation must be bounded and stated, not discovered later.
4. Host-testable independently of `GlobalAlloc` plumbing and of the frame allocator (`CLAUDE.md`).
5. No dependency on physically contiguous memory for large allocations, since ADR-0021's contiguous path is the slow one.

## Considered options

### Option A: Slab on top of the page allocator (Linux baseline)
- **Description:** Per-type or per-size caches of objects in slabs, with per-CPU layers, over the buddy allocator.
- **Evidence for:** Best known behaviour under real kernel load; constructor caching [outside evidence: Bonwick, USENIX 1994; Linux `mm/slub.c`].
- **Evidence against:** Most of the machinery serves SMP scaling and type stability that stage 1 cannot use or measure.
- **Cost:** High (estimate: 1,000 lines or more for the useful subset).
- **Precedent:** Linux, Solaris.

### Option B: One first-fit free list
- **Description:** A single address-ordered list of free blocks with coalescing.
- **Evidence for:** Smallest. Coalescing limits external fragmentation.
- **Evidence against:** O(free blocks) for every operation, including the many small allocations a kernel makes; small and large blocks fragment each other.
- **Cost:** Lowest (estimate: 150 lines).
- **Precedent:** The `linked_list_allocator` crate used by many hobby kernels [outside evidence: crates.io].

### Option C: Size classes over a page-span allocator
- **Description:** Requests up to 2 KiB go to power-of-two size classes with intrusive free lists, carved from 64 KiB chunks. Larger requests take whole pages from an address-ordered, coalescing span list. The span list grows from a contiguous virtual window that is backed page by page from the frame allocator.
- **Evidence for:** O(1) small paths. Small blocks cannot fragment large ones. Internal fragmentation is bounded by the class sizes (under 50 %). Natural alignment follows from power-of-two classes. Large allocations need only virtual contiguity (driver 5).
- **Evidence against:** Memory given to a class stays in that class, so peak usage per class is never returned. A span list is O(free spans) for large requests.
- **Cost:** Moderate (estimate: 300 lines).
- **Precedent:** The structure of most general-purpose allocators, simplified [outside evidence: Bonwick and Adams, USENIX 2001].

### Option D: No kernel heap (Hubris)
- **Description:** All kernel objects static or allocated at boot; Hubris checks peak memory at build time (alternatives-postmortem.md#hubris).
- **Evidence for:** No allocation failure at run time at all.
- **Evidence against:** Contradicts the stage's definition of done, and cannot serve a dynamic process model.
- **Cost:** Not applicable to the roadmap.
- **Precedent:** Hubris, in a fixed task set.

## Recommendation
Recommended: **Option C**, in a `no_std` crate `zoraya-heap`. It is a recommendation, not a decision.

- **Backing.** The crate asks a `Backing` trait for more pages, and coalesces ranges that happen to be adjacent. The kernel implements it by mapping frames at the top of the heap window (ADR-0020). Growth is at least 1 MiB at a time. Stage 1 never shrinks the heap.
- **Alignment.** Size is rounded up to `max(size, align)`, then to a class, so a block is aligned to its class. Alignment above 4 KiB comes from a span.
- **Fragmentation.** Each small class holds at most its own peak live size plus one chunk. Span fragmentation is limited by coalescing. A host test enforces a bound on one stated workload: sizes from 16 bytes to 256 KiB, up to 8 MiB live, memory taken from the backing within 3 times the peak of live bytes plus 16 chunks.
- **Failure policy.** Both kinds of API, with a rule for where each is allowed:
  - `GlobalAlloc` returns null when the backing is exhausted. An infallible `Box::new` or `Vec::push` then panics through Rust's allocation error path. This is allowed only in boot code and at fixed, bounded call sites whose size no user action can influence.
  - Everything reachable from a system call, from stage 2 on, uses fallible paths (`Vec::try_reserve` and `memory::try_box`) and returns the failure to the caller, as CP-14 expects.

## Consequences
- **Positive.** Small allocations are constant time. Large ones do not need contiguous frames. The rule for infallible allocation can be checked in review.
- **Negative.** No double-free detection, since `GlobalAlloc` has no header to check. Memory taken by a size class is not returned. The virtual-window design costs a page-table update per growth.
- **Follow-up.** A lint or review rule for infallible allocation on syscall paths in stage 2. Per-CPU caches at SMP.

## Invariants
- **INV-0022-1.** Every block returned satisfies its layout's alignment and overlaps no live block. *Check:* host property tests in `zoraya-heap` against a model.
- **INV-0022-2.** When the backing is exhausted the allocator returns failure and stays usable. *Check:* host test in `zoraya-heap`, and an in-QEMU test of a fallible request that cannot be met.

## Open questions
- Whether a debug mode with redzones and poison is worth having before stage 2. It would have to be built on the `Layout`, without headers.
- Whether to return empty chunks to the span list.

## Revisit triggers
- A memory leak or fragmentation seen in a long test (alternatives-postmortem.md#redox).
- Per-class memory that stays high after its peak in a measured workload.
- SMP, when a global lock around the heap becomes contended.

## Architect's decision
