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
#   make test-posix the POSIX session: W5, W8, W9, W10, tcc hello.c (stage
#                   6e, ADR-0110); make test-posix-long the builds W1, W2,
#                   W3, W2-j, W3-j and the call log's union (not in make
#                   test); make measure-w4 GNU hello's configure and make
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

# Local runs skip what CI runs anyway: the TCG configurations and runs, the
# tour's second run (under TCG) and crash rounds past 2. FULL=1 runs
# everything; CI (where GitHub sets CI=true) always does.
FULL ?= $(if $(CI),1,0)
ifeq ($(FULL),1)
LOCAL_ACCELS := auto tcg
else
LOCAL_ACCELS := auto
endif
# A recipe line that runs only with FULL=1.
FULL_ONLY = [ "$(FULL)" = 1 ] || exit 0;
KERNEL := $(BUILD)/zoraya-kernel.elf
TEST_KERNEL := $(BUILD)/zoraya-kernel-test.elf
RAMDISK := $(BUILD)/ramdisk.bin
DISK := $(BUILD)/disk.img
TEST_DISK := $(BUILD)/test-disk.img
USER_DIR := target/user/$(TARGET)/user
USER_PROGRAMS := hello spin isolate badptr fuzz regs f_kread f_kwrite f_null f_priv \
	f_undef f_exdata f_stack f_brk f_wfi f_align regsave lone koverflow copyfar \
	parent child counter portclose t_handles t_spawn t_signals t_ports portbench t_stress t_leak \
	busy zosh echo-args sleep exit-with fault fanout flood ticker handles stubborn requests probe t_console t_memory t_channels init t_devices t_loop chanbench blkd fsd \
	crashwriter crashcheck fsbench t_storage gpud inputd compd term panel chooser t_open files settings edit t_gui guibench t_text \
	ethd t_net t_streams t_snapshot t_image t_suspend t_quota t_bound posixrun ls cat cp mv rm mkdir stat head find search broker ttyd
RUNNER := target/debug/zoraya-test-runner
RUN_QEMU := tools/run-qemu.sh
QEMU_TIMEOUT ?= 180

KERNEL_ARGS := -p zoraya-kernel --target $(TARGET)

.PHONY: all build run run-gui run-net test-net debug check test-changed test-par test test-unit test-qemu test-qemu-one test-qemu-disk ci-build check-ci-jobs $(addprefix test-qemu-,$(QEMU_CONFIGS)) test-boot test-files test-libposix test-posix test-posix-long posix-calls posix-table measure-w4 test-posix-gui test-tour-gui posix-image tour-scripts test-crash test-gui mutants mutants-host tools-storage ramdisk fmt-check check-user-access check-kernel-tty check-argv check-host-paths clippy adr-check clean \
	$(KERNEL) $(TEST_KERNEL) $(RUNNER) $(RAMDISK)

all: build

build: $(KERNEL)

# User programs go into the ramdisk, and the ramdisk into the kernel image
# (ADR-0033). They have their own target directory and a stripped profile.
$(RAMDISK):
	$(CARGO) build -p zoraya-user --target $(TARGET) --profile user --target-dir target/user
	$(CARGO) build -p zoraya-mkramdisk
	@mkdir -p $(BUILD)
	target/debug/zoraya-mkramdisk --describe user/describe $@ $(addprefix $(USER_DIR)/,$(USER_PROGRAMS))

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
# On macOS the window is on a Retina screen as a rule: twice the pixels,
# scale 2 (the architect's check of 2026-11-10); GUI_SCALE=1 gives 1024x768.
GUI_SCALE ?= $(if $(filter Darwin,$(shell uname -s)),2,1)
run-gui: $(KERNEL) $(DISK)
	$(RUN_QEMU) --gui --gui-scale $(GUI_SCALE) --mem 512M --disk $(DISK) $(KERNEL)

debug: $(KERNEL)
	@echo "Waiting for a debugger on localhost:1234. Attach with:"
	@echo "  lldb $(KERNEL) -o 'gdb-remote localhost:1234'   or"
	@echo "  gdb-multiarch $(KERNEL) -ex 'target remote localhost:1234'"
	$(RUN_QEMU) --debug $(KERNEL)

# UNIT_ARGS narrows it to packages (CI's plan on a merge candidate).
UNIT_ARGS ?= --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt
test-unit:
	$(CARGO) test $(UNIT_ARGS)

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

# One target per machine configuration (CI runs each as a job of its own,
# ADR-0213's Decision); make test-qemu runs them all, in this order, on one
# test disk, with zoraya-fsck after each.
QEMU_CONFIGS := auto-default auto-smp4-2g auto-el2 tcg-default tcg-smp4-gicv2 tcg-el2 tcg-smp4-gicv3
QEMU_RUN := $(if $(filter 1,$(FULL)),$(QEMU_CONFIGS),$(filter auto-%,$(QEMU_CONFIGS)))
test-qemu: $(addprefix test-qemu-,$(QEMU_RUN))

