# Zoraya build and test entry points. See docs/design.md, "Where things are".
#
#   make build      build the kernel (build/zoraya-kernel.elf)
#   make run        boot it interactively (Ctrl-A X quits QEMU)
#   make run-gui    the same with a GPU, keyboard and tablet, in a window
#   make test-unit  host tests (cargo test) for crates/ and tools/
#   make test-qemu  in-QEMU tests under several machine configurations
#   make test-boot  boot the normal kernel, run the shell script under each
#                   machine configuration
#   make test-files files across two boots, restarts, a failed write
#   make test-libposix C programs through libposix against fsd (stage 6c);
#                   needs make fetch-ports once, as make ports does
#   make test-crash kill QEMU at random points (CRASH_ROUNDS=200 for more)
#   make test-gui   the GUI criterion: boot headless with --gui, type into the
#                   window over QMP and read the screen back (HVF and TCG)
#   make test       all of the above plus fmt, clippy and the ADR checks:
#                   a summary, with each step's output in build/logs/
#                   (V=1 prints everything, KEEP_GOING=1 runs on after a failure)
#   make fetch-ports  download the ports' pinned sources (the only step with
#                   network; ADR-0109)
#   make ports      build build/sysroot and build/rootfs-posix from them and
#                   run the host checks (make check-ports alone)
#   make mutants    the mutation lists of tools/mutants (not part of make
#                   test); survivors in build/mutants/report.md
#   make mutants-host CRATE=name
#                   cargo-mutants on one host crate (installed separately)
#   make debug      boot under TCG and wait for a debugger on :1234
#
# rustup installed with --no-modify-path keeps cargo in ~/.cargo/bin, which
# may not be on PATH. cargo itself puts the toolchain's bin directory first
# for the processes it starts.
CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
TARGET := aarch64-unknown-none
BUILD := build
KERNEL := $(BUILD)/zoraya-kernel.elf
TEST_KERNEL := $(BUILD)/zoraya-kernel-test.elf
RAMDISK := $(BUILD)/ramdisk.bin
DISK := $(BUILD)/disk.img
TEST_DISK := $(BUILD)/test-disk.img
USER_DIR := target/user/$(TARGET)/user
USER_PROGRAMS := hello spin isolate badptr fuzz regs f_kread f_kwrite f_null f_priv \
	f_undef f_exdata f_stack f_brk f_wfi f_align regsave lone koverflow copyfar \
	parent child counter portclose t_handles t_spawn t_signals t_ports portbench t_stress t_leak \
	busy zosh echo-args sleep exit fault fanout flood ticker handles stubborn t_console t_memory t_channels init t_devices t_loop chanbench blkd fsd \
	crashwriter crashcheck fsbench t_storage gpud inputd compd term t_gui guibench \
	t_streams t_snapshot t_image t_suspend posixrun broker ttyd
RUNNER := target/debug/zoraya-test-runner
RUN_QEMU := tools/run-qemu.sh
QEMU_TIMEOUT ?= 180

KERNEL_ARGS := -p zoraya-kernel --target $(TARGET)

.PHONY: all build run run-gui debug test test-unit test-qemu test-qemu-one test-boot test-files test-libposix test-posix test-posix-gui posix-image test-crash test-gui mutants mutants-host tools-storage ramdisk fmt-check check-user-access check-kernel-tty check-host-paths clippy adr-check clean \
	$(KERNEL) $(TEST_KERNEL) $(RUNNER) $(RAMDISK)

all: build

build: $(KERNEL)

# User programs go into the ramdisk, and the ramdisk into the kernel image
# (ADR-0033). They have their own target directory and a stripped profile.
$(RAMDISK):
	$(CARGO) build -p zoraya-user --target $(TARGET) --profile user --target-dir target/user
	$(CARGO) build -p zoraya-mkramdisk
	@mkdir -p $(BUILD)
	target/debug/zoraya-mkramdisk $@ $(addprefix $(USER_DIR)/,$(USER_PROGRAMS))

ramdisk: $(RAMDISK)

