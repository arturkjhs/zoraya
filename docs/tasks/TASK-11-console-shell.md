# TASK-11: Native console shell on stage 3

## Context
Read `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, and in full ADR-0003, ADR-0004, ADR-0006, ADR-0032, ADR-0034, ADR-0040 to ADR-0046. Stage 3 is on `main`.
Goal: a person boots Zoraya with `make run` and works in a small native shell over the serial console. It is not POSIX and not dash. It demonstrates the native architecture: spawn with explicit handles, waiting through a port, exit records, job-wide interruption. File commands come with the filesystem service in stage 4 and are out of scope here.

## Step 1: proposed ADRs (numbers 0060 to 0069 reserved)
Template-exact, `proposed`, at least three options each, a Linux baseline and a non-Unix precedent. Short.
1. **Console input.** Input reaches user space as raw bytes. No line discipline, echo, line editing or signal generation in the kernel (the TTY layer entries in `docs/research/design-mistakes.md` say why); the shell does all of that. UART receive interrupts fill a fixed-size kernel buffer; overflow drops bytes and counts them, never allocates. The console object gets a `READ` right, a `console_read(handle, buffer)` call that never blocks, and a readable signal for ports (ADR-0006). State what happens when two processes hold the console.
2. **Listing the ramdisk.** A call to enumerate a directory handle's entries (needs `LOOKUP`), in a form the stage 4 filesystem service can also offer later.
3. **Power off.** How a process may turn the machine off. Not ambient: a handle with a right, given only to the first process.
4. **What the kernel starts.** Until ADR-0053 is accepted, the kernel starts the shell directly as the first process with the ADR-0043 startup handles. Write it as a temporary decision that ADR-0053 will replace.
Commit the ADRs before the code that depends on them.

## Step 2: the shell (`userspace/zosh` or a name you justify, not `zsh`)
A `no_std` Rust program on `zoraya-rt`:
- Line editing: printable input, backspace, Enter, Ctrl-U (kill line), Up/Down history of the last 32 lines. The shell echoes.
- Commands: `help`; `ls` (ramdisk entries); `run <program> [args...]` and also a bare program name; `<command> &` runs in the background; `jobs`; `wait [%n]`; `kill [-int|-term|-hup|-quit] %n` (request signals through `job_signal`); `echo`; `uptime` (monotonic clock); `exit-code` (the last `ExitRecord`); `poweroff`.
- Every program runs in its own sub-job with only the handles the shell lists (console, and nothing else unless a command needs more).
- The shell waits for the console and for its children through one port (ADR-0006).
- Ctrl-C at the prompt clears the line. Ctrl-C while a foreground job runs sends `Interrupt` to that whole job with `job_signal`. Ctrl-\\ sends `Quit`.
- After each program the shell prints its `ExitRecord` in a short form when it is not a clean exit 0, for example `[exited 3]`, `[terminated: Interrupt]`, `[fault: data abort at 0x...]`, `[killed]`.
- A program that faults, floods, or never ends cannot take the shell down.

Demonstration programs in the ramdisk: `hello`, `echo-args`, `sleep <seconds>`, `spin` (loops until interrupted), `exit <n>`, `fault` (touches an unmapped address), `fanout <n>` (spawns n children that spin; one Ctrl-C must end all of them, ADR-0004 job-wide delivery).

## Step 3: tests
The test runner gains scripted serial input: it sends keystrokes to QEMU and checks the output. In-QEMU tests at least for: a command and its output; a bare program name with arguments; the `ExitRecord` lines for exit, interrupt and fault; Ctrl-C ending all `fanout` children (none left: check with `jobs` and with the job's terminated signal); a background job with `jobs`/`wait`/`kill`; history recall; a burst of input larger than the kernel buffer (bytes dropped and counted, no crash); the shell surviving every demonstration program. Every invariant of the new ADRs gets its check. `make test` passes from a clean clone in all configurations; CI stays green.

## Documentation
`docs/design.md`: "as built" sections and a short "Using the shell" section. In the roadmap table add a row "3a — native console shell" between stages 3 and 4: the architect instructs you to do this. `invariants.md`, `index.md`.

## Process and Git
Independent review by a subagent that did not write the code, with deliberate mutations to confirm the tests catch broken behaviour; findings and dispositions in `docs/tasks/TASK-11-review.md`. Branch `shell/console` from the latest `main`. Configured git identity only; commit after each step; never merge, never push.

## Out of scope
Files and the filesystem service, POSIX, a terminal service, pipes between programs, SMP.