test-qemu-disk: $(TEST_KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(TEST_DISK)

test-qemu-auto-default: test-qemu-disk
	$(RUN_TEST) --label auto-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "cpus: 1" \
		--expect-line "memory: 131072 KiB total" \
		--expect-line "console: pl011 at 0x9000000 (/pl011@9000000)" \
		--expect-line "t_gui: all passed" \
		-- $(RUN_QEMU) --test --gui --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-auto-smp4-2g: test-qemu-disk
	$(RUN_TEST) --label auto-smp4-2g \
		--expect-line "cpus: 4" --expect-line "2048 MiB  RAM" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --smp 4 --mem 2G $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-auto-el2: test-qemu-disk
	$(call EL2_OR_SKIP,auto-el2) $(RUN_TEST) --label auto-el2 \
		--expect-line "exception level at entry: EL2, now EL1" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --machine-opt virtualization=on $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-default: test-qemu-disk
	$(RUN_TEST) --label tcg-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "(GICv2)" \
		--expect-line "gic: version 2" --expect-line "t_gui: all passed" \
		-- $(RUN_QEMU) --test --gui --disk $(TEST_DISK) --accel tcg $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-smp4-gicv2: test-qemu-disk
	$(RUN_TEST) --label tcg-smp4-gicv2 \
		--expect-line "cpus: 4" --expect-line "(GICv2)" --expect-line "gic: version 2" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --smp 4 $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-el2: test-qemu-disk
	$(RUN_TEST) --label tcg-el2 \
		--expect-line "exception level at entry: EL2, now EL1" --expect-line "psci: arm,psci-1.0 via smc" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --machine-opt virtualization=on $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-smp4-gicv3: test-qemu-disk
	$(RUN_TEST) --label tcg-smp4-gicv3 \
		--expect-line "cpus: 4" --expect-line "(GICv3)" \
		--expect-line "gic: version 3" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --smp 4 --mem 2G --machine-opt gic-version=3 $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-one: $(TEST_KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(TEST_DISK)
	$(RUN_TEST) --label auto-default -- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

# N1 (ADR-0221, ADR-0230): the test kernel with a virtio-net device on QEMU's
# restricted user network, whose `t_net` (ethd, ARP through both paths) is
# the point; the pcap stays in target/test-logs/. A host whose QEMU has no
# user-mode networking skips the step, listed by `make test`'s summary (INV-0230-3);
# on CI (CI set) a skip, and any other failure of the probe, fails it.
NET_DISK := $(BUILD)/net-disk.img
NET_PCAP := target/test-logs/test-net.pcap
# Forwarded to the guest so that SLIRP asks who has 10.0.2.15 (tools/net-poke.sh).
NET_FWD_PORT := 47015
test-net: $(TEST_KERNEL) $(RUNNER) tools-storage
	@$(RUN_QEMU) --probe-net; rc=$$?; \
	if [ $$rc = 1 ]; then \
		if [ -n "$$CI" ]; then echo "[net] FAIL: no user-mode networking on a CI runner"; exit 1; fi; \
		echo "[net] skipped: QEMU has no user-mode networking"; exit 0; \
	elif [ $$rc != 0 ]; then echo "[net] FAIL: the networking probe failed (exit $$rc)"; exit 1; fi; \
	mkdir -p target/test-logs && rm -f $(NET_PCAP) && \
	target/debug/zoraya-mkfs --size 16M --from rootfs $(NET_DISK) && \
	run() { accel=$$1; shift; tools/net-poke.sh $(NET_FWD_PORT) 240 & poke=$$!; \
		$(RUN_TEST) "$$@" -- $(RUN_QEMU) --test --net --net-forward $(NET_FWD_PORT) --net-pcap $(NET_PCAP) --disk $(NET_DISK) --accel $$accel $(TEST_KERNEL); \
		rc=$$?; kill $$poke 2>/dev/null; wait $$poke 2>/dev/null; return $$rc; }; \
	run auto --label net-auto --expect-line "t_net: all passed" --expect-line "t_net: arp reply from 10.0.2.2" && \
	target/debug/zoraya-fsck $(NET_DISK) && \
	hex="$$(od -An -tx1 -v $(NET_PCAP) | tr -d ' \n')" && \
	( case "$$hex" in *08060001080006040002525[0-9a-f]0a000202*) ;; \
		*) echo "[net-auto] FAIL: no ARP reply from the gateway in $(NET_PCAP)"; exit 1 ;; esac ) && \
	( case "$$hex" in *080600010800060400025254001234560a00020f*) ;; \
		*) echo "[net-auto] FAIL: the guest answered no ARP request for 10.0.2.15 in $(NET_PCAP)"; exit 1 ;; esac ) && \
	( [ "$(FULL)" != 1 ] || run tcg --label net-tcg --expect-line "t_net: all passed" )

# An interactive machine on QEMU's user network without `restrict`: the guest
# reaches the Internet through the host, for trying things by hand.
run-net: $(KERNEL) $(DISK)
	$(RUN_QEMU) --net --disk $(DISK) $(KERNEL)

# The normal kernel, which the test kernel does not exercise: its own path
# through boot, its heap and timer checks, the receive interrupt, and the shell
# with keys sent by the runner. The script powers the machine off.
SHELL_SCRIPT := tests/shell/console.script
LEAK_SCRIPT := tests/shell/leak.script
EXIT_SCRIPT := tests/shell/exit.script
JOBS_SCRIPT := tests/shell/jobs.script
REBOOT_SCRIPT := tests/shell/reboot.script
CONVENTIONS_SCRIPT := tests/shell/conventions.script
FILES_WRITE := tests/shell/files-write.script
FILES_READ := tests/shell/files-read.script
STORAGE_DOWN := tests/shell/storage-down.script
DEVICE_ERROR := tests/shell/device-error.script
GRANTS_SCRIPT := tests/shell/grants.script
GRANTS_ROOT := $(BUILD)/grants-root
CRASH_IMAGE := $(BUILD)/crash.img
CRASH_ROUNDS ?= $(if $(filter 1,$(FULL)),10,2)
RUN_BOOT = $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(SHELL_SCRIPT)