# Cargo decides what to rebuild; the copies give the ELFs stable names.
$(KERNEL): $(RAMDISK)
	ZORAYA_RAMDISK=$(abspath $(RAMDISK)) $(CARGO) build $(KERNEL_ARGS)
	@mkdir -p $(BUILD)
	cp target/$(TARGET)/debug/zoraya-kernel $@

# The test kernel has its own target directory so the two builds do not
# overwrite each other's output.
$(TEST_KERNEL): $(RAMDISK)
	ZORAYA_RAMDISK=$(abspath $(RAMDISK)) $(CARGO) build $(KERNEL_ARGS) --features qemu-test --target-dir target/qemu-test
	@mkdir -p $(BUILD)
	cp target/qemu-test/$(TARGET)/debug/zoraya-kernel $@

$(RUNNER):
	$(CARGO) build -p zoraya-test-runner

# The default disk: made once from rootfs/, and kept, so that files stay.
$(DISK): | $(KERNEL)
	$(CARGO) build -q -p zoraya-mkfs
	target/debug/zoraya-mkfs --size 64M --from rootfs $@

run: $(KERNEL) $(DISK)
	$(RUN_QEMU) --disk $(DISK) $(KERNEL)

# The GUI (ADR-0097): a window on the host; the serial line stays here.
run-gui: $(KERNEL) $(DISK)
	$(RUN_QEMU) --gui --mem 256M --disk $(DISK) $(KERNEL)

debug: $(KERNEL)
	@echo "Waiting for a debugger on localhost:1234. Attach with:"
	@echo "  lldb $(KERNEL) -o 'gdb-remote localhost:1234'   or"
	@echo "  gdb-multiarch $(KERNEL) -ex 'target remote localhost:1234'"
	$(RUN_QEMU) --debug $(KERNEL)

test-unit:
	$(CARGO) test --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt

# One runner invocation per machine configuration. "auto" is HVF on an
# Apple Silicon Mac and TCG elsewhere; the tcg lines always use TCG, so on
# a Mac both accelerators are exercised.
EXPECT := --expect-panic harness_reports_panic_as_failure \
	--expect-panic harness_reports_exception_as_failure \
	--expect-panic fault_report_for_brk \
	--expect-panic fault_report_for_unmapped_access \
	--expect-panic fault_report_for_stack_overflow \
	--expect-panic fault_report_for_thread_stack_overflow \
	--expect-panic fault_report_for_stack_overflow_from_el0 \
	--expect-panic fault_report_for_unexpected_interrupt \
	--expect-panic fault_report_for_el0_exception \
	--expect-line "ZORAYA-FAULT synchronous from EL1" \
	--expect-line "EC 0x3c: BRK instruction" \
	--expect-line "SPSR_EL1 0x" --expect-line "x30 0x" --expect-line "  sp  0x" \
	--expect-line "no syndrome: an interrupt does not set ESR_EL1" \
	--expect-line "EC=0x3c (BRK instruction) ESR_EL1=0xf2000321" \
	--expect-line "EC 0x25: data abort, same EL" \
	--expect-line "FAR_EL1 0xffffffff7fffe000" \
	--expect-line "ZORAYA-FAULT kernel stack overflow from EL1" \
	--expect-line "ZORAYA-FAULT interrupt 40 has no handler" \
	--expect-line "ZORAYA-FAULT synchronous from EL0 (AArch64)" \
	--expect-line "hello from EL0" --expect-line "process f_kread: fault, EC 0x24" \
	--expect-line "timer: virtual" --expect-line ", interrupt 27" \
	--expect-line "uart interrupt 33" \
	--expect-line "first process: hello" \
	--expect-line "first process ended: ExitRecord { code: 7, reason: Exited"
RUN_TEST = $(RUNNER) --timeout $(QEMU_TIMEOUT) $(EXPECT)

# Put in front of an "auto" configuration with virtualization=on: if the
# accelerator cannot start a machine at EL2 (HVF before the M3), print
# "[LABEL] skipped: <reason>" and end the recipe line with success, so the
# rest of the line does not run. tools/test-summary.sh reports the skip.
# Under TCG (CI, Linux) nothing is probed and the line runs as before.
EL2_OR_SKIP = reason=$$($(RUN_QEMU) --accel auto --probe-el2); \
	case $$? in 0) ;; 1) echo "[$(1)] skipped: $$reason"; exit 0 ;; *) exit 1 ;; esac;

