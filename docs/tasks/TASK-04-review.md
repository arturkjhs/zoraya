# TASK-04 independent review

- **Reviewed:** branch `kernel/stage-0`, commits `21993ff..01f40c2` (`git log main..kernel/stage-0`, 6 commits), HEAD `01f40c29c20b81d6203caa2ef4c1f7009f742ff1`.
- **Reviewer:** independent subagent that did not write the code (TASK-04 Process item 8).
- **Date:** 2026-10-25.
- **Environment:** Apple Silicon M4 Mac, macOS (Darwin 25.4.0), HVF available; Rust 1.99.1 (rustup, pinned), QEMU 11.1.2, dtc, GNU make 3.81. All checks ran in a fresh clone under the scratch directory. The real repository was not modified.

## make test from a fresh clone

```
git clone --branch kernel/stage-0 /Users/svitlana/workspace/os clone && cd clone && make test
```

**Exit status: 0** (wall time 10.9 s, fresh `target/`).

Summary lines, verbatim:

```
test result: ok. 9 passed; 0 failed; ...     (zoraya-adr-check unit tests)
test result: ok. 8 passed; 0 failed; ...     (zoraya-dtb fixtures.rs)
test result: ok. 13 passed; 0 failed; ...    (zoraya-dtb malformed.rs)
test result: ok. 12 passed; 0 failed; ...    (zoraya-test-runner unit tests)
adr-check: 6 ADRs checked against INV-0001-1..5
adr-check: warning: 34 invariants of accepted ADRs have no check yet (ADR-0001 rule 6):
adr-check: 0 errors, 34 warnings
[auto-default] PASS: 11 passed, 2 expected failures
[auto-smp4-2g] PASS: 11 passed, 2 expected failures
[auto-el2] PASS: 11 passed, 2 expected failures
[tcg-default] PASS: 11 passed, 2 expected failures
[tcg-el2] PASS: 11 passed, 2 expected failures
[tcg-smp4-gicv3] PASS: 11 passed, 2 expected failures
make test: all checks passed
```

fmt --check and all three clippy runs (host crates, kernel, test kernel) passed with `-D warnings` and printed nothing.

Also ran: `make build` (exit 0). `tools/run-qemu.sh --test build/zoraya-kernel.elf` under `auto` (HVF), `tcg` and HVF with `virtualization=on`: each printed the banner and memory map, then `stage 0 complete; powering off`, and QEMU exited with status 0. I did not run `make debug` interactively. Its command line (`--print-cmd`) is `-accel tcg ... -s -S`, which is correct.

## Findings