test-boot: $(KERNEL) $(RUNNER)
	$(RUN_BOOT:$(SHELL_SCRIPT)=$(EXIT_SCRIPT)) --label exit-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT:$(SHELL_SCRIPT)=$(JOBS_SCRIPT)) --label jobs-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT:$(SHELL_SCRIPT)=$(REBOOT_SCRIPT)) --label reboot-auto -- $(RUN_QEMU) --test --keys --reboot --accel auto $(KERNEL)
	$(RUNNER) --timeout 240 --script $(LEAK_SCRIPT) --label leak-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT) --label boot-auto -- $(RUN_QEMU) --test --keys --accel auto $(KERNEL)
	$(RUN_BOOT) --label boot-auto-smp4-2g -- $(RUN_QEMU) --test --keys --accel auto --smp 4 --mem 2G $(KERNEL)
	$(call EL2_OR_SKIP,boot-auto-el2) $(RUN_BOOT) --label boot-auto-el2 -- $(RUN_QEMU) --test --keys --accel auto --machine-opt virtualization=on $(KERNEL)
	$(FULL_ONLY) $(RUN_BOOT) --label boot-tcg -- $(RUN_QEMU) --test --keys --accel tcg $(KERNEL)
	$(FULL_ONLY) $(RUN_BOOT) --label boot-tcg-smp4-gicv2 -- $(RUN_QEMU) --test --keys --accel tcg --smp 4 $(KERNEL)
	$(FULL_ONLY) $(RUN_BOOT) --label boot-tcg-el2 -- $(RUN_QEMU) --test --keys --accel tcg --machine-opt virtualization=on $(KERNEL)
	$(FULL_ONLY) $(RUN_BOOT) --label boot-tcg-smp4-gicv3 -- $(RUN_QEMU) --test --keys --accel tcg --smp 4 --mem 2G --machine-opt gic-version=3 $(KERNEL)