test-qemu: $(TEST_KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(TEST_DISK)
	$(RUN_TEST) --label auto-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "cpus: 1" \
		--expect-line "memory: 131072 KiB total" \
		--expect-line "console: pl011 at 0x9000000 (/pl011@9000000)" \
		--expect-line "t_gui: all passed" \
		-- $(RUN_QEMU) --test --gui --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)
	$(RUN_TEST) --label auto-smp4-2g \
		--expect-line "cpus: 4" --expect-line "2048 MiB  RAM" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --smp 4 --mem 2G $(TEST_KERNEL)
	$(call EL2_OR_SKIP,auto-el2) $(RUN_TEST) --label auto-el2 \
		--expect-line "exception level at entry: EL2, now EL1" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --machine-opt virtualization=on $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "(GICv2)" \
		--expect-line "gic: version 2" --expect-line "t_gui: all passed" \
		-- $(RUN_QEMU) --test --gui --disk $(TEST_DISK) --accel tcg $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-smp4-gicv2 \
		--expect-line "cpus: 4" --expect-line "(GICv2)" --expect-line "gic: version 2" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --smp 4 $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-el2 \
		--expect-line "exception level at entry: EL2, now EL1" --expect-line "psci: arm,psci-1.0 via smc" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --machine-opt virtualization=on $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-smp4-gicv3 \
		--expect-line "cpus: 4" --expect-line "(GICv3)" \
		--expect-line "gic: version 3" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --smp 4 --mem 2G --machine-opt gic-version=3 $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

# One configuration, for working on the kernel.
test-qemu-one: $(TEST_KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(TEST_DISK)
	$(RUN_TEST) --label auto-default -- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

# The normal kernel, which the test kernel does not exercise: its own path
# through boot, its heap and timer checks, the receive interrupt, and the shell
# with keys sent by the runner. The script powers the machine off.
SHELL_SCRIPT := tests/shell/console.script
LEAK_SCRIPT := tests/shell/leak.script
EXIT_SCRIPT := tests/shell/exit.script
FILES_WRITE := tests/shell/files-write.script
FILES_READ := tests/shell/files-read.script
STORAGE_DOWN := tests/shell/storage-down.script
DEVICE_ERROR := tests/shell/device-error.script
CRASH_IMAGE := $(BUILD)/crash.img
CRASH_ROUNDS ?= 10
RUN_BOOT = $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(SHELL_SCRIPT)

test-boot: $(KERNEL) $(RUNNER)
	$(RUN_BOOT:$(SHELL_SCRIPT)=$(EXIT_SCRIPT)) --label exit-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUNNER) --timeout 240 --script $(LEAK_SCRIPT) --label leak-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT) --label boot-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT) --label boot-auto-smp4-2g -- $(RUN_QEMU) --test --keys --accel auto --smp 4 --mem 2G $(KERNEL)
	$(call EL2_OR_SKIP,boot-auto-el2) $(RUN_BOOT) --label boot-auto-el2 -- $(RUN_QEMU) --test --keys --accel auto --machine-opt virtualization=on $(KERNEL)
	$(RUN_BOOT) --label boot-tcg -- $(RUN_QEMU) --test --keys --accel tcg $(KERNEL)
	$(RUN_BOOT) --label boot-tcg-smp4-gicv2 -- $(RUN_QEMU) --test --keys --accel tcg --smp 4 $(KERNEL)
	$(RUN_BOOT) --label boot-tcg-el2 -- $(RUN_QEMU) --test --keys --accel tcg --machine-opt virtualization=on $(KERNEL)
	$(RUN_BOOT) --label boot-tcg-smp4-gicv3 -- $(RUN_QEMU) --test --keys --accel tcg --smp 4 --mem 2G --machine-opt gic-version=3 $(KERNEL)

# The roadmap's criterion: two boots of one image, and the image is clean.
test-files: $(KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(FILES_WRITE) --label files-write -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(FILES_READ) --label files-read -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# A volume that cannot be mounted: ADR-0080's limit on restarts, from `init`.
# Then one failed write, injected in QEMU's block layer.
	head -c 1048576 /dev/zero > $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(STORAGE_DOWN) --label storage-down -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-mkfs --size 16M --from rootfs $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(DEVICE_ERROR) --label device-error -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --disk-write-error --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)

