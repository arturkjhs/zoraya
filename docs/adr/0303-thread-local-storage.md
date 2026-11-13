# ADR-0303: Thread-local storage: musl's variant, the thread pointer set at birth, and who pays

- **Status:** accepted
- **Date:** 2026-11-12
- **Depends on:** ADR-0100, ADR-0102, ADR-0261, ADR-0300, ADR-0302
- **Evidence:** CP-03, CP-14, DM-004

## Context

ADR-0100 item 5 keeps musl's `__init_tls` (it walks `AT_PHDR` for `PT_TLS`, uses a built-in block when the TLS fits and
`mmap` otherwise) because `errno`, the `FILE` locks and the locale are thread-local in musl, and `__set_thread_area` is
`msr tpidr_el0`, which needs no system call. That is enough for one thread. With several, each thread needs its own block,
its own thread pointer, and a place to be paid for.

What the code gives: `TPIDR_EL0` is saved and restored at every switch (`context.rs`), the kernel never changes it
(`arch::tpidr_el0`), and `snapshot_process` already starts the child with the caller's value (`new_user_with_tls`). On
AArch64 musl uses variant 1 (`TLS_ABOVE_TP`): the thread pointer points at the end of `struct pthread`, the TLS image
follows it, and `pthread_create` places the block and `struct pthread` at the top of the thread's stack mapping
(`__copy_tls(tsd - libc.tls_size)`). `ADR-0261` point 8 charges a memory object to its creator for its whole life.

DM-004 is the reminder that per-thread state is easy to leave per-process: `errno`, the signal mask and the locale pointer
must be per thread, and credentials must not be (Zoraya has none, CP-03).

## Decision drivers

1. No new system call if the hardware does it: `TPIDR_EL0` is writable from EL0.
2. The first value of a new thread's thread pointer must exist before its first instruction: musl's `start` reads `errno`.
3. The cost of a thread must be on a charged object (CP-14), and a refusal must be `EAGAIN` from `pthread_create`.
4. Programs are static (ADR-0109): no dynamic TLS, no `dlopen`, so no `__tls_get_addr` beyond musl's static case.

## Considered options

### Option A: The thread pointer is an argument of `thread_create`; musl lays the block out (recommended)

- **Description:** As recommended below. The kernel stores `tls` into the new thread's saved `TPIDR_EL0`; it never reads it.
- **Evidence for:** Drivers 1 to 4; it is what `clone(CLONE_SETTLS)` does, and the kernel code to start a thread with a
  given value exists for the snapshot child.
- **Evidence against:** A program can set `TPIDR_EL0` to anything, including a pointer into another thread's block; that
  harms only itself, as with any wild pointer.
- **Cost:** *(estimate)* 10 lines in the kernel.
- **Precedent:** Linux (`CLONE_SETTLS`), musl.

### Option B: A `thread_set_tls` call after creation (Linux's `set_thread_area`, Windows' `TlsSetValue` shape)

- **Description:** The thread sets its own pointer with a system call as its first act.
- **Evidence for:** The kernel can validate the address.
- **Evidence against:** `TPIDR_EL0` is writable anyway, so validation prevents nothing; a thread has a window in which
  `errno` is not usable; one more call in the ABI.
- **Cost:** *(estimate)* 30 lines.
- **Precedent:** `set_thread_area` on x86 Linux, which exists because the instruction is privileged there.

### Option C: Kernel-managed per-thread storage (a block the kernel allocates from `PT_TLS`; NT's thread environment block is kernel-made `[outside evidence: Windows internals]`)

- **Description:** The kernel reads the program's `PT_TLS` at `spawn`, makes a block per thread, and sets the pointer.
- **Evidence for:** The library cannot get the layout wrong.
- **Evidence against:** The kernel would know the C ABI's TLS variant, and it loads ELF only to map segments
  (ADR-0033).
- **Cost:** *(estimate)* 250 lines and an ABI commitment.
- **Precedent:** Windows NT.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **Birth.** `thread_create`'s `tls` becomes the new thread's `TPIDR_EL0` (ADR-0300 item 2). The first thread of a process
   starts with 0, as now; `__init_tls` sets it with `msr` (musl's `__set_thread_area`).
2. **Layout.** musl's, unchanged: `pthread_create` maps one memory object for guard, stack, TLS image and `struct pthread`,
   and passes `TP(new)` as `tls`. The size of the TLS image is bounded by the program's `PT_TLS` (an ELF image of the
   program, at most the image's size). A `PT_TLS` of more than 64 KiB makes `pthread_create` fail with `EAGAIN` (the
   mapping would exceed the default stack size five times over); checked by `libposix` at start-up and reported in
   the program's first diagnostic.
3. **Who pays.** The mapping is a memory object made by the creating thread, so the creator's job pays for its frames
   and page tables for its whole life (ADR-0261 points 5 and 8), and the thread's kernel stack and record are paid at
   `thread_create` (ADR-0300 item 2). A thread's TLS is therefore charged with the stack, not separately. A refusal at
   either place is `EAGAIN` from `pthread_create` (musl's `goto fail` for a failed `mmap`; ADR-0307 for the thread figure).
4. **Freeing.** A joined thread's mapping is unmapped by the joiner (musl's `pthread_join`), a detached thread's by itself
   through `thread_exit`'s `unmap_base` (ADR-0300 item 3); both refund the charge.
5. **`snapshot_process`** copies the caller's `TPIDR_EL0` and the whole of every thread's TLS (they are ordinary mappings);
   ADR-0306 says what `fork` does with the others'.
6. **No dynamic TLS.** `__tls_get_addr` (musl's `src/thread/__tls_get_addr.c`) is retained but, in a static program, finds no
   dynamic module; `dlopen` stays excluded (INV-0005-2).

## Consequences

- **Positive.** No kernel TLS code beyond ten lines; the TLS of every thread is charged without a new rule.
- **Negative.** A 128 KiB default stack plus TLS per thread is committed in full when made (no demand paging, ADR-0031).
- **Follow-up.** `thread_create` takes `tls`; a test that a thread's `errno` is its own.

## Invariants

- **INV-0303-1.** A new thread's `TPIDR_EL0` is the `tls` argument of its `thread_create` at its first instruction. *Check:* an
  in-QEMU test whose thread entry stores `mrs tpidr_el0` and compares.
- **INV-0303-2.** `errno` and the locale pointer are per thread: two threads that set `errno` to different values and yield read
  back their own, 10,000 rounds. *Check:* an in-QEMU `libposix` test.
- **INV-0303-3.** A job at its memory limit gets `EAGAIN` from `pthread_create` and its usage is unchanged. *Check:* an in-QEMU test
  using `job_get_usage` before and after.
- **INV-0303-4.** The frames of a joined thread's stack and TLS are refunded: usage after 1000 create and join rounds equals usage
  before. *Check:* an in-QEMU test.

## Open questions

- Whether the 64 KiB `PT_TLS` bound is right: OpenSSL and CPython have small TLS; measure when 9c links.

## Revisit triggers

- Dynamic linking, or a program with a large `PT_TLS`.

## Architect's decision

Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.