| ID | Area | Finding | Severity | Suggested fix |
|---|---|---|---|---|
| R-01 | Scope / docs/research | Commit `01f40c2` adds `docs/research/TASK-04-foundation-and-stage-0.md`, 180 lines, byte-identical to `docs/tasks/TASK-04-…`. So `git diff --stat main -- docs/research` is not empty. This breaks TASK-04 "Out of scope: changing … `docs/research/`" and the branch's own CLAUDE.md:41. The same commit adds `docs/tasks/TASK-06-filesystem-design.md` and `TASK-07-filesystem-implementation.md`. TASK-04 does not ask for them and the commit message does not mention them. They look like untracked files swept into the commit. | error | Remove `docs/research/TASK-04-…` from the branch (keep the `docs/tasks/` copy), unless the architect explicitly wants it there. Then either drop TASK-06/07 from the branch or say in the commit message or report that the architect supplied them. |
| R-02 | Docs vs QEMU behaviour | `kernel/linker.ld:3-4` and `docs/design.md:93-94` say QEMU "refuses to load an image that overlaps" the DTB. This is false on QEMU 11.1.2. I relinked with `KERNEL_BASE = 0x40080000` in a scratch copy. QEMU booted it without complaint and simply did not place a DTB: `arm_load_dtb` skips the blob when it does not fit below the image. The kernel printed `ZORAYA-PANIC at boot: no device tree blob in x0 or at 0x40000000` and halted. The fallback path works, but the documented reason for the 2 MiB offset is wrong. | imprecision | Reword: "if less than the blob's size is left below the image, QEMU silently omits the DTB; the kernel then reports the missing blob through the fallback UART." Optionally add a linker `ASSERT(KERNEL_BASE - 0x40000000 >= 2M, …)`. |
| R-03 | Stack / linker | `.stack` sits directly above `.bss` (`linker.ld:31-43`) and grows down into it. With the MMU off there is no guard, so an overflow silently corrupts `UART_BASE`, `PANICKING`, `PSCI_CONDUIT` and the test counters. It does not fault. I measured the high-water mark by scanning the loader-zeroed stack at shutdown: **26,688 B** (normal kernel) and **29,616 B** (test kernel) of 65,536, debug build, the same under HVF and TCG. That is about 45 % used, and `docs/design.md` "Known limitations" does not mention it. | imprecision | Add a canary at `__stack_bottom`, checked at shutdown, at test end and in the panic handler. List "no stack guard before the MMU; 64 KiB, ~30 KiB used" in Known limitations. Consider placing the stack below `.data`/`.bss`. |
| R-04 | Boot / EL2 docs | `docs/design.md:103` says "EL1 is AArch64 with no stage-2 translation or traps". That overstates it. `CPTR_EL2` deliberately keeps SVE/SME trapped (boot.rs:27-28 says so). `MDCR_EL2` is left at its reset value, which is architecturally UNKNOWN on hardware, and so are `HFG*_EL2`/`HCRX_EL2` on CPUs with FEAT_FGT/FEAT_HCX. I probed at EL2: `MDCR_EL2` = 0x6 (TCG) and 0x0 (HVF), and FGT and HCX are absent on both, so stage 0 hits no trap. Also, `SCTLR_EL1` is written only after `eret` (boot.rs:86-91), so EL1 runs a few instructions with its reset `SCTLR_EL1`. That is fine on QEMU (M=0) but not robust. | imprecision | Reword to "no traps that stage 0 can hit; SVE/SME trapped; MDCR_EL2 left at reset". In stage 1, set `MDCR_EL2` (HPMN, TPM/TDA/TDE=0) before any PMU or debug use, and write `SCTLR_EL1` from EL2 before `eret`, as Linux's `init_el2` does. |
| R-05 | Boot / EL2 | boot.rs:60-67 assumes `HCR_EL2.E2H` can be 0 (FEAT_E2H0). On a CPU where E2H is RES1, `CNTHCTL_EL2` bits 0/1 and `CPTR_EL2` use the E2H=1 layout. `0x33ff` would then give `FPEN=00`, FP would trap to an EL2 with no vectors, and boot would hang silently. I probed both: `HCR_EL2` reads back `0x80000000` and `ID_AA64MMFR4_EL1` = 0 under HVF (M4) and TCG, so this is correct today. | note | State the assumption in the boot.rs header. Optionally read `HCR_EL2` back after the write and park if E2H stayed 1. |
| R-06 | CLAUDE.md vs ADR-0001 | CLAUDE.md:11-13 adds a standing exception to ADR-0001 rule 2 ("An agent never edits the 'Architect's decision' section"): transcribing an already-recorded decision "when a task tells you to". TASK-04 Step 1 authorised exactly one transcription ("the only status change you may make"). Turning that into a general rule contradicts rule 2 as written. | imprecision | Remove the exception sentence. Or narrow it: "only when an architect's task explicitly instructs it (as TASK-04 Step 1 did); this is the architect's instruction, not an agent right." |
| R-07 | CLAUDE.md vs ADR-0001 | CLAUDE.md:17-18 says "If the task reserves none, use the next free number". That settles ADR-0001's open question on ADR numbering in parallel work ("To be settled when the second batch is commissioned"). It is also the collision case: TASK-06 runs in parallel and writes proposed ADRs. | imprecision | "If the task reserves no ADR number, ask the architect (ADR-0001 open question)." |
| R-08 | make test portability | `auto-el2` (Makefile test-qemu) needs HVF nested virtualisation, which means M3 or later and a recent macOS. On an M1 or M2, `make test` would fail instead of skipping. design.md:161-163 only says EL2 under HVF "may not work on older Apple chips". | note | Either gate `auto-el2` on a capability probe, or state in Known limitations that `make test` as written requires an M3 or later. |
| R-09 | Kernel arithmetic on DTB values | `main.rs:134` (`r.address + r.size - 1`), `main.rs:145` and `testing.rs:183` (`r.address + r.size`) overflow on a memory or reserved region that wraps past 2^64. `zoraya-dtb` accepts such a region because it filters only `size == 0`. In the debug kernel that is an overflow panic after console init, so it is reported, not UB. The parser itself is fine. | note | Use `checked_add`/`saturating_*` in the kernel, or have `platform::memory_regions` drop wrapping regions, with a host test. |
| R-10 | Test recovery from an exception | After `harness_reports_exception_as_failure`, `on_panic` resets the stack and runs the remaining tests without `eret`, still in the exception context: `ELR_EL1`/`SPSR_EL1` are stale and DAIF is masked, as it already was. This is harmless at stage 0, but the SAFETY comment at testing.rs:133-136 does not mention it. | note | Add one sentence to the SAFETY or module doc. When stage 1 makes the vectors real, recover by `eret` to a trampoline instead. |
| R-11 | ADR checker strength | Mutations that pass the checker: `superseded by ADR-0009` where ADR-0009 does not exist (only the index mismatch is reported); a placeholder such as `_(empty)_` as the Architect's decision; a second `Status:` line (the first one wins); `checked: <anything>` without checking that the command exists. Every INV-0001-1..5 mutation I tried was caught (see below). | note | Optional hardening: resolve `superseded by` targets, reject duplicate Status lines. |
| R-12 | Build metadata | `kernel/build.rs` recomputes `-dirty` only when the build script reruns, which happens on a HEAD or index change. Editing a tracked file without staging it rebuilds the kernel but keeps a stale commit label, and the reverse can happen too. | note | Add `rerun-if-changed` for `kernel/src` and `crates/`, or document that the dirty flag is approximate. |
| R-13 | Test coverage strength | `uart_base_matches_console` re-parses the same blob with the same code that chose the console, so it cannot detect choosing the wrong node. No `--expect-line` pins the console address. A hung test is reported only as `timeout after 60 s (no end line)`, without its name. | note | Add `--expect-line "console: pl011 at 0x9000000"` to one configuration. Optionally print a `ZORAYA-TEST run <name>` line, or report the last test started. |