# The roadmap's criterion: two boots of one image, and the image is clean.
test-files: $(KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(FILES_WRITE) --label files-write -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(FILES_READ) --label files-read -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# INV-0155-1: every command's --help and usage error, with a volume.
	target/debug/zoraya-mkfs --size 16M --from rootfs $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(CONVENTIONS_SCRIPT) --label conventions -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# What a command gets (ADR-0153): rootfs/ with link objects over it.
	rm -rf $(GRANTS_ROOT) && mkdir -p $(GRANTS_ROOT) && cp -R rootfs/. tests/shell/grants-root/. $(GRANTS_ROOT)/
	target/debug/zoraya-mkfs --size 16M --from $(GRANTS_ROOT) $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(GRANTS_SCRIPT) --label grants -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
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
	@for accel in $(LOCAL_ACCELS); do \
	  target/debug/zoraya-mkfs --size 32M --from $(POSIX_CORE)/root $(POSIX_CORE)/disk.img && \
	  $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(POSIX_CORE_SCRIPT) --label libposix-$$accel -- \
	    $(RUN_QEMU) --test --keys --disk $(POSIX_CORE)/disk.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(POSIX_CORE)/disk.img || exit 1; done

# The POSIX session (TASK-21, ADR-0110): dash on ttyd through the broker, on
# an image of the ports' root (make ports' build/rootfs-posix), rootfs/ and
# tests/posix/rootfs (with /etc/zoraya/session and /tests/wN.sh), in
# ADR-0110's layout: the root in build/posix/root, the image
# build/posix-disk.img. The root also holds the workloads' sources as
# ports/sources.lock pins them (/src), W8's and W10's files as TASK-02b ran
# them, W3-j's Makefile, and a fresh tree per build in /work
# (tests/posix/trees.sh). Needs make fetch-ports once, as make ports does.
POSIX_SESSION := $(BUILD)/posix
POSIX_ROOT := $(POSIX_SESSION)/root
POSIX_DISK := $(BUILD)/posix-disk.img
POSIX_ACCELS ?= $(LOCAL_ACCELS)
WORKLOADS_SRC := tools/posix-surface/workloads
posix-image: $(KERNEL) tools-storage
	@$(MAKE) -s -f ports/Makefile rootfs workloads
	rm -rf $(POSIX_ROOT)
	mkdir -p $(POSIX_ROOT)
	cp -R rootfs/. $(POSIX_ROOT)/
	cp -R $(BUILD)/rootfs-posix/. $(POSIX_ROOT)/
	cp -R tests/posix/rootfs/. $(POSIX_ROOT)/
	tools/zcc -O2 -o $(POSIX_ROOT)/bin/t_posix tests/posix/t_posix.c
	$(CARGO) build -q -p zoraya-elf-check
	target/debug/zoraya-elf-check $(POSIX_ROOT)/bin/t_posix
	mkdir -p $(POSIX_ROOT)/src/w10 $(POSIX_ROOT)/src/w3
	cp -R $(BUILD)/ports/workloads/tcc $(BUILD)/ports/workloads/lua $(BUILD)/ports/workloads/sqlite \
	  $(BUILD)/ports/workloads/hello $(POSIX_ROOT)/src/
	cp -R $(WORKLOADS_SRC)/w8 $(POSIX_ROOT)/src/w8
	cp $(WORKLOADS_SRC)/w10/w10.sh $(WORKLOADS_SRC)/w8/mktab.c $(POSIX_ROOT)/src/w10/
	@# W3-j's Makefile with ADR-0210's flag on both compiles, as W3's.
	sed -e 's/-c sqlite3.c$$/-DSQLITE_OMIT_LOAD_EXTENSION -c sqlite3.c/' \
	  -e 's/tcc -c shell.c$$/tcc -DSQLITE_OMIT_LOAD_EXTENSION -c shell.c/' \
	  $(WORKLOADS_SRC)/w3/Makefile > $(POSIX_ROOT)/src/w3/Makefile
	grep -c OMIT_LOAD_EXTENSION $(POSIX_ROOT)/src/w3/Makefile | grep -qx 2
	tests/posix/trees.sh $(POSIX_ROOT)
	@# The same root without the session file: init then starts zosh, with
	@# the broker beside it (TASK-18 step 10, tests/posix/zosh*.script).
	rm -rf $(POSIX_ZOSH_ROOT)
	cp -R $(POSIX_ROOT) $(POSIX_ZOSH_ROOT)
	rm $(POSIX_ZOSH_ROOT)/etc/zoraya/session
	@# The tour's (docs/tour.md): rootfs/ and the ports' programs, an empty
	@# /home and no session file.
	rm -rf $(POSIX_TOUR_ROOT)
	mkdir -p $(POSIX_TOUR_ROOT)
	cp -R rootfs/. $(BUILD)/rootfs-posix/. $(POSIX_TOUR_ROOT)/

# One boot per script under each accelerator, zoraya-fsck after each
# (ADR-0110 item 6). The runner's timeout per script comes from this table,
# set from the first measured durations (TASK-22) with a margin of about
# three; QEMU_TIMEOUT for the others.
POSIX_TIMEOUTS := w1=600 w2=600 w2j=900 w3=600 w3j=900 w8=1800 w9-int=600 w9-term=600 \
	w10=600 w4=1800 w5-jobs=300 tour=900 w8-j1=600 w8-pipe-j2=600 w8-fifo-j2=600 w8-pipe-j4=600 w8-fifo-j4=600
# W8's five builds: w8.script runs them in one boot; w8-BUILD.script runs one
# (CI's jobs, ADR-0213's Decision).
POSIX_W8_BUILDS := j1 pipe-j2 fifo-j2 pipe-j4 fifo-j4
POSIX_IMAGE_SIZE := 256M
# The directory /limited of the POSIX images carries a storage limit of this
# size (ADR-0265, Q5): `t_posix diskfill` writes into it until EDQUOT
# (tests/posix/scenarios.script). Until the room keeper (Q6a) sets limits, the
# image tool does.
POSIX_LIMITED ?= 64K
POSIX_ZOSH_ROOT := $(BUILD)/posix-zosh
POSIX_TOUR_ROOT := $(BUILD)/posix-tour
# The guest's memory for the POSIX session: W8 at -j4 ran out of 128 MiB
# ("tcc: memory full"; every POSIX process holds at least its 1 MiB of heap,
# its segments and 64 KiB of stack, and tmpd 16 MiB), with no leak across
# repeated builds (ADR-0211 item 9).
POSIX_MEM ?= 512M
# For script $$script under accelerator $$accel: the image, a boot, a check.
define posix_run
	name=$$(basename $$script .script); \
	timeout=$$(echo " $(POSIX_TIMEOUTS) " | sed -n "s/.* $$name=\([0-9]*\) .*/\1/p"); \
	root=$(POSIX_ROOT); case $$name in zosh*) root=$(POSIX_ZOSH_ROOT);; tour*) root=$(POSIX_TOUR_ROOT);; esac; \
	limit=""; [ -d $$root/limited ] && limit="--limit limited=$(POSIX_LIMITED)"; \
	target/debug/zoraya-mkfs --size $(POSIX_IMAGE_SIZE) $$limit --from $$root $(POSIX_DISK) > /dev/null && \
	$(RUNNER) --timeout $${timeout:-$(QEMU_TIMEOUT)} --script $$script --label posix-$$name-$$accel -- \
	  $(RUN_QEMU) --test --keys --mem $(POSIX_MEM) --disk $(POSIX_DISK) --accel $$accel $(KERNEL) && \
	target/debug/zoraya-fsck $(POSIX_DISK)
endef

# make test-posix (a step of make test, ADR-0110 item 6): the export check
# first (item 9: INV-0005-1, INV-0005-2), then W5, W8, W9 and W10 and the
# roadmap's criterion (items 2 to 5), and stage 6d's invariants and
# scenarios.
POSIX_SCRIPTS ?= tests/posix/hello.script tests/posix/w5.script tests/posix/w8.script \
	tests/posix/w9-int.script tests/posix/w9-term.script tests/posix/w10.script \
	tests/posix/invariants.script tests/posix/scenarios.script tests/posix/zosh.script \
	tests/posix/tour.script
