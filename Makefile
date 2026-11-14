# Zoraya build and test entry points. See docs/design.md, "Where things are".
#
#   make build      build the kernel (build/zoraya-kernel.elf)
#   make run        boot it interactively (Ctrl-A X quits QEMU), no network
#   make run-net    the same with the network (Internet through the host)
#   make run-gui    the same with a GPU, keyboard and tablet, in a window, with
#                   the network (NET=0 make run-gui: without it)
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
# make test-net-fetch's image (below).
NET_TEST_USER_DIR := target/user-net-test/$(TARGET)/user
NET_TEST_RAMDISK := $(BUILD)/ramdisk-net-test.bin
NET_TEST_KERNEL := $(BUILD)/zoraya-kernel-net-test.elf
RAMDISK := $(BUILD)/ramdisk.bin
DISK := $(BUILD)/disk.img
TEST_DISK := $(BUILD)/test-disk.img
USER_DIR := target/user/$(TARGET)/user
USER_PROGRAMS := hello spin isolate badptr fuzz regs f_kread f_kwrite f_null f_priv \
	f_undef f_exdata f_stack f_brk f_wfi f_align regsave lone koverflow copyfar \
	parent child counter portclose t_handles t_spawn t_signals t_ports portbench t_stress t_leak \
	busy zosh echo-args sleep exit-with fault fanout flood ticker handles stubborn requests probe t_console t_memory t_channels init t_devices t_loop chanbench blkd fsd \
	crashwriter crashcheck fsbench t_storage gpud inputd compd term panel chooser t_open files settings edit t_gui guibench t_text \
	ethd netd net fetch ping t_net t_netd t_streams t_snapshot t_image t_suspend t_quota t_thread t_bound t_cpu posixrun ls cat cp mv rm mkdir stat head find search date rngd rooms t_rooms memhog diskfill random broker ttyd
RUNNER := target/debug/zoraya-test-runner
RUN_QEMU := tools/run-qemu.sh
QEMU_TIMEOUT ?= 180
# The deadline of the TCG, 4-CPU and CI test-qemu-* and test-net runs: 116 tests
# took 147-176 s under TCG in CI; with 128 tests (stage 9b) the runs took 168,
# 201 and 267 s, and one run on a slow runner (every timed test 2x slower) did
# not finish in 300 s. 420 s keeps a 57% margin over the slowest green run.
# The CI job limit (10 minutes) still holds: about 1 minute of setup + 7.
# HVF-only runs keep QEMU_TIMEOUT.
QEMU_SLOW_TIMEOUT ?= 420

KERNEL_ARGS := -p zoraya-kernel --target $(TARGET)

.PHONY: FORCE apps netsurf test-netsurf test-apps python python-testlib test-python test-python-regr test-python-long test-hand-run test-keeper test-fuzz test-fuzz-net all build run run-gui run-net test-net test-net-fetch test-net-lookup bench-net test-net-posix debug check test-changed test-par test test-unit test-qemu test-qemu-one test-qemu-disk ci-build check-ci-jobs $(addprefix test-qemu-,$(QEMU_CONFIGS)) test-boot test-files test-libposix test-posix test-posix-long posix-calls posix-table measure-w4 test-posix-gui test-tour-gui posix-image tour-scripts test-crash test-gui mutants mutants-host tools-storage ramdisk fmt-check check-user-access check-kernel-tty check-argv check-host-paths clippy adr-check clean test-posix-threads test-posix-threadtest3 posix-threadtest3 \
	$(KERNEL) $(TEST_KERNEL) $(RUNNER) $(RAMDISK) $(NET_TEST_KERNEL) $(NET_TEST_RAMDISK)

all: build

build: $(KERNEL)

# User programs go into the ramdisk, and the ramdisk into the kernel image
# (ADR-0033). They have their own target directory and a stripped profile.
$(RAMDISK):
	@# ring's build script archives its C objects with the toolchain's ar
	@# (.cargo/config.toml), which must exist first (ADR-0289).
	$(CARGO) build -q -p zoraya-ar
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

# The image of `make test-net-fetch` alone (ADR-0296 item 5): the user
# programs with the feature `net-test`, which builds the test of the
# network's restart into `init`, `netd` and `zosh`, and a kernel that embeds
# them, each in a target directory of its own. Every other kernel embeds
# $(RAMDISK), which has no such test, whatever its volume holds.
$(NET_TEST_RAMDISK):
	$(CARGO) build -q -p zoraya-ar
	$(CARGO) build -p zoraya-user --target $(TARGET) --profile user --target-dir target/user-net-test --features net-test
	$(CARGO) build -p zoraya-mkramdisk
	@mkdir -p $(BUILD)
	target/debug/zoraya-mkramdisk --describe user/describe $@ $(addprefix $(NET_TEST_USER_DIR)/,$(USER_PROGRAMS))

$(NET_TEST_KERNEL): $(NET_TEST_RAMDISK)
	ZORAYA_RAMDISK=$(abspath $(NET_TEST_RAMDISK)) $(CARGO) build $(KERNEL_ARGS) --target-dir target/net-test
	@mkdir -p $(BUILD)
	cp target/net-test/$(TARGET)/debug/zoraya-kernel $@

$(RUNNER):
	$(CARGO) build -p zoraya-test-runner

# The default disk: made from rootfs/ and kept, so that files stay, while it
# is a volume of this system: tools/run-disk.sh makes a new one (and keeps the
# old as build/disk.img.old) when the format or rootfs/system changed.
$(DISK): tools-storage FORCE
	@tools/run-disk.sh $@

FORCE:

# The machines a person runs by hand, one definition each, which
# `make test-hand-run` boots as they are (the image $(KERNEL), a disk made by
# tools/run-disk.sh, this command line): $(1) is the disk; RUN_GUI_CMD's $(2)
# is empty for a person and `--display-opts none` for the test, which has no
# window. `make run`: the serial console, no network. `make run-net`: the serial
# console and the network. `make run-gui`: the window and the network (the GUI
# and the network work together since N6); `NET=0 make run-gui` leaves the
# network out: only NET=0 turns it off, any other value (or none) leaves it on.
# The test boots RUN_GUI_CMD as it is, so with the network.
NET ?= 1
RUN_CMD = $(RUN_QEMU) --disk $(1) $(KERNEL)
# 512 MiB for the working set, not for a leak (the 8.5 MiB leak per `curl` launch is
# fixed): the programs of the volume are large (python3 15.5 MB, NetSurf 12.5 MB, curl
# and wget 6.7 MiB each), a Python thread's stack is committed in full and `os.fork`
# copies the writable space (11 children at once on 512 MiB at 8 MiB a stack), the GUI
# surfaces are 1024 x 768 a client, and 1/16 of memory is the charge reserve (the GUI
# test on 256 MiB refused a menu). No smaller size has been measured (docs/backlog.md,
# "Memory of the run machines").
RUN_NET_CMD = $(RUN_QEMU) --net --mem 512M --disk $(1) $(KERNEL)
RUN_GUI_CMD = $(RUN_QEMU) --gui $(if $(filter 0,$(NET)),,--net) --gui-scale $(GUI_SCALE) --mem 512M --disk $(1) $(2) $(KERNEL)

run: $(KERNEL) $(DISK)
	$(call RUN_CMD,$(DISK))

# The GUI (ADR-0097): a window on the host; the serial line stays here. The
# network is on by default (`--net`, as `make run-net`): NET=0 starts without.
# On macOS the window is on a Retina screen as a rule: twice the pixels,
# scale 2 (the architect's check of 2026-11-10); GUI_SCALE=1 gives 1024x768.
GUI_SCALE ?= $(if $(filter Darwin,$(shell uname -s)),2,1)
run-gui: $(KERNEL) $(DISK)
	$(call RUN_GUI_CMD,$(DISK),)

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
RUN_TEST_SLOW = $(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) $(EXPECT)

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
	$(RUN_TEST_SLOW) --label auto-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "cpus: 1" \
		--expect-line "memory: 131072 KiB total" \
		--expect-line "console: pl011 at 0x9000000 (/pl011@9000000)" \
		--expect-line "t_gui: all passed" \
		-- $(RUN_QEMU) --test --gui --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-auto-smp4-2g: test-qemu-disk
	$(RUN_TEST_SLOW) --label auto-smp4-2g \
		--expect-line "cpus: 4" --expect-line "2048 MiB  RAM" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --smp 4 --mem 2G $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-auto-el2: test-qemu-disk
	$(call EL2_OR_SKIP,auto-el2) $(RUN_TEST_SLOW) --label auto-el2 \
		--expect-line "exception level at entry: EL2, now EL1" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto --machine-opt virtualization=on $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-default: test-qemu-disk
	$(RUN_TEST_SLOW) --label tcg-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "(GICv2)" \
		--expect-line "gic: version 2" --expect-line "t_gui: all passed" \
		-- $(RUN_QEMU) --test --gui --disk $(TEST_DISK) --accel tcg $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-smp4-gicv2: test-qemu-disk
	$(RUN_TEST_SLOW) --label tcg-smp4-gicv2 \
		--expect-line "cpus: 4" --expect-line "(GICv2)" --expect-line "gic: version 2" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --smp 4 $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-el2: test-qemu-disk
	$(RUN_TEST_SLOW) --label tcg-el2 \
		--expect-line "exception level at entry: EL2, now EL1" --expect-line "psci: arm,psci-1.0 via smc" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --machine-opt virtualization=on $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-tcg-smp4-gicv3: test-qemu-disk
	$(RUN_TEST_SLOW) --label tcg-smp4-gicv3 \
		--expect-line "cpus: 4" --expect-line "(GICv3)" \
		--expect-line "gic: version 3" \
		-- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel tcg --smp 4 --mem 2G --machine-opt gic-version=3 $(TEST_KERNEL)
	target/debug/zoraya-fsck $(TEST_DISK)