# The stage 6c criterion (TASK-20): C programs built by zcc against libc.a run
# through libposix against fsd, from posixrun, under HVF (on a Mac) and TCG.
# libc.a needs musl's source in the cache of `make fetch-ports` (ADR-0109:
# the fetch is outside make test, as the Rust toolchain's install is).
POSIX_CORE := $(BUILD)/posix-core
POSIX_CORE_SCRIPT := tests/posix-core/libposix.script
test-libposix: $(KERNEL) $(RUNNER) tools-storage
	@$(MAKE) -s -f ports/Makefile libc
	@$(MAKE) -s -f ports/Makefile check-libc
	rm -rf $(POSIX_CORE) && mkdir -p $(POSIX_CORE)/root/bin $(POSIX_CORE)/root/work
	cp -R rootfs/. $(POSIX_CORE)/root/
	tools/zcc -O2 -o $(POSIX_CORE)/root/bin/t_libposix tests/posix-core/t_libposix.c
	$(CARGO) build -q -p zoraya-elf-check
	target/debug/zoraya-elf-check $(POSIX_CORE)/root/bin/t_libposix
	@for accel in auto tcg; do \
	  target/debug/zoraya-mkfs --size 32M --from $(POSIX_CORE)/root $(POSIX_CORE)/disk.img && \
	  $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(POSIX_CORE_SCRIPT) --label libposix-$$accel -- \
	    $(RUN_QEMU) --test --keys --disk $(POSIX_CORE)/disk.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(POSIX_CORE)/disk.img || exit 1; done

# The stage 6d criterion (TASK-21): the POSIX session, dash on ttyd through
# the broker, on an image of the ports' root (make ports' build/rootfs-posix),
# rootfs/ and tests/posix-session/rootfs (with /etc/zoraya/session). Needs make
# fetch-ports once, as make ports does. POSIX_SCRIPTS picks the scripts,
# POSIX_ACCELS the accelerators.
POSIX_SESSION := $(BUILD)/posix-session
POSIX_SCRIPTS ?= tests/posix-session/session.script tests/posix-session/invariants.script \
	tests/posix-session/scenarios.script
POSIX_ACCELS ?= auto tcg
posix-image: $(KERNEL) tools-storage
	@$(MAKE) -s -f ports/Makefile rootfs
	rm -rf $(POSIX_SESSION)/root
	mkdir -p $(POSIX_SESSION)/root
	cp -R rootfs/. $(POSIX_SESSION)/root/
	cp -R $(BUILD)/rootfs-posix/. $(POSIX_SESSION)/root/
	cp -R tests/posix-session/rootfs/. $(POSIX_SESSION)/root/
	tools/zcc -O2 -o $(POSIX_SESSION)/root/bin/t_posix tests/posix-session/t_posix.c
	$(CARGO) build -q -p zoraya-elf-check
	target/debug/zoraya-elf-check $(POSIX_SESSION)/root/bin/t_posix
	@# W10 as TASK-02b ran it (ADR-0110 item 1): the script and mktab.c.
	mkdir -p $(POSIX_SESSION)/root/home/w10
	cp tools/posix-surface/workloads/w10/w10.sh tools/posix-surface/workloads/w8/mktab.c \
	  $(POSIX_SESSION)/root/home/w10/

test-posix: posix-image $(RUNNER)
	@for script in $(POSIX_SCRIPTS); do for accel in $(POSIX_ACCELS); do \
	  name=$$(basename $$script .script); \
	  target/debug/zoraya-mkfs --size 64M --from $(POSIX_SESSION)/root $(POSIX_SESSION)/disk.img && \
	  $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $$script --label posix-$$name-$$accel -- \
	    $(RUN_QEMU) --test --keys --disk $(POSIX_SESSION)/disk.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(POSIX_SESSION)/disk.img || exit 1; done; done

