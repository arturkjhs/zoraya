# Zoraya build and test entry points. See docs/design.md, "Where things are".
#
#   make build      build the kernel (build/zoraya-kernel.elf)
#   make run        boot it interactively (Ctrl-A X quits QEMU)
#   make test-unit  host tests (cargo test) for crates/ and tools/
#   make test-qemu  in-QEMU tests under several machine configurations
#   make test-boot  boot the normal kernel, run the shell script under each
#                   machine configuration
#   make test       all of the above plus fmt, clippy and the ADR checks
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
	crashwriter crashcheck fsbench t_storage
RUNNER := target/debug/zoraya-test-runner
RUN_QEMU := tools/run-qemu.sh
QEMU_TIMEOUT ?= 180

KERNEL_ARGS := -p zoraya-kernel --target $(TARGET)

.PHONY: all build run debug test test-unit test-qemu test-qemu-one test-boot test-files test-crash tools-storage ramdisk fmt-check check-user-access clippy adr-check clean \
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

test-qemu: $(TEST_KERNEL) $(RUNNER) $(TEST_DISK)
	target/debug/zoraya-mkfs --size 16M --from rootfs $(TEST_DISK)
	$(RUN_TEST) --label auto-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "cpus: 1" \
		--expect-line "memory: 131072 KiB total" \
		--expect-line "console: pl011 at 0x9000000 (/pl011@9000000)" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)
	$(RUN_TEST) --label auto-smp4-2g \
		--expect-line "cpus: 4" --expect-line "2048 MiB  RAM" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --smp 4 --mem 2G $(TEST_KERNEL)
	$(RUN_TEST) --label auto-el2 \
		--expect-line "exception level at entry: EL2, now EL1" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --machine-opt virtualization=on $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "(GICv2)" \
		--expect-line "gic: version 2" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg $(TEST_KERNEL)
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

# The image the in-QEMU tests use: made fresh by each target.
$(TEST_DISK):
	$(CARGO) build -q -p zoraya-mkfs -p zoraya-fsck
	@mkdir -p $(BUILD)
	target/debug/zoraya-mkfs --size 16M --from rootfs $@

# One configuration, for working on the kernel.
test-qemu-one: $(TEST_KERNEL) $(RUNNER) $(TEST_DISK)
	target/debug/zoraya-mkfs --size 16M --from rootfs $(TEST_DISK)
	$(RUN_TEST) --label auto-default -- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)

# The normal kernel, which the test kernel does not exercise: its own path
# through boot, its heap and timer checks, the receive interrupt, and the shell
# with keys sent by the runner. The script powers the machine off.
SHELL_SCRIPT := tests/shell/console.script
LEAK_SCRIPT := tests/shell/leak.script
EXIT_SCRIPT := tests/shell/exit.script
FILES_WRITE := tests/shell/files-write.script
FILES_READ := tests/shell/files-read.script
CRASH_IMAGE := $(BUILD)/crash.img
CRASH_ROUNDS ?= 10
RUN_BOOT = $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(SHELL_SCRIPT)

test-boot: $(KERNEL) $(RUNNER)
	$(RUN_BOOT:$(SHELL_SCRIPT)=$(EXIT_SCRIPT)) --label exit-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUNNER) --timeout 240 --script $(LEAK_SCRIPT) --label leak-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT) --label boot-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT) --label boot-auto-smp4-2g -- $(RUN_QEMU) --test --keys --accel auto --smp 4 --mem 2G $(KERNEL)
	$(RUN_BOOT) --label boot-auto-el2 -- $(RUN_QEMU) --test --keys --accel auto --machine-opt virtualization=on $(KERNEL)
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

# Crashes at random points. CRASH_ROUNDS=200 for the number in the report.
test-crash: $(KERNEL) tools-storage
	$(CARGO) build -q -p zoraya-test-runner
	target/debug/zoraya-crash-test --rounds $(CRASH_ROUNDS) --kernel $(KERNEL) --image $(CRASH_IMAGE)

tools-storage:
	$(CARGO) build -q -p zoraya-mkfs -p zoraya-fsck
	@mkdir -p $(BUILD)

fmt-check:
	$(CARGO) fmt --all --check

clippy:
	$(CARGO) clippy --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt --all-targets -- -D warnings
	$(CARGO) clippy -p zoraya-user -p zoraya-rt --target $(TARGET) -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) --features qemu-test --target-dir target/qemu-test -- -D warnings

# ADR-0001 invariants (docs/adr/invariants.md).
adr-check:
	$(CARGO) run -q -p zoraya-adr-check -- docs

# INV-0031-4: only usercopy.rs may use the unprivileged load and store forms.
check-user-access:
	@! grep -rnE 'ldtr|sttr' kernel/src --include='*.rs' --exclude=usercopy.rs

test: fmt-check clippy test-unit adr-check check-user-access test-qemu test-boot test-files test-crash
	@echo "make test: all checks passed"

clean:
	$(CARGO) clean
	rm -rf $(BUILD)