## Checked and found correct

**Fresh clone and Make targets**
- `make test` passes from a fresh clone (exit 0).
- `make build` works, and the kernel boots, prints the banner and memory map, and powers off via PSCI under HVF, TCG and EL2 entry (both accelerators).
- `make debug` waits on :1234 under TCG and prints how to attach lldb and gdb-multiarch.
- `Cargo.lock` has no registry packages, so no network access is needed.

**unsafe and SAFETY**
- Every `unsafe` block has a `// SAFETY:` comment and every `unsafe fn` has a `# Safety` section; clippy `undocumented_unsafe_blocks = deny` enforces this workspace-wide.
- `dtb.rs`: reads are confined to `[RAM_BASE, kernel start)`, 8-aligned, with the length checked before `from_raw_parts`. The memory is never written, so the stated invariants hold.
- `pl011.rs`/`console.rs`: volatile, aligned 32-bit MMIO. Before `console::init` stores the base, a fault in `init` panics through `print_fallback`.
- `psci.rs`: clobbering x1–x17 meets SMCCC v1.0, where x4–x17 may be corrupted. v1.1+ preserves them, so the list is conservative. x18–x30 and SP are preserved by the callee. Leaving out `nomem`/`preserves_flags` is conservative. The SMC32 result is truncated to `i32`, which is correct.
- `reset_stack_and_call`:
  - It is sound in practice. Every abandoned frame (the test fn, `core::panicking`, the handler) has no destructors.
  - `x0` is an explicit operand, so `top` and `f` cannot be allocated to it.
  - `PANICKING` is cleared before the jump.
  - A panic while printing the fail line hits the nested-panic halt, and the runner then fails on timeout.
- No exclusive or LSE instructions appear in either kernel ELF (objdump count = 0). Atomics compile to plain `ldr`/`str`.
- The `aarch64-unknown-none` spec has `+strict-align` (checked with `--print target-spec-json`), so the "aligned accesses only, MMU off" claim holds.