# W8 runs under one accelerator here, the first of POSIX_ACCELS (auto: HVF
# where there is one, else TCG), and under the second in make
# test-posix-long (ADR-0212's Decision, part 1).
POSIX_ONCE := w8
POSIX_IMAGE_DEP ?= posix-image
test-posix: $(POSIX_IMAGE_DEP) $(RUNNER)
	@$(MAKE) -s -f ports/Makefile check-libc
	@for script in $(POSIX_SCRIPTS); do first=1; for accel in $(POSIX_ACCELS); do \
	  case " $(POSIX_ONCE) " in *" $$(basename $$script .script) "*) [ $$first = 1 ] || continue;; esac; \
	  first=0; $(posix_run) || exit 1; done; done

# make test-posix-long (not in make test; stage 6 is not merged until it
# passes, ADR-0110 item 6): the builds W1, W2, W3, W2-j and W3-j, then the
# call log's union over its own logs and those of make test-posix of the
# same build (item 8, INV-0005-3, INV-0110-2), which fails naming make
# test-posix when those are missing.
POSIX_LONG_SCRIPTS ?= tests/posix/w1.script tests/posix/w2.script tests/posix/w2j.script \
	tests/posix/w3.script tests/posix/w3j.script
POSIX_T1_RUNS := w1 w2 w2j w3 w3j w5 w8 w9-int w9-term w10
POSIX_LOGS := target/test-logs
test-posix-long: posix-image $(RUNNER)
	@for script in $(POSIX_LONG_SCRIPTS); do for accel in $(POSIX_ACCELS); do \
	  $(posix_run) || exit 1; done; done
	@for script in $(foreach w,$(POSIX_ONCE),tests/posix/$(w).script); do \
	  for accel in $(wordlist 2,9,$(POSIX_ACCELS)); do $(posix_run) || exit 1; done; done
	@$(MAKE) -s posix-calls

# The call log's union alone, over the logs there are (INV-0110-2).
posix-calls:
	$(CARGO) build -q -p posix-stats
	target/debug/posix-stats --calls $(BUILD)/sysroot/usr/lib/zoraya-calls.txt \
	  docs/adr/posix-surface-T1.txt tools/posix-surface/results/dynamic-programs.tsv \
	  docs/adr/0211-acceptance-choices-of-stage-6e.md \
	  $(foreach r,$(POSIX_T1_RUNS),$(wildcard $(foreach a,auto hvf tcg,$(POSIX_LOGS)/posix-$(r)-$(a).log))) \
	  --require $(POSIX_T1_RUNS)

# make measure-w4 (ADR-0110 items 10 and 12, not in make test): T2's
# applets probed against libc.a, then GNU hello's configure and make (W4)
# and the W5 extension under HVF (on a Mac) and TCG, and the W4 report,
# which fails without every counter of INV-0110-3 and its accelerator.
W4_ACCELS ?= $(if $(filter Darwin,$(shell uname -s)),hvf tcg,tcg)
W4_SCRIPTS := tests/posix/w4.script tests/posix/w5-jobs.script
W4_REPORT := $(BUILD)/w4-report.md
measure-w4: posix-image $(RUNNER)
	@$(MAKE) -s -f ports/Makefile t2-probe
	@for script in $(W4_SCRIPTS); do for accel in $(W4_ACCELS); do \
	  $(posix_run) || exit 1; done; done
	$(CARGO) build -q -p posix-stats
	{ echo "# W4: GNU hello's configure and make inside Zoraya"; echo; \
	  echo '```'; cat $(BUILD)/ports/logs/t2-probe.txt; echo '```'; echo; \
	  target/debug/posix-stats table --w4 $(foreach a,$(W4_ACCELS),$(POSIX_LOGS)/posix-w4-$(a).log); \
	  echo; echo "The order to T2 (the architect, 2026-11-04): the image cache (ADR-0212), then T2's missing"; \
	  echo "names above, then W4 measured for real (docs/backlog.md)."; \
	  } > $(W4_REPORT)
	@cat $(W4_REPORT)

# The stage 6 table from the logs there are (posix-stats table).
posix-table:
	$(CARGO) build -q -p posix-stats
	target/debug/posix-stats table $(wildcard $(POSIX_LOGS)/posix-*-auto.log $(POSIX_LOGS)/posix-*-tcg.log)

# The same session in the stage 5 terminal window (ttyd's window backend),
# read back by screendump as make test-gui reads zosh.
POSIX_GUI_SCRIPT := tests/posix/gui.script
test-posix-gui: $(POSIX_IMAGE_DEP) $(RUNNER)
	@for accel in $(POSIX_ACCELS); do \
	  target/debug/zoraya-mkfs --size $(POSIX_IMAGE_SIZE) --from $(POSIX_ROOT) $(BUILD)/posix-gui.img && \
	  $(RUNNER) --timeout $(QEMU_TIMEOUT) --gui-script $(POSIX_GUI_SCRIPT) --qmp $(BUILD)/posix-gui-$$accel.qmp \
	    --label posix-gui-$$accel -- $(RUN_QEMU) --test --gui --mem 512M --qmp $(BUILD)/posix-gui-$$accel.qmp \
	    --disk $(BUILD)/posix-gui.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(BUILD)/posix-gui.img || exit 1; done