test-qemu-one: $(TEST_KERNEL) $(RUNNER) tools-storage
	target/debug/zoraya-mkfs --size 16M --from rootfs $(TEST_DISK)
	$(RUN_TEST_SLOW) --label auto-default -- $(RUN_QEMU) --test --disk $(TEST_DISK) --accel auto $(TEST_KERNEL)
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
NET_SCRIPT := tests/shell/net.script
# INV-0296-1: a string of each of the test image's Crash paths (feature
# net-test), as PROGRAM:STRING: netd's end on request, init's request on its
# control connection and its look for the volume's file, and netd's read of
# the test DNS port (ADR-0316's test hook). The shell's `net
# crash` only asks init, which then has no such path. test-net checks that
# the image's programs hold none; test-net-fetch that its own hold each, so
# that a string changed in the source does not make the check pass blind.
NET_CRASH_MARKS := 'netd:netd: ended on request' 'init:init: the shell asked the network service to end' 'init:etc/zoraya/net-crash' 'init:init: the shell asked the room keeper to end' 'zosh:no room keeper to end' 'init:init: the shell reconnected to the room keeper' 'netd:net-test-dns-port'
test-net: $(TEST_KERNEL) $(KERNEL) $(RUNNER) tools-storage
	@for m in $(NET_CRASH_MARKS); do f=$${m%%:*}; s=$${m#*:}; \
		if [ ! -f $(USER_DIR)/$$f ]; then echo "[net] FAIL: INV-0296-1: no $(USER_DIR)/$$f to look in"; exit 1; fi; \
		if grep -a -q "$$s" $(USER_DIR)/$$f; then echo "[net] FAIL: INV-0296-1: the image's $$f holds '$$s'"; exit 1; fi; done
	@$(RUN_QEMU) --probe-net; rc=$$?; \
	if [ $$rc = 1 ]; then \
		if [ -n "$$CI" ]; then echo "[net] FAIL: no user-mode networking on a CI runner"; exit 1; fi; \
		echo "[net] skipped: QEMU has no user-mode networking"; exit 0; \
	elif [ $$rc != 0 ]; then echo "[net] FAIL: the networking probe failed (exit $$rc)"; exit 1; fi; \
	mkdir -p target/test-logs && rm -f $(NET_PCAP) && \
	target/debug/zoraya-mkfs --size 16M --from rootfs $(NET_DISK) && \
	run() { accel=$$1; shift; tools/net-poke.sh $(NET_FWD_PORT) 240 & poke=$$!; \
		$(RUN_TEST_SLOW) "$$@" -- $(RUN_QEMU) --test --net --net-forward $(NET_FWD_PORT) --net-pcap $(NET_PCAP) --disk $(NET_DISK) --accel $$accel $(TEST_KERNEL); \
		rc=$$?; kill $$poke 2>/dev/null; wait $$poke 2>/dev/null; return $$rc; }; \
	run auto --label net-auto --expect-line "t_net: all passed" --expect-line "t_net: arp reply from 10.0.2.2" \
		--expect-line "t_netd: all passed" --expect-line "t_netd: lease 10.0.2.15/24 gateway" \
		--expect-line "netd: holds no device, interrupt or pin" --expect-line "ethd: holds one channel, to the network service" && \
	target/debug/zoraya-fsck $(NET_DISK) && \
	hex="$$(od -An -tx1 -v $(NET_PCAP) | tr -d ' \n')" && \
	( case "$$hex" in *08060001080006040002525[0-9a-f]0a000202*) ;; \
		*) echo "[net-auto] FAIL: no ARP reply from the gateway in $(NET_PCAP)"; exit 1 ;; esac ) && \
	( case "$$hex" in *080600010800060400025254001234560a00020f*) ;; \
		*) echo "[net-auto] FAIL: the guest answered no ARP request for 10.0.2.15 in $(NET_PCAP)"; exit 1 ;; esac ) && \
	( case "$$hex" in *00440043*) ;; \
		*) echo "[net-auto] FAIL: no DHCP request from port 68 to 67 in $(NET_PCAP)"; exit 1 ;; esac ) && \
	( case "$$hex" in *00430044*) ;; \
		*) echo "[net-auto] FAIL: no DHCP reply from port 67 to 68 in $(NET_PCAP)"; exit 1 ;; esac ) && \
	( case "$$hex" in *080045????????????????01????0a00020f0a0002020800*) ;; \
		*) echo "[net-auto] FAIL: no ICMP echo request from 10.0.2.15 to 10.0.2.2 in $(NET_PCAP)"; exit 1 ;; esac ) && \
	( case "$$hex" in *080045????????????????01????0a0002020a00020f0000*) ;; \
		*) echo "[net-auto] FAIL: no ICMP echo reply from 10.0.2.2 in $(NET_PCAP)"; exit 1 ;; esac ) && \
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(NET_SCRIPT) --label net-boot-auto -- $(RUN_QEMU) --test --keys --net --accel auto $(KERNEL) && \
	target/debug/zoraya-mkfs --size 16M --from rootfs $(NET_DISK) && \
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script tests/shell/net-settings.script --label net-settings-auto -- $(RUN_QEMU) --test --keys --net --disk $(NET_DISK) --accel auto $(KERNEL) && \
	target/debug/zoraya-fsck $(NET_DISK) && \
	( [ "$(FULL)" != 1 ] || $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(NET_SCRIPT) --label net-boot-tcg -- $(RUN_QEMU) --test --keys --net --accel tcg $(KERNEL) ) && \
	( [ "$(FULL)" != 1 ] || run tcg --label net-tcg --expect-line "t_net: all passed" --expect-line "t_netd: all passed" ) && \
	$(MAKE) --no-print-directory test-net-fetch && \
	$(MAKE) --no-print-directory test-net-posix && \
	$(MAKE) --no-print-directory test-net-lookup

# N3 (ADR-0222, ADR-0223): `fetch` in the guest against the host-side test
# server (tools/test-httpd: 127.0.0.1 only, bounded, gone after 300 s or when
# this recipe ends). The guest reaches it at 10.0.2.2 through SLIRP, which
# needs the network without `restrict=on` (`--net-open`, tools/run-qemu.sh);
# nothing else is reached. The disk's /config/settings gives `test.zoraya`
# (net.hosts), so that a name is resolved too; the script's checksums come
# from the server's own pattern function (`--checksum`).
HTTPD := target/debug/zoraya-test-httpd
NET_HTTP_READY := $(BUILD)/net-httpd.port
NET_TLS_READY := $(BUILD)/net-tlsd.ports
NET_DNS_SCRIPT := tests/shell/net-dns.script
NET_FETCH_ROOT := $(BUILD)/net-fetch-root
NET_FETCH_DISK := $(BUILD)/net-fetch-disk.img
NET_FETCH_TEMPLATE := tests/shell/net-fetch.script
NET_FETCH_SCRIPT := $(BUILD)/net-fetch.script
NET_FETCH_RUN = $(RUNNER) --timeout $(1) --script $(NET_FETCH_SCRIPT) --label $(2) -- $(RUN_QEMU) --test --keys --net --net-open --disk $(NET_FETCH_DISK) --accel $(3) $(NET_TEST_KERNEL)

$(HTTPD): tools/test-httpd/src/main.rs
	$(CARGO) build -q -p zoraya-test-httpd