**Boot path against the Arm ARM**
- CurrentEL is decoded from bits [3:2].
- `HCR_EL2 = RW` with E2H=0 is honoured on M4 HVF and TCG (R-05).
- `CNTHCTL_EL2` bits 0/1 = EL1PCTEN/EL1PCEN for E2H=0. CNTPCT and CNTP_CTL read fine at EL1 after the drop.
- `CPTR_EL2 = 0x33ff`: RES1 bits 13:12 and 9:0; TFP(10)=0, TTA(20)=0, TCPAC(31)=0; TZ(8) and TSM(12) set, so SVE/SME stay trapped.
- `HSTR_EL2 = 0`. `VPIDR`/`VMPIDR` come from MIDR/MPIDR.
- `ICC_SRE_EL2` is `S3_4_C12_C9_5`, and `0xf` = SRE|DFB|DIB|Enable, gated on `ID_AA64PFR0_EL1.GIC` [27:24].
- `SPSR_EL2 = 0x3c5` is EL1h with DAIF masked. `ELR`/`eret` are correct, and the ISB before eret is harmless.
- `SCTLR_EL1 = 0x30d00800`: the RES1 bits of v8.0 (11, 20, 22, 23, 28, 29) take the legacy-safe values for their later meanings (EOS, TSCXT, EIS, SPAN, nTLSMD, LSMAOE). M, C, I, A, SA and EE are 0.
- `CPACR_EL1.FPEN = 0b11`, and the ISB comes after the sysreg writes.
- `VBAR_EL1` = `0x40200800`, which is 2 KiB aligned. Each vector entry is 5 instructions, well within 32.
- The stack top is 16-aligned. `.bss` is 16-aligned at both ends and cleared with 16-byte `stp`.
- The MPIDR mask is Aff3 [39:32] plus Aff2..0 [23:0].
- With the MMU off, data accesses are Device-nGnRnE (HCR_EL2.DC=0).
- Secondary CPUs verified empirically: with `--smp 4` under HVF and TCG, a probe that makes any non-zero-affinity CPU write to the UART printed nothing, while the boot CPU's probe printed. So the secondaries never run.
- `x0` is `0x0` for an ELF and the DTB is at `0x40000000`. Every banner shows this.

**zoraya-dtb (crates/zoraya-dtb)**
- The crate is `#![forbid(unsafe_code)]`.
- Every multi-byte read goes through `be32`/`be64`, which use `checked_add` and `get`.
- Offset arithmetic stays bounded by the slice length, so there is no overflow in debug or release.
- The validation loop advances at least 4 bytes per token, and depth is capped at `MAX_DEPTH` = 32.
- No recursion: `Nodes` uses a fixed 32-slot stack, and `skip_subtree`'s depth cannot underflow.
- The root `props_off = root_off + 8` is correct for the validated empty name.
- Existing tests cover every truncation, wrong magic, versions, bad header offsets, an unterminated rsvmap, structure and string errors, maximum depth, and 30,000 mutations.
- I added 900,000 mutations in release (seeds 1–3, with the EL2 fixture added as a seed) and 200,000 in debug, all in my scratch copy. No panics.
- The fixtures can be regenerated: rerunning `tools/dump-dtb-fixtures.sh` on this host gave blobs identical to the committed ones apart from `rng-seed`/`kaslr-seed`.
- The README records the exact commands.

**Runner and harness**
- I drove `target/debug/zoraya-test-runner` with fake `sh -c` commands. It returned non-zero for:
  - a hang (timeout);
  - a missing end line;
  - a `ZORAYA-PANIC` outside a test;
  - an unexpected `fail`, including one that was a panic;
  - an end line with no power-off;
  - a non-zero QEMU exit status;
  - an expected panic that did not run;
  - fewer results than `begin` announced;
  - a grandchild holding stdout.
- A clean run returned 0.
- In-QEMU, the deliberate panic and the `udf` exception are both reported as `fail … panic`, and the next test runs.
- The spec's required test list is present: DT parsed; UART matches; memory non-empty and inside PA space; EL1; deliberate panic.

**ADR files and the checker**
- Step 1: `git diff main -- docs/adr/000*.md` shows only six `Status: proposed → accepted` lines and six copies of the exact Architect's decision text. `index.md` is unchanged.
- `invariants.md` has a Status column: `checked: make adr-check` for INV-0001-1..5, and `not yet applicable (stage N)` for the other 34.
- `tools/adr-check` caught each of these mutations:
  - a renamed heading, reordered headings, an extra heading;
  - an invalid status, and a bare `superseded`;
  - `accepted` with an empty or whitespace-only decision section;
  - an undefined DM ID, an ID missing from the Evidence line, a bad section slug, a nonexistent research file;
  - changed invariant text, a missing invariant row, an unknown invariant row, a garbage status, `checked` with no command;
  - a wrong index status, a missing index row, an index row for a nonexistent ADR;
  - a new ADR with a template status.