# The tour in the window (INV-0154-4), on the tour's image: a step of its
# own, so that CI runs it as a job of its own (review K2).
# Then a terminal in a room runs the volume's POSIX programs (ADR-0172,
# the toolchain a trusted room's shell reads).
ROOM_GUI_SCRIPT := tests/posix/room-gui.script
test-tour-gui: $(POSIX_IMAGE_DEP) $(RUNNER)
	@for accel in $(POSIX_ACCELS); do \
	  for run in tour-gui:$(TOUR_GUI_SCRIPT):1200 room-gui:$(ROOM_GUI_SCRIPT):$(QEMU_TIMEOUT); do \
	  name=$$(echo $$run | cut -d: -f1); script=$$(echo $$run | cut -d: -f2); limit=$$(echo $$run | cut -d: -f3); \
	  target/debug/zoraya-mkfs --size $(POSIX_IMAGE_SIZE) --from $(POSIX_TOUR_ROOT) $(BUILD)/posix-gui.img > /dev/null && \
	  $(RUNNER) --timeout $$limit --gui-script $$script --qmp $(BUILD)/$$name-$$accel.qmp \
	    --label $$name-$$accel -- $(RUN_QEMU) --test --gui --mem $(POSIX_MEM) --qmp $(BUILD)/$$name-$$accel.qmp \
	    --disk $(BUILD)/posix-gui.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(BUILD)/posix-gui.img || exit 1; done; done

# The tour's two sessions, from docs/tour.md (INV-0154-4), and INV-0155-1's
# from the descriptions; make test-unit fails when the committed ones differ.
TOUR_GUI_SCRIPT := tests/posix/tour-gui.script
tour-scripts:
	$(CARGO) run -q -p zoraya-tour -- docs/tour.md tests/posix/tour.script $(TOUR_GUI_SCRIPT) $(CONVENTIONS_SCRIPT)

# The stage 5 criterion (TASK-14): the GUI headless, keys through QMP, the
# screen read back as text. Under the host's accelerator and under TCG.
GUI_IMAGE := $(BUILD)/gui.img
# 7b (ADR-0160, ADR-0164): the terminal's script and the windows' script,
# each under the host's accelerator and TCG at scale 1, and under the host's
# accelerator at scale 2 (2048 x 1536; the same scripts in logical pixels),
# with three times the time: every screendump read is four times larger.
# The terminal script also boots at 1600 x 1000 and 1920 x 1200 (ADR-0246: any
# size the driver takes; the compositor picks scale 1 and 2 from the width).
# A script's image is rootfs, with tests/gui/SCRIPT.root/ over it when there is
# one (the layouts' settings file, ADR-0169's gui.keyboard-layouts).
# The terminal also under TCG at scale 2, which is what CI's "auto" runs
# (it found term's 5 s wait for the compositor too short, 2026-11-09).
GUI_TIMEOUT_2 ?= 540
GUI_RUNS_ALL := terminal:auto:1 terminal:tcg:1 windows:auto:1 windows:tcg:1 layouts:auto:1 layouts:tcg:1 rooms:auto:1 rooms:tcg:1 files:auto:1 files:tcg:1 files-manage:auto:1 files-manage:tcg:1 files-manage:auto:2 settings:auto:1 settings:tcg:1 terminal:auto:2 terminal:tcg:2 windows:auto:2 terminal:auto:1600x1000 terminal:auto:1920x1200
GUI_RUNS ?= $(if $(filter 1,$(FULL)),$(GUI_RUNS_ALL),$(filter-out %:tcg:1 %:tcg:2,$(GUI_RUNS_ALL)))
test-gui: $(KERNEL) $(RUNNER) tools-storage
	@for run in $(GUI_RUNS); do \
	  script=$$(echo $$run | cut -d: -f1); accel=$$(echo $$run | cut -d: -f2); scale=$$(echo $$run | cut -d: -f3); \
	  label=gui-$$script-$$accel-$$scale; \
	  root=rootfs; if [ -d tests/gui/$$script.root ]; then \
	    rm -rf $(BUILD)/gui-root && mkdir -p $(BUILD)/gui-root && \
	    cp -R rootfs/. tests/gui/$$script.root/. $(BUILD)/gui-root/ && root=$(BUILD)/gui-root; fi; \
	  target/debug/zoraya-mkfs --size 16M --from $$root $(GUI_IMAGE) > /dev/null && \
	  timeout=$(QEMU_TIMEOUT); [ $$scale = 1 ] || timeout=$(GUI_TIMEOUT_2); \
	  case $$scale in *x*) size="--gui-size $$scale" ;; *) size="--gui-scale $$scale" ;; esac; \
	  $(RUNNER) --timeout $$timeout --gui-script tests/gui/$$script.script --qmp $(BUILD)/$$label.qmp --label $$label -- \
	    $(RUN_QEMU) --test --gui $$size --mem 512M --qmp $(BUILD)/$$label.qmp --disk $(GUI_IMAGE) --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(GUI_IMAGE) > /dev/null || exit 1; done

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
# Each mutant's tests get MUTANTS_TIMEOUT_MULTIPLIER times the unmutated
# baseline's test time, and at least MUTANTS_TIMEOUT_MIN seconds: a mutant
# that loops costs seconds, not a fixed 300 (TASK-18's pass spent 5 min on
# each of 17). MUTANTS_TIMEOUT=N sets N seconds instead, for a slow crate.
CRATE ?= zoraya-virtio
MUTANTS_TIMEOUT_MIN ?= 20
MUTANTS_TIMEOUT_MULTIPLIER ?= 5
MUTANTS_TIMEOUT ?=
MUTANTS_TIMEOUT_ARGS = $(if $(MUTANTS_TIMEOUT),--timeout $(MUTANTS_TIMEOUT),--minimum-test-timeout $(MUTANTS_TIMEOUT_MIN) --timeout-multiplier $(MUTANTS_TIMEOUT_MULTIPLIER))
mutants-host:
	@command -v cargo-mutants >/dev/null || { echo "cargo-mutants is not installed: cargo install cargo-mutants --version 27.1.0 --locked (docs/dependencies.md)"; exit 2; }
	@# cargo-mutants exits 2 when a mutant was missed and 3 on a timeout: results, not failures.
	@$(CARGO) build -q -p zoraya-footprint
	@mkdir -p build/mutants; tools/mutants/watchdog.sh $(CARGO) mutants -p $(CRATE) -j 2 $(MUTANTS_TIMEOUT_ARGS) $(MUTANTS_HOST_ARGS) -o build/mutants/cargo > build/mutants/cargo.log 2>&1; \
	  rc=$$?; tail -1 build/mutants/cargo.log; \
	  case $$rc in 0|2|3) echo "missed: build/mutants/cargo/mutants.out/missed.txt";; \
	  *) echo "cargo-mutants failed ($$rc), see build/mutants/cargo.log"; exit $$rc;; esac

