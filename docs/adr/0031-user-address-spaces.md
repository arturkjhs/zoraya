# ADR-0031: User address spaces and access to user memory

- **Status:** proposed
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0020, ADR-0024
- **Evidence:** CP-15, CP-16

## Context
ADR-0020 leaves ASIDs and the use of TTBR0 to this stage. CP-15 makes hardware address spaces the isolation boundary. ADR-0024 calls for a new ADR on faults during copies from user memory, and its open question asks how the stack limit in `SP_EL0` survives EL0.

## Decision drivers
1. A process reaches only its own memory, and the kernel never trusts a user pointer (CP-15).
2. No mapping is writable and executable (CP-16).
3. A bad user pointer is an error for that system call, never a kernel fault.
4. A context switch should not have to flush the whole TLB.
5. Kernel-only threads must keep TTBR0 disabled, as INV-0020-2 says.

## Considered options

### Option A: Ordinary loads after a range check, with PAN
- **Description:** Check the range, then use ordinary loads with fix-ups, relying on PAN to catch stray accesses.
- **Evidence for:** Fast, and any size of access works.
- **Evidence against:** Ordinary loads run with EL1 permission, so a range check is the only barrier unless the CPU has PAN, which is optional and which `cortex-a72` under TCG lacks [outside evidence: Arm ARM DDI 0487, FEAT_PAN].
- **Cost:** Moderate.
- **Precedent:** Older Linux arm64 kernels [outside evidence: Linux arm64 `uaccess` history].

### Option B: Unprivileged load and store instructions with a fix-up (Linux baseline)
- **Description:** One small assembly routine copies with `ldtrb` and `sttrb`, which check permissions as EL0 would. A fault with the PC inside the routine resumes at its error exit.
- **Evidence for:** The hardware enforces user permissions, so a kernel pointer passed by a user faults even if a range check were wrong. Needs no optional feature.
- **Evidence against:** A byte at a time, so slow for large copies. Word-sized forms can come later.
- **Cost:** Low (estimate: 40 lines of assembly).
- **Precedent:** Linux on arm64 today: `arch/arm64/include/asm/uaccess.h` and `lib/copy_from_user.S` use `ldtr` forms, with an exception table [outside evidence: Linux sources].

### Option C: Map user pages into the kernel for each call (microkernel style)
- **Description:** seL4 avoids general user access by passing messages through registers and a mapped IPC buffer [outside evidence: seL4 reference manual]. Zircon uses checked user-pointer types around its copies [outside evidence: Zircon `user_ptr` sources].
- **Evidence for:** A narrow, typed path into the kernel.
- **Evidence against:** An IPC buffer design decides stage 3's message format ahead of time. The direct map could reach the frames, but walking tables for every copy is slower and easy to get wrong.
- **Cost:** Moderate.
- **Precedent:** seL4.

## Recommendation
Recommended: **Option B**, with a typed `UserSlice` as the only way kernel code reaches user memory. It is a recommendation, not a decision.

- **Layout.** User addresses are `[0x0040_0000, 2^47)`. Page 0 up to 4 MiB stays unmapped. The ELF decides text and data. The stack sits at the top, 64 KiB and mapped in full at creation, with an unmapped page below it.
- **Tables.** One `zoraya-pagetable` tree per process in `Half::Lower`, from frames, with no sharing between processes. User pages are `nG`, so their TLB entries carry the ASID.
- **ASIDs.** 8-bit, one per live process, in the top of `TTBR0_EL1`. Freed on exit with `tlbi aside1is`. 255 live processes at most at this stage: creation then fails with an error `[assumption: enough until stage 3 measures otherwise]`.
- **Kernel threads.** `EPD0 = 1` and `TTBR0 = 0` while no process is current, so INV-0020-2 holds. `EPD0` is cleared only when switching to a thread with a process.
- **Stack limit.** Each thread stack has a fixed size, and SP_EL1 is at the top of it on entry from EL0, so the vector computes the limit from SP. It saves the user's `SP_EL0` in the frame, loads the limit, and restores `SP_EL0` on return to EL0. No per-CPU pointer is needed, which also settles ADR-0024's open question. SMP will still need one emergency stack per CPU.
- **User access.** `UserSlice::new` checks the range lies in the user half. Then `read` uses the fix-up routine. No call writes to user memory yet, so the `sttrb` form comes with the first that does. A fault there, and nowhere else, is resumed: this is the one exception ADR-0024 planned. The normal kernel's fix-up check compares the PC with the routine's bounds.
- **Faults.** Every page is mapped at creation, so a fault from a user access is a bad pointer; there is no demand paging at this stage.

## Consequences
- **Positive.** The kernel cannot be tricked into touching kernel memory with user permissions. No optional CPU feature is needed. Switches need no full TLB flush.
- **Negative.** Byte copies are slow, which matters later for large I/O. A limit of 255 live processes.
- **Follow-up.** ASID generations before anything needs more; word-sized copies; demand paging and copy-on-write for `snapshot_process`.

## Invariants
- **INV-0031-1.** No user mapping is writable and executable, and all user mappings have no EL1 execute permission. *Check:* `zoraya-pagetable` host tests, and an in-QEMU test walking a live process's tables.
- **INV-0031-2.** A process cannot read or write kernel memory or another process's memory. *Check:* in-QEMU tests with user programs that try, each ending in a `Fault` exit record.
- **INV-0031-3.** A bad pointer passed to a system call returns `BadAddress` and leaves the kernel running. *Check:* in-QEMU test with kernel, unmapped and wrapping addresses.
- **INV-0031-4.** Kernel code reaches user memory only through `UserSlice`. *Check:* `make check-user-access` greps `kernel/src` for `ldtr` and `sttr` outside `arch/aarch64/usercopy.rs`. A plain cast of a user address is left to review.
- **INV-0031-5.** With no current process, TTBR0 translation is disabled. *Check:* in-QEMU test after a process has run and exited.

## Open questions
- When the byte copy's cost becomes visible; stage 4 and its file I/O should measure it.
- ASID reuse strategy for more than 255 processes.

## Revisit triggers
- A workload that needs more than 255 live processes.
- A measured copy cost.

## Architect's decision