test-net-fetch: $(NET_TEST_KERNEL) $(RUNNER) $(HTTPD) tools-storage
	@for m in $(NET_CRASH_MARKS); do f=$${m%%:*}; s=$${m#*:}; \
		grep -a -q "$$s" $(NET_TEST_USER_DIR)/$$f || { echo "[net-fetch] FAIL: INV-0296-1's check is blind: $$f holds no '$$s'"; exit 1; }; done
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[net-fetch] skipped: QEMU has no user-mode networking"; exit 0; }; \
	mkdir -p $(NET_FETCH_ROOT) target/test-logs && cp -R rootfs/. $(NET_FETCH_ROOT)/ && \
	printf 'net.hosts = test.zoraya=10.0.2.2 expired.zoraya=10.0.2.2 wrong.zoraya=10.0.2.2\n' > $(NET_FETCH_ROOT)/config/settings && \
	cp tests/net/tls/root.pem $(NET_FETCH_ROOT)/system/tls/roots && \
	mkdir -p $(NET_FETCH_ROOT)/etc/zoraya && : > $(NET_FETCH_ROOT)/etc/zoraya/net-crash && \
	target/debug/zoraya-mkfs --size 16M --from $(NET_FETCH_ROOT) $(NET_FETCH_DISK) && \
	: > $(NET_HTTP_READY) && : > $(NET_TLS_READY) && \
	{ $(HTTPD) --port 0 --lifetime 300 --ready-file $(NET_HTTP_READY) --tls-dir tests/net/tls --tls-ready-file $(NET_TLS_READY) > target/test-logs/test-httpd.log 2>&1 & echo $$! > $(BUILD)/net-httpd.pid; } && \
	httpd=$$(cat $(BUILD)/net-httpd.pid); \
	for i in $$(seq 1 20); do [ -s $(NET_HTTP_READY) ] && [ -s $(NET_TLS_READY) ] && break; sleep 0.25; done; \
	set -- $$(cat $(NET_TLS_READY)); tlsvalid=$$1; tlsexpired=$$2; tlswrong=$$3; \
	port=$$(cat $(NET_HTTP_READY)); \
	if [ -z "$$port" ] || ! kill -0 $$httpd 2>/dev/null; then \
		echo "[net-fetch] FAIL: the test server did not start"; kill $$httpd 2>/dev/null; cat target/test-logs/test-httpd.log; exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" -e "s/@TLSVALID@/$$tlsvalid/g" -e "s/@TLSEXPIRED@/$$tlsexpired/g" -e "s/@TLSWRONG@/$$tlswrong/g" \
		-e "s/@SUM1M@/$$($(HTTPD) --checksum 1000000)/" \
		-e "s/@SUM300K@/$$($(HTTPD) --checksum 300000)/" \
		-e "s/@SUM70K@/$$($(HTTPD) --checksum 70000)/" \
		-e "s/@SUM4M@/$$($(HTTPD) --checksum 4000000)/" \
		$(NET_FETCH_TEMPLATE) > $(NET_FETCH_SCRIPT); \
	$(call NET_FETCH_RUN,$(QEMU_TIMEOUT),net-fetch-auto,auto); rc=$$?; \
	if [ $$rc = 0 ] && [ "$(FULL)" = 1 ]; then $(call NET_FETCH_RUN,400,net-fetch-tcg,tcg); rc=$$?; fi; \
	kill $$httpd 2>/dev/null; wait $$httpd 2>/dev/null; \
	if [ $$rc = 0 ]; then \
		if grep -q '^tls \(expired\|wrong\): request of' target/test-logs/test-httpd.log || ! grep -q '^tls valid: request of' target/test-logs/test-httpd.log \
		   || ! grep -q '^tls expired: ended after 0 request bytes' target/test-logs/test-httpd.log || ! grep -q '^tls wrong: ended after 0 request bytes' target/test-logs/test-httpd.log \
		   || grep '^tls \(expired\|wrong\): ended after' target/test-logs/test-httpd.log | grep -qv 'ended after 0 request bytes'; then \
			echo '[net-fetch] FAIL: INV-0228-1: the server saw request bytes after a refused handshake (or no clean end for it), or no request on the valid one'; cat target/test-logs/test-httpd.log; rc=1; \
		else echo '[net-fetch] ok: the refused TLS servers read no request byte'; fi; \
	fi; \
	if [ $$rc = 0 ]; then target/debug/zoraya-fsck $(NET_FETCH_DISK); rc=$$?; fi; \
	if [ $$rc = 0 ]; then \
		if [ -n "$$CI" ] && [ "$(NET_DNS)" != 1 ]; then echo "[net-dns] SKIP: SLIRP's DNS forwarder needs the host's resolver (NET_DNS=1 runs it on CI)"; \
		else $(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(NET_DNS_SCRIPT) --label net-dns-auto -- $(RUN_QEMU) --test --keys --net --net-open --disk $(NET_FETCH_DISK) --accel auto $(KERNEL); rc=$$?; fi; \
	fi; \
	exit $$rc

# The first lookup of a boot over the wire (proposed ADR-0316): a fresh boot
# whose first network command is `ping NAME`, then `fetch` by name, against
# tools/test-dns on the host's 127.0.0.1 (the disk's `net.dns = 10.0.2.2`,
# which SLIRP maps there), which answers late or never. The script checks
# what the commands say (the answer, or "the name lookup timed out" at
# netd's bound); `zoraya-test-dns --check-pcap` checks the capture: one ARP
# request for the server before the first query, and the echo request and
# the SYN leaving within 0.5 s of their answers (netd acted on the answer
# at once; before the fix the run waited for the client's timeout). The
# server listens on a free port, not 53 (root only): the image is the test
# image, whose `netd` reads that port from /config/net-test-dns-port.
TEST_DNS := target/debug/zoraya-test-dns
NET_LOOKUP_ROOT := $(BUILD)/net-lookup-root
NET_LOOKUP_DISK := $(BUILD)/net-lookup-disk.img
NET_LOOKUP_SCRIPT := $(BUILD)/net-lookup.script
NET_LOOKUP_PCAP := target/test-logs/test-net-lookup.pcap
NET_LOOKUP_DNS_READY := $(BUILD)/net-lookup-dns.port
NET_LOOKUP_HTTP_READY := $(BUILD)/net-lookup-httpd.port
NET_LOOKUP_DNS_LOG := target/test-logs/test-dns.log

$(TEST_DNS): tools/test-dns/src/main.rs
	$(CARGO) build -q -p zoraya-test-dns

test-net-lookup: $(NET_TEST_KERNEL) $(RUNNER) $(HTTPD) $(TEST_DNS) tools-storage
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[net-lookup] skipped: QEMU has no user-mode networking"; exit 0; }; \
	mkdir -p $(NET_LOOKUP_ROOT) target/test-logs && cp -R rootfs/. $(NET_LOOKUP_ROOT)/ && \
	rm -f $(NET_LOOKUP_PCAP) && : > $(NET_LOOKUP_DNS_READY) && : > $(NET_LOOKUP_HTTP_READY) && \
	{ $(TEST_DNS) --port 0 --lifetime 300 --ready-file $(NET_LOOKUP_DNS_READY) \
		--answer slow.zoraya=10.0.2.2@3000 --answer slowweb.zoraya=10.0.2.2@3000 \
		--silent silent.zoraya --silent silentweb.zoraya > $(NET_LOOKUP_DNS_LOG) 2>&1 & echo $$! > $(BUILD)/net-lookup-dns.pid; } && \
	{ $(HTTPD) --port 0 --lifetime 300 --ready-file $(NET_LOOKUP_HTTP_READY) > target/test-logs/test-httpd-lookup.log 2>&1 & echo $$! > $(BUILD)/net-lookup-httpd.pid; } && \
	dns=$$(cat $(BUILD)/net-lookup-dns.pid); httpd=$$(cat $(BUILD)/net-lookup-httpd.pid); \
	for i in $$(seq 1 20); do [ -s $(NET_LOOKUP_DNS_READY) ] && [ -s $(NET_LOOKUP_HTTP_READY) ] && break; sleep 0.25; done; \
	dnsport=$$(cat $(NET_LOOKUP_DNS_READY)); port=$$(cat $(NET_LOOKUP_HTTP_READY)); \
	if [ -z "$$dnsport" ] || [ -z "$$port" ] || ! kill -0 $$dns 2>/dev/null || ! kill -0 $$httpd 2>/dev/null; then \
		echo "[net-lookup] FAIL: the test servers did not start"; cat $(NET_LOOKUP_DNS_LOG); kill $$httpd $$dns 2>/dev/null; exit 1; fi; \
	if ! { printf 'net.dns = 10.0.2.2\n' > $(NET_LOOKUP_ROOT)/config/settings && \
		echo "$$dnsport" > $(NET_LOOKUP_ROOT)/config/net-test-dns-port && \
		target/debug/zoraya-mkfs --size 16M --from $(NET_LOOKUP_ROOT) $(NET_LOOKUP_DISK); }; then \
		echo "[net-lookup] FAIL: the disk was not made"; kill $$httpd $$dns 2>/dev/null; exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" -e "s/@SUM1K@/$$($(HTTPD) --checksum 1000)/" tests/shell/net-lookup.script > $(NET_LOOKUP_SCRIPT); \
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(NET_LOOKUP_SCRIPT) --label net-lookup-auto -- \
		$(RUN_QEMU) --test --keys --net --net-open --net-pcap $(NET_LOOKUP_PCAP) --disk $(NET_LOOKUP_DISK) --accel auto $(NET_TEST_KERNEL); rc=$$?; \
	kill $$httpd $$dns 2>/dev/null; wait $$httpd $$dns 2>/dev/null; \
	if [ $$rc = 0 ]; then \
		$(TEST_DNS) --check-pcap $(NET_LOOKUP_PCAP) --server 10.0.2.2 --server-port $$dnsport --first slow.zoraya \
			--then-echo slow.zoraya --then-syn slowweb.zoraya; rc=$$?; \
		[ $$rc = 0 ] && echo "[net-lookup] ok: the capture shows the first lookup and what followed it at once"; \
	fi; \
	if [ $$rc != 0 ]; then echo "[net-lookup] FAIL (the DNS server's log: $(NET_LOOKUP_DNS_LOG))"; tail -20 $(NET_LOOKUP_DNS_LOG); fi; \
	exit $$rc

# N6 (ADR-0230 item 8): the throughput and latency numbers of the network, from
# `fetch --sum` against the test server: 16 MiB and one byte, five times each,
# under HVF (`FULL=1` adds TCG). Prints the times of every fetch; nothing is
# asserted but that every fetch ends. The numbers go to docs/design.md by hand.
NET_BENCH_SCRIPT := $(BUILD)/net-bench.script
bench-net: $(KERNEL) $(RUNNER) $(HTTPD) tools-storage
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[bench-net] skipped: QEMU has no user-mode networking"; exit 0; }; \
	mkdir -p $(NET_FETCH_ROOT) target/test-logs && cp -R rootfs/. $(NET_FETCH_ROOT)/ && \
	printf 'net.hosts = test.zoraya=10.0.2.2\n' > $(NET_FETCH_ROOT)/config/settings && \
	target/debug/zoraya-mkfs --size 16M --from $(NET_FETCH_ROOT) $(NET_FETCH_DISK) && \
	: > $(NET_HTTP_READY) && \
	{ $(HTTPD) --port 0 --lifetime 300 --ready-file $(NET_HTTP_READY) > target/test-logs/test-httpd-bench.log 2>&1 & echo $$! > $(BUILD)/net-httpd-bench.pid; } && \
	httpd=$$(cat $(BUILD)/net-httpd-bench.pid); \
	for i in $$(seq 1 20); do [ -s $(NET_HTTP_READY) ] && break; sleep 0.25; done; \
	port=$$(cat $(NET_HTTP_READY)); \
	if [ -z "$$port" ] || ! kill -0 $$httpd 2>/dev/null; then echo "[bench-net] FAIL: the test server did not start"; exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" tests/shell/net-bench.script > $(NET_BENCH_SCRIPT); \
	for accel in auto $(if $(filter 1,$(FULL)),tcg); do \
	  log=target/test-logs/bench-net-$$accel.log; \
	  $(RUNNER) --timeout $(if $(filter 1,$(FULL)),400,$(QEMU_TIMEOUT)) --script $(NET_BENCH_SCRIPT) --label bench-net-$$accel -- \
	    $(RUN_QEMU) --test --keys --net --net-open --disk $(NET_FETCH_DISK) --accel $$accel $(KERNEL) > $$log 2>&1; rc=$$?; \
	  if [ $$rc != 0 ]; then echo "[bench-net] FAIL ($$accel), log $$log"; tail -20 $$log; kill $$httpd 2>/dev/null; exit $$rc; fi; \
	  for size in 16777216 1; do \
	    echo "[bench-net] $$accel $$size bytes, ms:" $$(grep -a "^$$size bytes fnv1a" $$log | sed -e 's/.* in \([0-9.]*\) ms.*/\1/' | tr '\n' ' '); \
	  done; \
	done; \
	kill $$httpd 2>/dev/null; wait $$httpd 2>/dev/null; exit 0

# N4 (ADR-0225, ADR-0287): `zfetch`, a C program built by zcc against libc.a,
# fetches the same pages through libposix's sockets, started by the shell from
# the disk's /bin through the broker, with the connection the shell narrows to
# the URL's host and port. Needs libc.a (musl's source in the cache of `make
# fetch-ports`, as test-libposix does). The test server and the guest's network
# are test-net-fetch's.
NET_POSIX_ROOT := $(BUILD)/net-posix-root
NET_POSIX_DISK := $(BUILD)/net-posix-disk.img
NET_POSIX_TEMPLATE := tests/shell/net-posix.script
NET_POSIX_SCRIPT := $(BUILD)/net-posix.script
NET_POSIX_RUN = $(RUNNER) --timeout $(1) --script $(NET_POSIX_SCRIPT) --label $(2) -- $(RUN_QEMU) --test --keys --net --net-open --disk $(NET_POSIX_DISK) --accel $(3) $(KERNEL)

test-net-posix: $(KERNEL) $(RUNNER) $(HTTPD) tools-storage
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[net-posix] skipped: QEMU has no user-mode networking"; exit 0; }; \
	$(MAKE) -s -f ports/Makefile libc && $(MAKE) -s -f ports/Makefile check-libc && \
	mkdir -p $(NET_POSIX_ROOT) target/test-logs && cp -R rootfs/. $(NET_POSIX_ROOT)/ && mkdir -p $(NET_POSIX_ROOT)/bin && \
	printf 'net.hosts = test.zoraya=10.0.2.2\n' > $(NET_POSIX_ROOT)/config/settings && \
	tools/zcc -O2 -o $(NET_POSIX_ROOT)/bin/zfetch tests/posix-net/zfetch.c && \
	$(CARGO) build -q -p zoraya-elf-check && target/debug/zoraya-elf-check $(NET_POSIX_ROOT)/bin/zfetch && \
	target/debug/zoraya-mkfs --size 16M --from $(NET_POSIX_ROOT) $(NET_POSIX_DISK) && \
	: > $(NET_HTTP_READY) && \
	{ $(HTTPD) --port 0 --lifetime 300 --ready-file $(NET_HTTP_READY) > target/test-logs/test-httpd-posix.log 2>&1 & echo $$! > $(BUILD)/net-httpd-posix.pid; } && \
	httpd=$$(cat $(BUILD)/net-httpd-posix.pid); \
	for i in $$(seq 1 20); do [ -s $(NET_HTTP_READY) ] && break; sleep 0.25; done; \
	port=$$(cat $(NET_HTTP_READY)); \
	if [ -z "$$port" ] || ! kill -0 $$httpd 2>/dev/null; then \
		echo "[net-posix] FAIL: the test server did not start"; kill $$httpd 2>/dev/null; cat target/test-logs/test-httpd-posix.log; exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" \
		-e "s/@SUM1M@/$$($(HTTPD) --checksum 1000000)/" \
		-e "s/@SUM300K@/$$($(HTTPD) --checksum 300000)/" \
		-e "s/@SUM70K@/$$($(HTTPD) --checksum 70000)/" \
		-e "s/@SUM4M@/$$($(HTTPD) --checksum 4000000)/" \
		$(NET_POSIX_TEMPLATE) > $(NET_POSIX_SCRIPT); \
	$(call NET_POSIX_RUN,$(QEMU_TIMEOUT),net-posix-auto,auto); rc=$$?; \
	if [ $$rc = 0 ] && [ "$(FULL)" = 1 ]; then $(call NET_POSIX_RUN,400,net-posix-tcg,tcg); rc=$$?; fi; \
	kill $$httpd 2>/dev/null; wait $$httpd 2>/dev/null; \
	if [ $$rc = 0 ]; then target/debug/zoraya-fsck $(NET_POSIX_DISK); rc=$$?; fi; \
	exit $$rc

# TASK-27 9a (proposed ADR-0310, ADR-0311): curl and wget, C programs on
# OpenSSL, zlib and libposix's sockets and getrandom. `make apps` builds them
# into build/rootfs-apps (needs `make fetch-ports` once, as `make ports` does);
# tools/run-disk.sh puts that directory on the volume of `make run`, `run-net`
# and `run-gui` when it exists, so `make test-hand-run` boots the same image
# a person does. `make test-apps` is the full path: the normal kernel with a
# disk of rootfs/, the apps and the test PKI's root in /system/tls/roots (as
# test-net-fetch's disk has), against the runner's HTTP and TLS servers
# (tools/test-httpd): both programs fetch a page, a file and the larger
# bodies over HTTP and over TLS, and refuse an expired and a wrong-name
# certificate before sending a request byte.
APPS_ROOT := $(BUILD)/rootfs-apps
apps:
	@$(MAKE) -f ports/Makefile apps
# NetSurf (TASK-27 9d, ADR-0318) into build/rootfs-apps, as apps does for curl and wget.
netsurf:
	@$(MAKE) -f ports/Makefile netsurf
# The OpenSSL-with-threads test program (tests/apps/tlsthreads.c), for test-apps only.
TLSTHREADS_BIN := $(BUILD)/ports/obj/apps/tlsthreads
$(TLSTHREADS_BIN): apps tests/apps/tlsthreads.c
	@$(MAKE) -f ports/Makefile tlsthreads

CRYPTOTHREADS_BIN := $(BUILD)/ports/obj/apps/cryptothreads
$(CRYPTOTHREADS_BIN): apps tests/apps/cryptothreads.c
	@$(MAKE) -f ports/Makefile cryptothreads

APPS_TEST_ROOT := $(BUILD)/apps-root
APPS_DISK := $(BUILD)/apps-disk.img
APPS_TEMPLATE := tests/shell/apps.script
APPS_SCRIPT := $(BUILD)/apps.script
# The TLS option refusals (ADR-0321) are a run of their own, so that each run
# stays within its timeout under TCG (each line launches a 6.7 MiB program).
APPS_REFUSALS_TEMPLATE := tests/shell/apps-refusals.script
APPS_REFUSALS_SCRIPT := $(BUILD)/apps-refusals.script
APPS_TCG_TIMEOUT ?= 500
# The test server outlives every run that uses it (FULL=1: four); it is killed after them.
APPS_LIFETIME := $(shell expr 2 \* $(QEMU_SLOW_TIMEOUT) + 2 \* $(APPS_TCG_TIMEOUT) + 60)
APPS_HTTP_READY := $(BUILD)/apps-httpd.port
APPS_TLS_READY := $(BUILD)/apps-tlsd.ports
# QEMU options; 512 MiB for the reason RUN_NET_CMD's comment gives.
APPS_QEMU ?= --mem 512M
APPS_RUN = $(RUNNER) --timeout $(1) --script $(4) --label $(2) -- $(RUN_QEMU) --test --keys --net --net-open --disk $(APPS_DISK) $(APPS_QEMU) --accel $(3) $(KERNEL)
# The runs of a program from the volume give their memory back: 16 `curl`s on
# 128 MiB, where 8 fitted while each run kept its image (TASK-27 9a item 2).
APPS_LAUNCHES := tests/shell/apps-launches.script
APPS_LAUNCHES_RUN = $(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script $(APPS_LAUNCHES) --label apps-launches -- $(RUN_QEMU) --test --keys --disk $(APPS_DISK) --mem 128M --accel auto $(KERNEL)

test-apps: $(KERNEL) $(RUNNER) $(HTTPD) tools-storage apps $(TLSTHREADS_BIN) $(CRYPTOTHREADS_BIN)
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[apps] skipped: QEMU has no user-mode networking"; exit 0; }; \
	rm -rf $(APPS_TEST_ROOT) && mkdir -p $(APPS_TEST_ROOT) target/test-logs && cp -R rootfs/. $(APPS_TEST_ROOT)/ && \
	cp -R $(APPS_ROOT)/. $(APPS_TEST_ROOT)/ && cp $(TLSTHREADS_BIN) $(APPS_TEST_ROOT)/bin/tlsthreads && cp $(CRYPTOTHREADS_BIN) $(APPS_TEST_ROOT)/bin/cryptothreads && \
	printf 'net.hosts = test.zoraya=10.0.2.2 expired.zoraya=10.0.2.2 wrong.zoraya=10.0.2.2\n' > $(APPS_TEST_ROOT)/config/settings && \
	cp tests/net/tls/root.pem $(APPS_TEST_ROOT)/system/tls/roots && \
	mkdir -p $(APPS_TEST_ROOT)/etc/test-pki/hashed && cp tests/net/tls/other-root.pem $(APPS_TEST_ROOT)/etc/test-pki/ && \
	cp tests/net/tls/hashed/24824ecf.0 $(APPS_TEST_ROOT)/etc/test-pki/hashed/ && \
	$(CARGO) build -q -p zoraya-elf-check && target/debug/zoraya-elf-check $(APPS_TEST_ROOT)/bin && \
	target/debug/zoraya-mkfs --size 64M --from $(APPS_TEST_ROOT) $(APPS_DISK) && \
	: > $(APPS_HTTP_READY) && : > $(APPS_TLS_READY) && \
	{ $(HTTPD) --port 0 --lifetime $(APPS_LIFETIME) --ready-file $(APPS_HTTP_READY) --tls-dir tests/net/tls --tls-ready-file $(APPS_TLS_READY) > target/test-logs/test-httpd-apps.log 2>&1 & echo $$! > $(BUILD)/apps-httpd.pid; } && \
	httpd=$$(cat $(BUILD)/apps-httpd.pid); \
	for i in $$(seq 1 20); do [ -s $(APPS_HTTP_READY) ] && [ -s $(APPS_TLS_READY) ] && break; sleep 0.25; done; \
	set -- $$(cat $(APPS_TLS_READY)); tlsvalid=$$1; tlsexpired=$$2; tlswrong=$$3; \
	port=$$(cat $(APPS_HTTP_READY)); \
	if [ -z "$$port" ] || [ -z "$$tlswrong" ] || ! kill -0 $$httpd 2>/dev/null; then \
		echo "[apps] FAIL: the test server did not start"; kill $$httpd 2>/dev/null; cat target/test-logs/test-httpd-apps.log; exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" -e "s/@TLSVALID@/$$tlsvalid/g" -e "s/@TLSEXPIRED@/$$tlsexpired/g" -e "s/@TLSWRONG@/$$tlswrong/g" \
		$(APPS_TEMPLATE) > $(APPS_SCRIPT); \
	sed -e "s/@TLSVALID@/$$tlsvalid/g" -e "s/@TLSEXPIRED@/$$tlsexpired/g" -e "s/@TLSWRONG@/$$tlswrong/g" \
		$(APPS_REFUSALS_TEMPLATE) > $(APPS_REFUSALS_SCRIPT); \
	$(call APPS_RUN,$(QEMU_SLOW_TIMEOUT),apps-auto,auto,$(APPS_SCRIPT)); rc=$$?; \
	if [ $$rc = 0 ]; then $(call APPS_RUN,$(QEMU_SLOW_TIMEOUT),apps-refusals-auto,auto,$(APPS_REFUSALS_SCRIPT)); rc=$$?; fi; \
	if [ $$rc = 0 ] && [ "$(FULL)" = 1 ]; then $(call APPS_RUN,$(APPS_TCG_TIMEOUT),apps-tcg,tcg,$(APPS_SCRIPT)); rc=$$?; fi; \
	if [ $$rc = 0 ] && [ "$(FULL)" = 1 ]; then $(call APPS_RUN,$(APPS_TCG_TIMEOUT),apps-refusals-tcg,tcg,$(APPS_REFUSALS_SCRIPT)); rc=$$?; fi; \
	kill $$httpd 2>/dev/null; wait $$httpd 2>/dev/null; \
	if [ $$rc = 0 ]; then $(APPS_LAUNCHES_RUN); rc=$$?; fi; \
	if [ $$rc = 0 ]; then \
		if grep -q '^tls \(expired\|wrong\): request of' target/test-logs/test-httpd-apps.log || ! grep -q '^tls valid: request of' target/test-logs/test-httpd-apps.log \
		   || ! grep -q '^tls expired: ended after 0 request bytes' target/test-logs/test-httpd-apps.log || ! grep -q '^tls wrong: ended after 0 request bytes' target/test-logs/test-httpd-apps.log \
		   || grep '^tls \(expired\|wrong\): ended after' target/test-logs/test-httpd-apps.log | grep -qv 'ended after 0 request bytes'; then \
			echo '[apps] FAIL: INV-0228-1: a C program sent request bytes after a refused handshake (or the server saw no clean end), or no request on the valid one'; cat target/test-logs/test-httpd-apps.log; rc=1; \
		else echo '[apps] ok: curl and wget sent no request byte to the refused TLS servers'; fi; \
	fi; \
	if [ $$rc = 0 ]; then target/debug/zoraya-fsck $(APPS_DISK); rc=$$?; fi; \
	exit $$rc

# TASK-27 9c (proposed ADR-0320): CPython 3.15, cross-built static with zcc.
# `make python` builds the host python of the same version (build/ports/hostpython),
# SQLite for the target and the program with its standard library into
# build/rootfs-python (needs `make fetch-ports` and the OpenSSL of `make apps`'
# ports once). Three steps boot a volume made of rootfs/, that directory and
# the test suite's subset (tests/python/), each with its own volume and script,
# so that each is a CI job of its own (ADR-0320 item 9: under TCG the whole of
# it did not fit one 30-minute job, run 43488132441):
#   test-python       tests/shell/python.script: smoke.py, grants, the planted
#                     module, Ctrl-C, the TLS clients (https_client.py,
#                     https_blocking.py) against the runner's servers
#   test-python-regr  tests/shell/python-regr.script: regr.py over
#                     tests/python/regr-modules.txt, the CI subset
#   test-python-long  the same over tests/python/regr-long.txt, the modules too
#                     slow for a CI job under TCG; by hand, not in make test
PYTHON_ROOT := $(BUILD)/rootfs-python
python:
	@$(MAKE) -f ports/Makefile python
PYTHON_QEMU ?= --mem 512M
PYTHON_ACCEL ?= auto
# The runner's limits, below the python CI jobs' 45 minutes
# (.github/workflows/test.yml), so that a slow run ends with the runner's
# report and not a cancelled job.
PYTHON_TIMEOUT ?= 900
PYTHON_REGR_TIMEOUT ?= 1500
PYTHON_LONG_TIMEOUT ?= 3600
# regrtest's options after the list (regr.py): --slowest prints the ten slowest
# modules, the measurement the CI subset is chosen by.
PYTHON_REGR_ARGS ?= --slowest
test-python: PY_TEMPLATE := tests/shell/python.script
test-python: PY_TIMEOUT = $(PYTHON_TIMEOUT)
test-python: PY_LIST := regr-modules.txt
test-python-regr: PY_TEMPLATE := tests/shell/python-regr.script
test-python-regr: PY_TIMEOUT = $(PYTHON_REGR_TIMEOUT)
test-python-regr: PY_LIST := regr-modules.txt
test-python-long: PY_TEMPLATE := tests/shell/python-regr.script
test-python-long: PY_TIMEOUT = $(PYTHON_LONG_TIMEOUT)
test-python-long: PY_LIST := regr-long.txt
# Every path is the step's own ($@), so that test-par may run the steps at once.
PY_ROOT = $(BUILD)/$@-root
PY_DISK = $(BUILD)/$@-disk.img
PY_SCRIPT = $(BUILD)/$@.script
PY_READY = $(BUILD)/$@-httpd.port
PY_TLS_READY = $(BUILD)/$@-tlsd.ports
PY_HTTPD_LOG = target/test-logs/test-httpd-$@.log
# The standard library's own tests: the modules of tests/python/regr-modules.txt
# and regr-long.txt, with the support files of Lib/test (every file of its top
# level that is not a test, support/ and libregrtest/), taken from the source
# tree (configure's --disable-test-modules leaves Lib/test out of the stage) and
# compiled by the host python, as the rest of the library is.
PYTHON_STAGE_LIB := $(BUILD)/ports/work/Python-3.15.2/Lib
PYTHON_HOSTPY := $(BUILD)/ports/hostpython/bin/python3.15
PYTHON_TESTLIB := $(BUILD)/python-testlib
PYTHON_REGR_FILES := tests/python/regr-modules.txt tests/python/regr-long.txt tests/python/regr-ignore.txt
# Made once, by ci-build, so that the test jobs need neither the source tree nor
# the host python (neither is in the build artifact).
$(PYTHON_TESTLIB): $(PYTHON_ROOT)/.stamp $(PYTHON_REGR_FILES)
	@$(MAKE) --no-print-directory python
	rm -rf $(PYTHON_TESTLIB).tmp && mkdir -p $(PYTHON_TESTLIB).tmp
	for f in $(PYTHON_STAGE_LIB)/test/*; do n=$$(basename $$f); \
		case $$n in test_*) ;; *) cp -R $$f $(PYTHON_TESTLIB).tmp/ ;; esac; done
	for m in $$(cat tests/python/regr-modules.txt tests/python/regr-long.txt | sed 's/\..*//' | sort -u); do cp -R $(PYTHON_STAGE_LIB)/test/$$m* $(PYTHON_TESTLIB).tmp/; done
	@# deliberately bad test files do not compile, so the exit status is not read
	$(PYTHON_HOSTPY) -I -m compileall -q -f -j3 --invalidation-mode unchecked-hash -d /usr/lib/python3.15/test $(PYTHON_TESTLIB).tmp >/dev/null 2>&1; true
	rm -rf $(PYTHON_TESTLIB) && mv $(PYTHON_TESTLIB).tmp $(PYTHON_TESTLIB)
python-testlib: $(PYTHON_TESTLIB)
# The TLS check (INV-0320-3) reads the server's log of test-python only: the
# regrtest scripts make no connection to it.
test-python test-python-regr test-python-long: $(KERNEL) $(RUNNER) $(HTTPD) tools-storage python $(PYTHON_TESTLIB)
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[$@] skipped: QEMU has no user-mode networking"; exit 0; }; \
	rm -rf $(PY_ROOT) && mkdir -p $(PY_ROOT) target/test-logs && cp -R rootfs/. $(PY_ROOT)/ && \
	cp -R $(PYTHON_ROOT)/. $(PY_ROOT)/ && mkdir -p $(PY_ROOT)/home && cp tests/python/*.py tests/python/regr-*.txt $(PY_ROOT)/home/ && \
	mkdir -p $(PY_ROOT)/usr/lib/python3.15/test && cp -R $(PYTHON_TESTLIB)/. $(PY_ROOT)/usr/lib/python3.15/test/ && \
	printf 'net.hosts = test.zoraya=10.0.2.2 expired.zoraya=10.0.2.2 wrong.zoraya=10.0.2.2\n' > $(PY_ROOT)/config/settings && \
	cp tests/net/tls/root.pem $(PY_ROOT)/system/tls/roots && \
	$(CARGO) build -q -p zoraya-elf-check && target/debug/zoraya-elf-check $(PY_ROOT)/bin && \
	target/debug/zoraya-mkfs --size 256M --from $(PY_ROOT) $(PY_DISK) && \
	: > $(PY_READY) && : > $(PY_TLS_READY) && \
	{ $(HTTPD) --port 0 --lifetime $$(( $(PY_TIMEOUT) + 60 )) --ready-file $(PY_READY) --tls-dir tests/net/tls --tls-ready-file $(PY_TLS_READY) > $(PY_HTTPD_LOG) 2>&1 & echo $$! > $(BUILD)/$@-httpd.pid; } && \
	httpd=$$(cat $(BUILD)/$@-httpd.pid); \
	for i in $$(seq 1 20); do [ -s $(PY_READY) ] && [ -s $(PY_TLS_READY) ] && break; sleep 0.25; done; \
	set -- $$(cat $(PY_TLS_READY)); tlsvalid=$$1; tlsexpired=$$2; tlswrong=$$3; \
	port=$$(cat $(PY_READY)); \
	if [ -z "$$port" ] || [ -z "$$tlswrong" ] || ! kill -0 $$httpd 2>/dev/null; then \
		echo "[$@] FAIL: the test server did not start"; kill $$httpd 2>/dev/null; cat $(PY_HTTPD_LOG); exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" -e "s/@TLSVALID@/$$tlsvalid/g" -e "s/@TLSEXPIRED@/$$tlsexpired/g" -e "s/@TLSWRONG@/$$tlswrong/g" \
		-e "s/@REGRLIST@/$(PY_LIST)/g" -e "s/@REGRARGS@/$(PYTHON_REGR_ARGS)/g" $(PY_TEMPLATE) > $(PY_SCRIPT); \
	$(RUNNER) --timeout $(PY_TIMEOUT) --script $(PY_SCRIPT) --label $(@:test-%=%)-$(PYTHON_ACCEL) -- \
		$(RUN_QEMU) --test --keys --net --net-open --disk $(PY_DISK) $(PYTHON_QEMU) --accel $(PYTHON_ACCEL) $(KERNEL); rc=$$?; \
	kill $$httpd 2>/dev/null; wait $$httpd 2>/dev/null; \
	if [ $$rc = 0 ] && [ $@ = test-python ]; then \
		if grep -q -e '^tls \(expired\|wrong\): request of' -e '^tls \(expired\|wrong\): ended after [1-9]' $(PY_HTTPD_LOG) || ! grep -q '^tls valid: request of' $(PY_HTTPD_LOG) \
		   || [ "$$(grep -c '^tls expired: ended after 0 request bytes' $(PY_HTTPD_LOG))" != 3 ] || [ "$$(grep -c '^tls wrong: ended after 0 request bytes' $(PY_HTTPD_LOG))" != 3 ]; then \
			echo '[python] FAIL: INV-0320-3: a Python program sent request bytes after a refused handshake, or a refused server did not see exactly 3 connections end after 0 request bytes (asyncio, http.client, urllib), or no request on the valid one'; cat $(PY_HTTPD_LOG); rc=1; \
		else echo '[python] ok: asyncio, http.client and urllib sent no request byte to the refused TLS servers (3 connections each, 0 bytes)'; fi; \
	fi; \
	if [ $$rc = 0 ]; then target/debug/zoraya-fsck $(PY_DISK); rc=$$?; fi; \
	exit $$rc

# TASK-27 9d (proposed ADR-0317, ADR-0318): NetSurf in a window of the desktop. The
# image is test-apps' (rootfs/, build/rootfs-apps with `netsurf`, the test PKI's root,
# test names), the GUI headless with the network, the runner's HTTP and TLS servers
# on the host (tools/test-httpd). The script tests/gui/netsurf.script reads the
# screen by colour regions and takes screendumps (docs/netsurf/ in the report).
NS_ROOT := $(BUILD)/netsurf-root
NS_DISK := $(BUILD)/netsurf-disk.img
NS_TEMPLATE := tests/gui/netsurf.script
# ADR-0326 (9f-text): the Cyrillic page again at scale 2, in a second boot (the
# first run is at scale 1); the system's fonts are in the root's /system/fonts.
NS_TEXT_TEMPLATE := tests/gui/netsurf-text.script
NS_TEXT_SCRIPT := $(BUILD)/netsurf-text.script
# The test server outlives both runs' timeouts (review N6).
NS_LIFETIME := $(shell expr 2 \* $(QEMU_SLOW_TIMEOUT) + 60)
NS_SCRIPT := $(BUILD)/netsurf.script
NS_HTTP_READY := $(BUILD)/netsurf-httpd.port
NS_TLS_READY := $(BUILD)/netsurf-tlsd.ports
NS_RUN = $(RUNNER) --timeout $(1) --gui-script $(4) --qmp $(BUILD)/netsurf-$(2).qmp --label netsurf-$(2) -- $(RUN_QEMU) --test --gui --gui-scale $(5) --net --net-open --mem 768M --qmp $(BUILD)/netsurf-$(2).qmp --disk $(NS_DISK) --accel $(3) $(KERNEL)
test-netsurf: $(KERNEL) $(RUNNER) $(HTTPD) tools-storage netsurf
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[netsurf] skipped: QEMU has no user-mode networking"; exit 0; }; \
	rm -rf $(NS_ROOT) && mkdir -p $(NS_ROOT) target/test-logs && cp -R rootfs/. $(NS_ROOT)/ && \
	cp -R $(APPS_ROOT)/. $(NS_ROOT)/ && \
	mkdir -p $(NS_ROOT)/system/fonts && cp -R fonts/. $(NS_ROOT)/system/fonts/ && \
	printf 'net.hosts = test.zoraya=10.0.2.2 expired.zoraya=10.0.2.2 wrong.zoraya=10.0.2.2\n' > $(NS_ROOT)/config/settings && \
	cp tests/net/tls/root.pem $(NS_ROOT)/system/tls/roots && \
	$(CARGO) build -q -p zoraya-elf-check && target/debug/zoraya-elf-check $(NS_ROOT)/bin && \
	target/debug/zoraya-mkfs --size 64M --from $(NS_ROOT) $(NS_DISK) && \
	: > $(NS_HTTP_READY) && : > $(NS_TLS_READY) && \
	{ $(HTTPD) --port 0 --lifetime $(NS_LIFETIME) --ready-file $(NS_HTTP_READY) --tls-dir tests/net/tls --tls-ready-file $(NS_TLS_READY) > target/test-logs/test-httpd-netsurf.log 2>&1 & echo $$! > $(BUILD)/netsurf-httpd.pid; } && \
	httpd=$$(cat $(BUILD)/netsurf-httpd.pid); \
	for i in $$(seq 1 20); do [ -s $(NS_HTTP_READY) ] && [ -s $(NS_TLS_READY) ] && break; sleep 0.25; done; \
	set -- $$(cat $(NS_TLS_READY)); tlsvalid=$$1; tlsexpired=$$2; \
	port=$$(cat $(NS_HTTP_READY)); \
	if [ -z "$$port" ] || [ -z "$$tlsvalid" ] || ! kill -0 $$httpd 2>/dev/null; then \
		echo "[netsurf] FAIL: the test server did not start"; kill $$httpd 2>/dev/null; cat target/test-logs/test-httpd-netsurf.log; exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" -e "s/@TLSVALID@/$$tlsvalid/g" -e "s/@TLSEXPIRED@/$$tlsexpired/g" $(NS_TEMPLATE) > $(NS_SCRIPT); \
	sed -e "s/@PORT@/$$port/g" $(NS_TEXT_TEMPLATE) > $(NS_TEXT_SCRIPT); \
	$(call NS_RUN,$(QEMU_SLOW_TIMEOUT),auto,auto,$(NS_SCRIPT),1); rc=$$?; \
	if [ $$rc = 0 ]; then $(call NS_RUN,$(QEMU_SLOW_TIMEOUT),text-2,auto,$(NS_TEXT_SCRIPT),2); rc=$$?; fi; \
	kill $$httpd 2>/dev/null; wait $$httpd 2>/dev/null; \
	if [ $$rc = 0 ]; then \
		log=target/test-logs/test-httpd-netsurf.log; mark='^http: GET /ns/mark-before-proceed '; \
		before=$$(sed -n "1,\#$$mark#p" $$log | grep -c '^tls expired: ended after 0 request bytes'); \
		after=$$(sed -n "\#$$mark#,\$$p" $$log | grep -c '^tls expired: ended after 0 request bytes'); \
		if grep -q '^tls expired: request of' $$log || ! grep -q '^tls valid: request of' $$log \
		   || ! grep -q "$$mark" $$log || [ "$$before" != 1 ] || [ "$$after" -lt 1 ] \
		   || grep '^tls expired: ended after' $$log | grep -qv 'ended after 0 request bytes'; then \
			echo "[netsurf] FAIL: INV-0228-1: NetSurf sent request bytes after a refused handshake (or no request on the valid one, or not exactly one refused handshake before the Proceed mark ($$before) and one or more after it ($$after): a retry of its own, or a click on Proceed that missed)"; cat $$log; rc=1; \
		else echo '[netsurf] ok: NetSurf sent no request byte to the expired-certificate server, also after Proceed'; fi; \
	fi; \
	if [ $$rc = 0 ]; then target/debug/zoraya-fsck $(NS_DISK); rc=$$?; fi; \
	exit $$rc

# An interactive machine on QEMU's user network without `restrict`: the guest
# reaches the Internet through the host, for trying things by hand.
run-net: $(KERNEL) $(DISK)
	$(call RUN_NET_CMD,$(DISK))

# What a person runs by hand, booted as it is (TASK-26, hand-run): the image
# $(KERNEL), not a test image; a disk made by tools/run-disk.sh, the recipe of
# $(DISK), here at $(HAND_DISK) so that a person's disk stays; and the command
# lines RUN_CMD, RUN_NET_CMD and RUN_GUI_CMD (interactive mode; RUN_GUI_CMD
# with no window and a QMP socket to read its screen). The first disk is a
# stale one without a key, which run-disk.sh must replace (what made the
# architect's `make run-net` fail); a second call must keep the new one. Each
# boot forbids the lines of a volume that did not mount ("fsd: mount", "there
# is no volume") and waits for the home's prompt, `~>`, not `/>`. The network
# run fetches http:// and https:// from
# tools/test-httpd on the host's 127.0.0.1: https:// reaches the certificate,
# which the production roots refuse (the full path is test-net-fetch's).
HAND_DISK := $(BUILD)/hand-run-disk.img
HAND_NET_TEMPLATE := tests/shell/hand-run-net.script
HAND_NET_SCRIPT := $(BUILD)/hand-run-net.script
HAND_SCRIPT := tests/shell/hand-run.script
HAND_GUI_SCRIPT := tests/shell/hand-run-gui.script
HAND_HTTP_READY := $(BUILD)/hand-run-httpd.port
HAND_TLS_READY := $(BUILD)/hand-run-tlsd.ports
test-hand-run: $(KERNEL) $(RUNNER) $(HTTPD) tools-storage apps
	@rm -f $(HAND_DISK) $(HAND_DISK).key $(HAND_DISK).old $(HAND_DISK).old.* $(BUILD)/hand-run-disk.copy
	@head -c 1048576 /dev/zero > $(HAND_DISK)
	@$(MAKE) --no-print-directory DISK=$(HAND_DISK) $(HAND_DISK) 2> $(BUILD)/hand-run-disk.log; cat $(BUILD)/hand-run-disk.log; \
	if [ ! -f $(HAND_DISK).old ] || ! grep -q 'made for another format' $(BUILD)/hand-run-disk.log; then \
		echo "[hand-run] FAIL: tools/run-disk.sh kept a disk without a key"; exit 1; fi; \
	cp $(HAND_DISK) $(BUILD)/hand-run-disk.copy && $(MAKE) --no-print-directory DISK=$(HAND_DISK) $(HAND_DISK) 2> $(BUILD)/hand-run-disk.log; \
	if [ -s $(BUILD)/hand-run-disk.log ] || ! cmp -s $(HAND_DISK) $(BUILD)/hand-run-disk.copy; then \
		echo "[hand-run] FAIL: tools/run-disk.sh replaced a disk of this system"; cat $(BUILD)/hand-run-disk.log; exit 1; fi; \
	cp $(HAND_DISK).old $(BUILD)/hand-run-disk.copy && echo stale > $(HAND_DISK).key && \
	$(MAKE) --no-print-directory DISK=$(HAND_DISK) $(HAND_DISK) 2> $(BUILD)/hand-run-disk.log; \
	if [ ! -f $(HAND_DISK).old.1 ] || ! cmp -s $(HAND_DISK).old $(BUILD)/hand-run-disk.copy; then \
		echo "[hand-run] FAIL: tools/run-disk.sh overwrote an earlier .old disk"; cat $(BUILD)/hand-run-disk.log; exit 1; fi; \
	echo "[hand-run] ok: a stale disk is replaced and never overwrites an earlier .old, a current one kept"
	@$(RUN_QEMU) --probe-net >/dev/null 2>&1 || { echo "[hand-run] skipped run-net: QEMU has no user-mode networking"; exit 0; }; \
	mkdir -p target/test-logs && : > $(HAND_HTTP_READY) && : > $(HAND_TLS_READY) && \
	{ $(HTTPD) --port 0 --lifetime 300 --ready-file $(HAND_HTTP_READY) --tls-dir tests/net/tls --tls-ready-file $(HAND_TLS_READY) > target/test-logs/test-httpd-hand-run.log 2>&1 & echo $$! > $(BUILD)/hand-run-httpd.pid; } && \
	httpd=$$(cat $(BUILD)/hand-run-httpd.pid); \
	for i in $$(seq 1 20); do [ -s $(HAND_HTTP_READY) ] && [ -s $(HAND_TLS_READY) ] && break; sleep 0.25; done; \
	set -- $$(cat $(HAND_TLS_READY)); tlsvalid=$${1:-}; port=$$(cat $(HAND_HTTP_READY)); \
	if [ -z "$$port" ] || [ -z "$$tlsvalid" ] || ! kill -0 $$httpd 2>/dev/null; then \
		echo "[hand-run] FAIL: the test server did not start"; kill $$httpd 2>/dev/null; cat target/test-logs/test-httpd-hand-run.log; exit 1; fi; \
	sed -e "s/@PORT@/$$port/g" -e "s/@TLSVALID@/$$tlsvalid/g" $(HAND_NET_TEMPLATE) > $(HAND_NET_SCRIPT); \
	$(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script $(HAND_NET_SCRIPT) --label hand-run-net -- $(call RUN_NET_CMD,$(HAND_DISK)); rc=$$?; \
	kill $$httpd 2>/dev/null; wait $$httpd 2>/dev/null; \
	if [ $$rc = 0 ]; then target/debug/zoraya-fsck $(HAND_DISK); rc=$$?; fi; \
	exit $$rc
	@tools/run-disk.sh $(HAND_DISK)
	$(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script $(HAND_SCRIPT) --label hand-run -- $(call RUN_CMD,$(HAND_DISK))
	target/debug/zoraya-fsck $(HAND_DISK)
	@tools/run-disk.sh $(HAND_DISK)
	$(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --gui-script $(HAND_GUI_SCRIPT) --qmp $(BUILD)/hand-run-gui.qmp --label hand-run-gui -- \
		$(call RUN_GUI_CMD,$(HAND_DISK),--display-opts none --qmp $(BUILD)/hand-run-gui.qmp)
	target/debug/zoraya-fsck $(HAND_DISK)

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
CLOCK_SCRIPT := tests/shell/clock.script
STORAGE_DOWN := tests/shell/storage-down.script
DEVICE_ERROR := tests/shell/device-error.script
ROOMS_SCRIPT := tests/shell/rooms.script
DISK_FILLER_SCRIPT := tests/shell/disk-filler.script
HOSTILE_SCRIPT := tests/shell/hostile.script
GRANTS_SCRIPT := tests/shell/grants.script
GRANTS_ROOT := $(BUILD)/grants-root
SYSTEM_RO_ROOT := $(BUILD)/system-ro-root
SYSTEM_RO_TEMPLATE := tests/shell/system-ro.script
SYSTEM_RO_SCRIPT := $(BUILD)/system-ro.script
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
# ADR-0289 item 8: /system is read-only for the console shell and for a +w grant.
	rm -rf $(SYSTEM_RO_ROOT) && mkdir -p $(SYSTEM_RO_ROOT) && cp -R rootfs/. $(SYSTEM_RO_ROOT)/
	target/debug/zoraya-mkfs --size 16M --from $(SYSTEM_RO_ROOT) $(CRASH_IMAGE)
	sed -e "s/@ROOTSSIZE@/$$(wc -c < rootfs/system/tls/roots | tr -d ' ')/" $(SYSTEM_RO_TEMPLATE) > $(SYSTEM_RO_SCRIPT)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(SYSTEM_RO_SCRIPT) --label system-ro -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# INV-0155-1: every command's --help and usage error, with a volume.
	target/debug/zoraya-mkfs --size 16M --from rootfs $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(CONVENTIONS_SCRIPT) --label conventions -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# ADR-0226: the date is the host's, to 2 s.
	target/debug/zoraya-mkfs --size 16M --from rootfs $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(CLOCK_SCRIPT) --label clock -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# What a command gets (ADR-0153): rootfs/ with link objects over it.
	rm -rf $(GRANTS_ROOT) && mkdir -p $(GRANTS_ROOT) && cp -R rootfs/. tests/shell/grants-root/. $(GRANTS_ROOT)/
	target/debug/zoraya-mkfs --size 16M --from $(GRANTS_ROOT) $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(GRANTS_SCRIPT) --label grants -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# Rooms with limits through the room keeper (Q6a; INV-0266-1, INV-0263-4, INV-0269-1).
	target/debug/zoraya-mkfs --size 16M --from $(GRANTS_ROOT) $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(ROOMS_SCRIPT) --label rooms -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
# The gate's disk-filler and hostile-programs rows (Q7), from the console, in
# rooms with limits: DISK_FILLER_SCRIPT, HOSTILE_SCRIPT.
	target/debug/zoraya-mkfs --size 16M --from $(GRANTS_ROOT) $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(DISK_FILLER_SCRIPT) --label disk-filler -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
	target/debug/zoraya-mkfs --size 16M --from $(GRANTS_ROOT) $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(HOSTILE_SCRIPT) --label hostile -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(KERNEL)
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

# Part 9b (ADR-0302, ADR-0309): musl's threads over the shim, through libposix,
# from posixrun, on the same image as test-libposix with /bin/t_threads.
POSIX_THREADS := $(BUILD)/posix-threads
POSIX_THREADS_SCRIPT := tests/posix-core/threads.script
test-posix-threads: $(KERNEL) $(RUNNER) tools-storage
	@$(MAKE) -s -f ports/Makefile libc
	@$(MAKE) -s -f ports/Makefile check-libc
	rm -rf $(POSIX_THREADS) && mkdir -p $(POSIX_THREADS)/root/bin $(POSIX_THREADS)/root/work
	cp -R rootfs/. $(POSIX_THREADS)/root/
	tools/zcc -O2 -o $(POSIX_THREADS)/root/bin/t_threads tests/posix-core/t_threads.c
	$(CARGO) build -q -p zoraya-elf-check
	target/debug/zoraya-elf-check $(POSIX_THREADS)/root/bin/t_threads
	@for accel in $(LOCAL_ACCELS); do \
	  target/debug/zoraya-mkfs --size 32M --from $(POSIX_THREADS)/root $(POSIX_THREADS)/disk.img && \
	  $(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script $(POSIX_THREADS_SCRIPT) --label posix-threads-$$accel -- \
	    $(RUN_QEMU) --test --keys --disk $(POSIX_THREADS)/disk.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(POSIX_THREADS)/disk.img || exit 1; done

# Part 9b's thread suites (ADR-0309 items 1 and 2): musl's libc-test thread
# tests (tests/libc-test/threads-excluded.txt names what is left out and why),
# each run by posixrun on the image of test-libposix. They need
# `make fetch-ports` once, as the ports do; the sources are unpacked by
# `make -f ports/Makefile suites`.
POSIX_SUITES := $(BUILD)/posix-suites
test-posix-suites: $(KERNEL) $(RUNNER) tools-storage
	@$(MAKE) -s -f ports/Makefile libc
	@$(MAKE) -s -f ports/Makefile check-libc
	@$(MAKE) -s -f ports/Makefile suites
	rm -rf $(POSIX_SUITES) && mkdir -p $(POSIX_SUITES)/root/bin/lct $(POSIX_SUITES)/root/work
	cp -R rootfs/. $(POSIX_SUITES)/root/
	tests/libc-test/build.sh $(BUILD)/ports/workloads/libctest $(POSIX_SUITES)/lct
	cp $(POSIX_SUITES)/lct/bin/* $(POSIX_SUITES)/root/bin/lct/
	$(CARGO) build -q -p zoraya-elf-check
	target/debug/zoraya-elf-check $(POSIX_SUITES)/root/bin/lct/*
	@for accel in $(LOCAL_ACCELS); do \
	  target/debug/zoraya-mkfs --size 32M --from $(POSIX_SUITES)/root $(POSIX_SUITES)/disk.img && \
	  $(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script $(POSIX_SUITES)/lct/script --label posix-suites-$$accel -- \
	    $(RUN_QEMU) --test --keys --disk $(POSIX_SUITES)/disk.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(POSIX_SUITES)/disk.img || exit 1; done

# SQLite's threadtest3 (ADR-0309 item 2): the scenarios of
# tests/libc-test/threadtest3.script (the rollback-journal ones; the WAL ones
# are in tests/libc-test/threads-excluded.txt) on SQLite's default unix VFS,
# which takes `fcntl` record locks. libposix has them, in the process
# (ADR-0319, INV-0319-3). A step of make test, on both accelerators in CI.
# `make posix-threadtest3` is the same, under its old name.
POSIX_TT := $(BUILD)/posix-tt
posix-threadtest3: test-posix-threadtest3
test-posix-threadtest3: $(KERNEL) $(RUNNER) tools-storage
	@$(MAKE) -s -f ports/Makefile libc suites
	@$(MAKE) -s -f ports/Makefile workloads
	rm -rf $(POSIX_TT) && mkdir -p $(POSIX_TT)/root/bin $(POSIX_TT)/root/work
	cp -R rootfs/. $(POSIX_TT)/root/
	tools/zcc -O2 -w -DSQLITE_THREADSAFE=1 -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_WAL \
	  -I$(BUILD)/ports/workloads/sqlite -I$(BUILD)/ports/workloads/sqlitesrc/src -I$(BUILD)/ports/workloads/sqlitesrc/test \
	  -o $(POSIX_TT)/root/bin/threadtest3 $(BUILD)/ports/workloads/sqlitesrc/test/threadtest3.c \
	  $(BUILD)/ports/workloads/sqlite/sqlite3.c $(BUILD)/ports/workloads/sqlitesrc/src/test_multiplex.c \
	  tests/libc-test/strerror_r.c
	@for accel in $(LOCAL_ACCELS); do \
	  target/debug/zoraya-mkfs --size 32M --from $(POSIX_TT)/root $(POSIX_TT)/disk.img && \
	  $(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script tests/libc-test/threadtest3.script --label posix-threadtest3-$$accel -- \
	    $(RUN_QEMU) --test --keys --disk $(POSIX_TT)/disk.img --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(POSIX_TT)/disk.img || exit 1; done

# musl's pthread_cancel-points without its shm_open scenario (shm_open is T3,
# ADR-0005, not exported, not in 9b's scope; tests/libc-test/threads-excluded.txt):
# the six cancel scenarios (sem_wait, sem_timedwait, pthread_join, each blocking
# and not) run on a copy of the source with that one scenario deleted. NOT a
# step of make test; run by hand with `make posix-cancel-points`.
posix-cancel-points: $(KERNEL) $(RUNNER) tools-storage
	@$(MAKE) -s -f ports/Makefile libc suites
	rm -rf $(POSIX_SUITES)/cp && mkdir -p $(POSIX_SUITES)/cp/root/bin $(POSIX_SUITES)/cp/root/work
	cp -R rootfs/. $(POSIX_SUITES)/cp/root/
	sed '/execute_shm_open, cleanup_shm/d' $(BUILD)/ports/workloads/libctest/src/functional/pthread_cancel-points.c \
	  > $(POSIX_SUITES)/cp/cancel-points-noshm.c
	tools/zcc -O2 -w -I$(BUILD)/ports/workloads/libctest/src/common -o $(POSIX_SUITES)/cp/root/bin/cancel-points \
	  $(POSIX_SUITES)/cp/cancel-points-noshm.c $(BUILD)/ports/workloads/libctest/src/common/print.c
	target/debug/zoraya-mkfs --size 32M --from $(POSIX_SUITES)/cp/root $(POSIX_SUITES)/cp/disk.img
	$(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script tests/libc-test/cancel-points.script --label posix-cancel-points -- \
	  $(RUN_QEMU) --test --keys --disk $(POSIX_SUITES)/cp/disk.img --accel auto $(KERNEL)

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
POSIX_TIMEOUTS := threads=900 w1=600 w2=600 w2j=900 w3=600 w3j=900 w8=1800 w9-int=600 w9-term=600 \
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
	tests/posix/invariants.script tests/posix/scenarios.script tests/posix/threads.script \
	tests/posix/zosh.script tests/posix/tour.script
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
	  --no-thread-calls docs/adr/posix-surface-T9.txt \
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
GUI_RUNS_ALL := terminal:auto:1 terminal:tcg:1 windows:auto:1 windows:tcg:1 layouts:auto:1 layouts:tcg:1 rooms:auto:1 rooms:tcg:1 limits:auto:1 limits:tcg:1 spinners:auto:1 spinners:tcg:1 files:auto:1 files:tcg:1 files-manage:auto:1 files-manage:tcg:1 files-manage:auto:2 settings:auto:1 settings:tcg:1 wheel:auto:1 wheel:tcg:1 wheel:auto:2 net:auto:1 net:tcg:1 terminal:auto:2 terminal:tcg:2 windows:auto:2 terminal:auto:1600x1000 terminal:auto:1920x1200
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
	  net=; [ -f tests/gui/$$script.net ] && net=--net; \
	  $(RUNNER) --timeout $$timeout --gui-script tests/gui/$$script.script --qmp $(BUILD)/$$label.qmp --label $$label -- \
	    $(RUN_QEMU) --test --gui $$net $$size --mem 512M --qmp $(BUILD)/$$label.qmp --disk $(GUI_IMAGE) --accel $$accel $(KERNEL) && \
	  target/debug/zoraya-fsck $(GUI_IMAGE) > /dev/null || exit 1; done

# Fuzzing (ADR-0268 point 3's "fuzzing" and point 7's "net-fuzz", ADR-0298):
# libFuzzer on a pinned dated nightly (tools/run-fuzz.sh; skipped unless CI or
# FUZZ_FORCE=1 is set, as test-net skips without user networking) for FUZZ_SECONDS a target, FUZZ_JOBS at
# once. CI runs 600 s (tools/ci-tests.txt); locally 20 s.
FUZZ_SECONDS ?= 20
FUZZ_JOBS ?= 2
FUZZ_TARGETS := spawn_request keeper_messages broker_tty compositor_protocol file_protocol
FUZZ_NET_TARGETS := net_rx net_stack net_dns_http
test-fuzz:
	@if [ -z "$$CI$$FUZZ_FORCE" ]; then echo "[fuzz] skipped: needs the pinned nightly toolchain (tools/run-fuzz.sh); runs on CI, or here with FUZZ_FORCE=1"; exit 0; fi; \
	tools/run-fuzz.sh $(FUZZ_SECONDS) $(FUZZ_JOBS) $(FUZZ_TARGETS)

test-fuzz-net:
	@if [ -z "$$CI$$FUZZ_FORCE" ]; then echo "[fuzz] skipped: needs the pinned nightly toolchain (tools/run-fuzz.sh); runs on CI, or here with FUZZ_FORCE=1"; exit 0; fi; \
	tools/run-fuzz.sh $(FUZZ_SECONDS) $(FUZZ_JOBS) $(FUZZ_NET_TARGETS)

# The room keeper ended in QEMU (Q7; gate rows keeper-restart, keeper-reconnect
# and prompt-rebind): the test image's `room crash-keeper` (feature net-test,
# proposed ADR-0298) ends the keeper under a running room; init restarts it,
# the shell reconnects and rebinds.
KEEPER_SCRIPT := tests/shell/keeper.script
test-keeper: $(NET_TEST_KERNEL) $(KERNEL) $(RUNNER) tools-storage
	@for m in $(NET_CRASH_MARKS); do f=$${m%%:*}; s=$${m#*:}; \
		if [ ! -f $(USER_DIR)/$$f ]; then echo "[keeper] FAIL: INV-0296-1: no $(USER_DIR)/$$f to look in"; exit 1; fi; \
		if grep -a -q "$$s" $(USER_DIR)/$$f; then echo "[keeper] FAIL: INV-0296-1: the image's $$f holds '$$s'"; exit 1; fi; done
	mkdir -p $(GRANTS_ROOT) && cp -R rootfs/. tests/shell/grants-root/. $(GRANTS_ROOT)/
	target/debug/zoraya-mkfs --size 16M --from $(GRANTS_ROOT) $(CRASH_IMAGE)
	$(RUNNER) --timeout $(QEMU_TIMEOUT) --script $(KEEPER_SCRIPT) --label keeper -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel auto $(NET_TEST_KERNEL)
	target/debug/zoraya-fsck $(CRASH_IMAGE)
	$(FULL_ONLY) target/debug/zoraya-mkfs --size 16M --from $(GRANTS_ROOT) $(CRASH_IMAGE) && \
		$(RUNNER) --timeout $(QEMU_SLOW_TIMEOUT) --script $(KEEPER_SCRIPT) --label keeper-tcg -- $(RUN_QEMU) --test --keys --disk $(CRASH_IMAGE) --accel tcg $(NET_TEST_KERNEL) && \
		target/debug/zoraya-fsck $(CRASH_IMAGE)

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
.PHONY: check-ci-plan fetch-ports ports check-ports clean-ports check-fetch-ports check-test-caps check-tokens check-charged-alloc check-untrusted-gate check-quota-tests check-ethd-maps check-tls-roots check-run-disk check-python-cache
fetch-ports ports check-ports clean-ports check-python-cache:
	@$(MAKE) -f ports/Makefile $@

# INV-0109-3 and INV-0109-4: the fetch step and the cache, offline.
check-fetch-ports:
	ports/test-fetch.sh
	tools/check-fetch-cache.sh

tools-storage:
	$(CARGO) build -q -p zoraya-mkfs -p zoraya-fsck
	@mkdir -p $(BUILD)

fmt-check:
	$(CARGO) fmt --all --check

clippy:
	$(CARGO) clippy --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt --all-targets -- -D warnings
	$(CARGO) build -q -p zoraya-ar
	$(CARGO) clippy -p zoraya-user -p zoraya-rt --target $(TARGET) -- -D warnings
	$(CARGO) clippy -p zoraya-user --target $(TARGET) --features net-test -- -D warnings
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
# vendor/ring/ is other people's code, with its authors' names and addresses; the
# rest of vendor/ is ours (its manifest is checked for what it holds, below).
check-host-paths: check-vendor-ring
	@id="$$(git config user.email)"; \
	home="$$(echo "$$HOME" | sed 's/[.[\*^$$]/\\&/g')"; \
	hits="$$(git grep -n -I -E "/Use[r]s/|$${home:-/nonexistent}/|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+[.][A-Za-z]{2,}" -- . \
	  ':!docs/research/' ':!tools/posix-surface/results/' ':!vendor/ring/' | { if [ -n "$$id" ]; then grep -vF "$$id"; else cat; fi; })"; \
	if [ -n "$$hits" ]; then echo "host paths or email addresses in tracked files:"; echo "$$hits"; exit 1; fi

# INV-0289-2 offline: vendor/ring matches its committed SHA-256 manifest.
.PHONY: check-vendor-ring
check-vendor-ring:
	tools/check-vendor-ring.sh

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
check-quota-tests:
	tools/check-quota-tests.sh

check-ethd-maps:
	tools/check-ethd-maps.sh

check-tls-roots:
	tools/check-tls-roots.sh

# INV-0310-2: tools/run-disk.sh and EXTRA_ROOT, on a scratch layout.
check-run-disk:
	tools/test-run-disk.sh
# Every mutant of tools/mutants/*.toml still applies: its `find` occurs in
# its file as the list says. A missing one is code that moved, or a mutant's
# `replace` that was committed by mistake (the texec failure of 9b).
# In make check over every list but four whose 15 stale mutants no longer
# apply (docs/backlog.md); MUTANT_LISTS= names other lists.
MUTANT_STALE := compositor-settings fs-limits fs-service kernel-objects
MUTANT_LISTS ?= $(filter-out tools/mutants/Cargo.toml $(MUTANT_STALE:%=tools/mutants/%.toml),$(wildcard tools/mutants/*.toml))
check-mutant-lists:
	@$(CARGO) build -q -p zoraya-mutants
	@target/debug/zoraya-mutants --check $(MUTANT_LISTS)

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
TEST_STEPS := fmt-check clippy test-unit adr-check check-user-access check-kernel-tty check-host-paths check-argv check-test-caps check-tokens check-fetch-ports test-qemu test-boot test-files test-libposix test-posix-threads test-posix-suites test-posix-threadtest3 test-posix test-posix-gui test-tour-gui test-crash test-gui test-net
TEST_STEPS += check-ci-jobs test-keeper test-hand-run test-fuzz test-fuzz-net check-charged-alloc check-untrusted-gate check-quota-tests check-ethd-maps check-tls-roots check-run-disk check-ci-plan test-apps test-netsurf test-python test-python-regr

# CI (ADR-0213's Decision): one job builds what every test needs, and every
# step of TEST_STEPS is a job of its own, test-qemu one a configuration and
# test-posix one a script (and an accelerator where needed). ci-build is the
# build job's; check-ci-jobs fails if the workflow leaves out a step, a
# configuration, a script under an accelerator, or a W8 build.
ci-build: $(KERNEL) $(TEST_KERNEL) $(NET_TEST_KERNEL) $(RUNNER) tools-storage
	@# Not -s: check-ports reads the compile commands in the ports' logs.
	$(MAKE) -f ports/Makefile ports workloads apps netsurf python
	@$(MAKE) -s python-testlib
	@$(MAKE) -s posix-image
	$(CARGO) test --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt --no-run
	$(CARGO) build -q -p zoraya-test-runner -p zoraya-elf-check -p posix-stats -p zoraya-footprint

# The docs-only rule of tools/ci-plan.sh, on a scratch repository.
check-ci-plan:
	@tools/check-ci-plan.sh

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
CHECK_STEPS := fmt-check clippy adr-check check-user-access check-kernel-tty check-host-paths check-argv check-test-caps check-tokens check-fetch-ports check-ci-jobs check-charged-alloc check-untrusted-gate check-quota-tests check-ethd-maps check-tls-roots check-mutant-lists check-run-disk check-ci-plan check-python-cache
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