# Stage 6b: the cross toolchain, the sysroot and the ported programs
# (ADR-0109, ports/Makefile). fetch-ports is the only step that downloads.
.PHONY: fetch-ports ports check-ports clean-ports check-fetch-ports check-test-caps check-tokens check-charged-alloc check-untrusted-gate check-ethd-maps
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
	@# The stats feature (ADR-0110 item 7), as ports/Makefile builds libc.a.
	$(CARGO) clippy -p zoraya-posix --target $(TARGET) --features stats -- -D warnings
	$(CARGO) clippy -p zoraya-rt --no-default-features --target $(TARGET) -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) --features qemu-test --target-dir target/qemu-test -- -D warnings

# ADR-0001 invariants (docs/adr/invariants.md).
adr-check:
	$(CARGO) run -q -p zoraya-adr-check -- docs

# INV-0031-4: only usercopy.rs may use the unprivileged load and store forms.
check-user-access:
	@! grep -rnE 'ldtr|sttr' kernel/src --include='*.rs' --exclude=usercopy.rs

# INV-0155-2: a program with a description (describe!) reads its arguments only
# through zoraya-command (user/src/command.rs); only test programs, which have
# none, may read argv themselves.
check-argv:
	@hits="$$(grep -l 'describe!(' user/src/bin/*.rs | xargs grep -n 'argv' || true)"; \
	if [ -n "$$hits" ]; then echo "described programs that read argv themselves (INV-0155-2):"; echo "$$hits"; exit 1; fi

# INV-0105-4: no terminal policy in the kernel; the console stays a raw ring
# (ADR-0060), and the line discipline is ttyd's.
check-kernel-tty:
	@! grep -rnE 'ICANON|ECHO|ISIG|VINTR|VQUIT|VSUSP|VERASE|VKILL|VEOF|ONLCR|TOSTOP|termios|winsize' kernel/src

# No host paths or email addresses in tracked files (CLAUDE.md, Conventions), apart from
# the configured git identity's address. docs/research/ and tools/posix-surface/results/
# are cleaned separately. The brackets keep the pattern from matching this file.
# Host paths: macOS's users' folders, and this host's own home directory (on Linux
# /home/NAME); not every /home/, which is Zoraya's own home folder
# (shell.home, /data/home/APP).
check-host-paths:
	@id="$$(git config user.email)"; \
	home="$$(echo "$$HOME" | sed 's/[.[\*^$$]/\\&/g')"; \
	hits="$$(git grep -n -I -E "/Use[r]s/|$${home:-/nonexistent}/|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+[.][A-Za-z]{2,}" -- . \
	  ':!docs/research/' ':!tools/posix-surface/results/' | { if [ -n "$$id" ]; then grep -vF "$$id"; else cat; fi; })"; \
	if [ -n "$$hits" ]; then echo "host paths or email addresses in tracked files:"; echo "$$hits"; exit 1; fi

# Every host-tested crate caps its test heap (CLAUDE.md, Memory limits): the
# library, if it has tests, and every top-level file of tests/, by the cap or
# by a counting allocator over `Capped`. An integration test binary does not
# get the library's cap: TASK-20's mutants found that (docs/backlog.md).
# INV-0261-5: kernel code reaches the heap's uncharged forms only where there is
# no paying job; every other allocation goes through crate::charge.
check-charged-alloc:
	tools/check-charged-alloc.sh

# INV-0268-1, INV-0268-2: the refusal of untrusted rooms and its gate list.
check-untrusted-gate:
	tools/check-untrusted-gate.sh

# INV-0280-1: ethd maps only its queue memory and takes no view of a frame slot.
check-ethd-maps:
	tools/check-ethd-maps.sh