**Docs**
- `docs/design.md` states each ADR accurately and adds no decisions. I checked against the Recommendation and Invariant text of ADR-0002..0006: namespace table, snapshot gating, the spawn parameters, Port being non-duplicable, ExitRecord and dispositions, T1/T2 counts (248/291, 49/59), Level/Edge subscriptions, Deadline, no `Interrupted`.
- Its Boot, Tests and roadmap sections match the code and the Makefile, except R-02 and R-04.
- The Vision section is left as the TODO line.
- CLAUDE.md (85 lines) contains every rule TASK-04 Step 2 requires. On statuses, who sets them, rule 3 and rule 6 it agrees with ADR-0001, apart from R-06 and R-07.

**Layout and other files**
- The layout matches Step 4. The toolchain is pinned (1.99.1 stable, minimal profile, rustfmt and clippy, `aarch64-unknown-none`).
- `docs/dependencies.md` lists no crates.
- `tools/posix-surface/` is unchanged.
- The CI workflow is limited to 10 minutes and has never run, which design.md says.

## Resolution

The implementing agent applied these after the review. `make test` was rerun afterwards: it passes, with 12 passed and 2 expected failures in each of the six QEMU configurations.

| ID | Resolution |
|---|---|
| R-01 | Fixed. The three files (`docs/research/TASK-04-…`, `docs/tasks/TASK-06-…`, `docs/tasks/TASK-07-…`) were not added by the agent. They had been staged in the working tree by someone else while the task ran, and `git commit` swept them in. They were removed from the unpushed commit with `git rm --cached` and `--amend`, and are left untracked in the working tree for the architect. `git diff --stat main -- docs/research` is now empty. |
| R-02 | Fixed. `linker.ld` and `design.md` now say QEMU silently omits the DTB when it does not fit below the image. `linker.ld` asserts the 2 MiB gap. |
| R-03 | Fixed. A canary at `__stack_bottom` is written at boot. It is checked at shutdown (warning) and by a new last in-QEMU test, `stack_canary_intact`. "No stack guard" and the measured high-water marks are listed under Known limitations. |
| R-04 | Fixed. `SCTLR_EL1` is now written at EL2 before `eret`. `design.md` and the `boot.rs` header say that SVE/SME stay trapped, and that `MDCR_EL2` and the fine-grained trap registers are left at reset, for stage 1 to set. |
| R-05 | Fixed: the E2H=0 assumption, and how it was checked, are stated in the `boot.rs` header. |
| R-06 | Fixed. CLAUDE.md now cites ADR-0001 rule 2 and limits transcription to an explicit architect's instruction for that task, not a standing permission. |
| R-07 | Fixed. CLAUDE.md: "If the task reserves none, ask the architect", citing the ADR-0001 open question. |
| R-08 | Documented under Known limitations: `make test` as written needs HVF nested virtualisation (M3 or later). The configuration was kept because the architect's Mac is an M4. |
| R-09 | Fixed. `platform::memory_regions` drops ranges that wrap past 2^64 (new host test `wrapping_memory_region_is_dropped`). The banner and the test helper use saturating/checked arithmetic. |
| R-10 | Fixed: the SAFETY comment in `testing.rs` and Known limitations both say so. |
| R-11 | Partly fixed. The checker now rejects duplicate Status lines and `superseded by` targets that do not exist, with unit tests for both. Placeholder decision text and unverified `checked:` commands are still accepted. |
| R-12 | Fixed in part. `build.rs` reruns on changes under `kernel/src`, `crates/` and `kernel/Cargo.toml`. Edits elsewhere can still leave the flag stale, as the comment says. |
| R-13 | Fixed. `auto-default` pins `console: pl011 at 0x9000000 (/pl011@9000000)`. The protocol gained a `ZORAYA-TEST run <name>` line before each test, and the runner names the hung test on timeout and rejects a result that has no matching `run` line (unit tests added). |