# The same session in the stage 5 terminal window (ttyd's window backend),
# read back by screendump as make test-gui reads zosh.
POSIX_GUI_SCRIPT := tests/posix-session/gui.script
test-posix-gui: posix-image $(RUNNER)
	@for accel in $(POSIX_ACCELS); do \
	  target/debug/zoraya-mkfs --size 64M --from $(POSIX_SESSION)/root $(POSIX_SESSION)/gui.img && \
	  $(RUNNER) --timeout $(QEMU_TIMEOUT) --gui-script $(POSIX_GUI_SCRIPT) --qmp $(BUILD)/posix-gui-$$accel.qmp \
	    --label posix-gui-$$accel -- $(RUN_QEMU) --test --gui --mem 256M --qmp $(BUILD)/posix-gui-$$accel.qmp \
	    --disk $(POSIX_SESSION)/gui.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(POSIX_SESSION)/gui.img || exit 1; done

# The stage 5 criterion (TASK-14): the GUI headless, keys through QMP, the
# screen read back as text. Under the host's accelerator and under TCG.
GUI_IMAGE := $(BUILD)/gui.img
GUI_SCRIPT := tests/gui/terminal.script
test-gui: $(KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(GUI_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --gui-script $(GUI_SCRIPT) --qmp $(BUILD)/gui-auto.qmp --label gui-auto -- \
		$(RUN_QEMU) --test --gui --mem 256M --qmp $(BUILD)/gui-auto.qmp --disk $(GUI_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(GUI_IMAGE)
	target/debug/zoraya-mkfs --size 16M --from rootfs $(GUI_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --gui-script $(GUI_SCRIPT) --qmp $(BUILD)/gui-tcg.qmp --label gui-tcg -- \
		$(RUN_QEMU) --test --gui --mem 256M --qmp $(BUILD)/gui-tcg.qmp --disk $(GUI_IMAGE) --accel tcg $(KERNEL)
	target/debug/zoraya-fsck $(GUI_IMAGE)

# Crashes at random points. CRASH_ROUNDS=200 for the number in the report.
test-crash: $(KERNEL) tools-storage
	$(CARGO) build -q -p zoraya-test-runner
	target/debug/zoraya-crash-test --rounds $(CRASH_ROUNDS) --kernel $(KERNEL) --image $(CRASH_IMAGE)

# Mutation testing: tools/mutants/*.toml, survivors in build/mutants/report.md.
# MUTANTS_ARGS="--only ID,ID" or "--jobs N" or a list's path narrows it.
# At most two mutants run at once, each under its list's timeout, and the
# watchdog kills any process above WATCHDOG_MB (docs/backlog.md).
mutants:
	$(CARGO) build -q -p zoraya-mutants -p zoraya-footprint
	tools/mutants/watchdog.sh target/debug/zoraya-mutants $(MUTANTS_ARGS)

# cargo-mutants on one host crate: mutants made from the code's syntax, as
# opposed to the hand-written lists above. Missed ones in
# build/mutants/cargo/mutants.out/missed.txt. Not part of make test.
# MUTANTS_HOST_ARGS adds cargo-mutants options, such as --in-diff FILE or
# --exclude src/ffi.rs (code the host does not compile).
CRATE ?= zoraya-virtio
mutants-host:
	@command -v cargo-mutants >/dev/null || { echo "cargo-mutants is not installed: cargo install cargo-mutants --version 27.1.0 --locked (docs/dependencies.md)"; exit 2; }
	@# cargo-mutants exits 2 when a mutant was missed and 3 on a timeout: results, not failures.
	@$(CARGO) build -q -p zoraya-footprint
	@mkdir -p build/mutants; tools/mutants/watchdog.sh $(CARGO) mutants -p $(CRATE) -j 2 --timeout 300 $(MUTANTS_HOST_ARGS) -o build/mutants/cargo > build/mutants/cargo.log 2>&1; \
	  rc=$$?; tail -1 build/mutants/cargo.log; \
	  case $$rc in 0|2|3) echo "missed: build/mutants/cargo/mutants.out/missed.txt";; \
	  *) echo "cargo-mutants failed ($$rc), see build/mutants/cargo.log"; exit $$rc;; esac

# Stage 6b: the cross toolchain, the sysroot and the ported programs
# (ADR-0109, ports/Makefile). fetch-ports is the only step that downloads.
.PHONY: fetch-ports ports check-ports clean-ports check-fetch-ports check-test-caps
fetch-ports ports check-ports clean-ports:
	@$(MAKE) -f ports/Makefile $@

# INV-0109-3 and INV-0109-4: the fetch step and the cache, offline.
check-fetch-ports:
	ports/test-fetch.sh

tools-storage:
	$(CARGO) build -q -p zoraya-mkfs -p zoraya-fsck
	@mkdir -p $(BUILD)

fmt-check:
	$(CARGO) fmt --all --check

clippy:
	$(CARGO) clippy --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt --all-targets -- -D warnings
	$(CARGO) clippy -p zoraya-user -p zoraya-rt --target $(TARGET) -- -D warnings
	@# zoraya-posix links zoraya-rt without its entry point: apart, so that
	@# the user programs' features do not unify with it.
	$(CARGO) clippy -p zoraya-posix --target $(TARGET) -- -D warnings
	$(CARGO) clippy -p zoraya-rt --no-default-features --target $(TARGET) -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) --features qemu-test --target-dir target/qemu-test -- -D warnings

# ADR-0001 invariants (docs/adr/invariants.md).
adr-check:
	$(CARGO) run -q -p zoraya-adr-check -- docs

# INV-0031-4: only usercopy.rs may use the unprivileged load and store forms.
check-user-access:
	@! grep -rnE 'ldtr|sttr' kernel/src --include='*.rs' --exclude=usercopy.rs

# INV-0105-4: no terminal policy in the kernel; the console stays a raw ring
# (ADR-0060), and the line discipline is ttyd's.
check-kernel-tty:
	@! grep -rnE 'ICANON|ECHO|ISIG|VINTR|VQUIT|VSUSP|VERASE|VKILL|VEOF|ONLCR|TOSTOP|termios|winsize' kernel/src

# No host paths or email addresses in tracked files (CLAUDE.md, Conventions), apart from
# the configured git identity's address. docs/research/ and tools/posix-surface/results/
# are cleaned separately. The brackets keep the pattern from matching this file.
check-host-paths:
	@id="$$(git config user.email)"; \
	hits="$$(git grep -n -I -E '/Use[r]s/|/hom[e]/|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+[.][A-Za-z]{2,}' -- . \
	  ':!docs/research/' ':!tools/posix-surface/results/' | { if [ -n "$$id" ]; then grep -vF "$$id"; else cat; fi; })"; \
	if [ -n "$$hits" ]; then echo "host paths or email addresses in tracked files:"; echo "$$hits"; exit 1; fi

# Every host-tested crate caps its test heap (CLAUDE.md, Memory limits): the
# library, if it has tests, and every top-level file of tests/, by the cap or
# by a counting allocator over `Capped`. An integration test binary does not
# get the library's cap: TASK-20's mutants found that (docs/backlog.md).
check-test-caps:
	@bad=""; \
	for f in crates/*/tests/*.rs; do \
	  grep -q -e 'cap_memory!' -e 'Capped::new' "$$f" || bad="$$bad $$f"; done; \
	for d in crates/*; do \
	  if grep -rqs -e '#\[test\]' "$$d/src"; then \
	    grep -rqs -e 'cap_memory!' -e 'Capped::new' "$$d/src" || bad="$$bad $$d/src"; fi; done; \
	if [ -n "$$bad" ]; then echo "no heap cap in:$$bad"; exit 1; fi

# `make test` prints a summary and keeps each step's output in build/logs/;
# `make test V=1` prints everything as it runs.
TEST_STEPS := fmt-check clippy test-unit adr-check check-user-access check-kernel-tty check-host-paths check-test-caps check-fetch-ports test-qemu test-boot test-files test-libposix test-posix test-posix-gui test-crash test-gui

ifeq ($(V),1)
test: $(TEST_STEPS)
	@echo "make test: all checks passed"
else
test:
	@MAKE="$(MAKE)" tools/test-summary.sh $(TEST_STEPS)
endif

clean:
	$(CARGO) clean
	rm -rf $(BUILD)