check-test-caps:
	@bad=""; \
	for f in crates/*/tests/*.rs; do \
	  grep -q -e 'cap_memory!' -e 'Capped::new' "$$f" || bad="$$bad $$f"; done; \
	for d in crates/*; do \
	  if grep -rqs -e '#\[test\]' "$$d/src"; then \
	    grep -rqs -e 'cap_memory!' -e 'Capped::new' "$$d/src" || bad="$$bad $$d/src"; fi; done; \
	if [ -n "$$bad" ]; then echo "no heap cap in:$$bad"; exit 1; fi

# INV-0161-1: no colour or duration literal in a GUI crate; they come from
# zoraya-tokens. The crates ADR-0161 names that exist (the compositor, the
# terminal, the GUI programs), tests excepted. A colour is a hexadecimal
# literal of six or eight digits once the separators are gone (0xrrggbb,
# 0x00rrggbb, 0xaarrggbb), with or without an integer suffix; a duration is
# any Duration::from_*. A line that is a size or a mask, not a colour, says
# so with `// not a colour` (review finding 6).
GUI_SOURCES := crates/zoraya-compositor/src crates/zoraya-term/src crates/zoraya-ui/src crates/zoraya-app/src crates/zoraya-panel/src user/src/bin/compd.rs user/src/bin/term.rs user/src/bin/panel.rs
check-tokens:
	@hits="$$(grep -rnE '0x[0-9a-fA-F_]+|Duration::from_' $(GUI_SOURCES) | grep -vE '/tests(_[a-z_]+)?\.rs:' | grep -v 'not a colour' | \
	  perl -ne 'print if /Duration::from_/ || grep { (my $$d = $$_) =~ s/_//g; length($$d) == 6 || length($$d) == 8 } /0x([0-9a-fA-F_]+)(?:[ui](?:8|16|32|64|size))?\b/g' || true)"; \
	if [ -n "$$hits" ]; then echo "colour or duration literals outside zoraya-tokens (INV-0161-1):"; echo "$$hits"; exit 1; fi
	@# INV-0240-1: fonts are embedded only through zoraya-fonts.
	@hits="$$(grep -rn 'include_bytes!(.*\.ttf' crates tools user | grep -v '^crates/zoraya-fonts/' | grep -v '/tests\.rs:' || true)"; \
	if [ -n "$$hits" ]; then echo "fonts embedded outside zoraya-fonts (INV-0240-1):"; echo "$$hits"; exit 1; fi

# `make test` prints a summary and keeps each step's output in build/logs/;
# `make test V=1` prints everything as it runs.
TEST_STEPS := fmt-check clippy test-unit adr-check check-user-access check-kernel-tty check-host-paths check-argv check-test-caps check-tokens check-fetch-ports test-qemu test-boot test-files test-libposix test-posix test-posix-gui test-tour-gui test-crash test-gui test-net
TEST_STEPS += check-ci-jobs check-charged-alloc check-untrusted-gate check-ethd-maps

# CI (ADR-0213's Decision): one job builds what every test needs, and every
# step of TEST_STEPS is a job of its own, test-qemu one a configuration and
# test-posix one a script (and an accelerator where needed). ci-build is the
# build job's; check-ci-jobs fails if the workflow leaves out a step, a
# configuration, a script under an accelerator, or a W8 build.
ci-build: $(KERNEL) $(TEST_KERNEL) $(RUNNER) tools-storage
	@# Not -s: check-ports reads the compile commands in the ports' logs.
	$(MAKE) -f ports/Makefile ports workloads
	@$(MAKE) -s posix-image
	$(CARGO) test --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt --no-run
	$(CARGO) build -q -p zoraya-test-runner -p zoraya-elf-check -p posix-stats -p zoraya-footprint

CI_WORKFLOW ?= .github/workflows/test.yml
CI_TESTS ?= tools/ci-tests.txt
check-ci-jobs:
	@TEST_STEPS="$(TEST_STEPS)" QEMU_CONFIGS="$(QEMU_CONFIGS)" POSIX_SCRIPTS="$(POSIX_SCRIPTS)" \
	  POSIX_ACCELS="auto tcg" POSIX_ONCE="$(POSIX_ONCE)" W8_BUILDS="$(POSIX_W8_BUILDS)" GUI_RUNS_ALL="$(GUI_RUNS_ALL)" \
	  tools/check-ci-jobs.sh $(CI_WORKFLOW) $(CI_TESTS)

# The tiers (CLAUDE.md, "Testing"): make check on every commit (seconds);
# make test-changed for what a change touches (tools/test-map.txt, under
# HVF); make test-par for everything in parallel lanes (tools/test-par.sh,
# at most 3 QEMU guests); make test for everything one step after another.
CHECK_STEPS := fmt-check clippy adr-check check-user-access check-kernel-tty check-host-paths check-argv check-test-caps check-tokens check-fetch-ports check-ci-jobs check-charged-alloc check-untrusted-gate check-ethd-maps
check:
	@MAKE="$(MAKE)" tools/test-summary.sh $(CHECK_STEPS)

test-changed:
	@MAKE="$(MAKE)" CARGO="$(CARGO)" tools/test-changed.sh

test-par:
	@MAKE="$(MAKE)" CARGO="$(CARGO)" FULL="$(FULL)" tools/test-par.sh

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
