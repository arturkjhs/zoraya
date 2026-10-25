# Zoraya build and test entry points. See docs/design.md, "Where things are".
#
#   make build      build the kernel (build/zoraya-kernel.elf)
#   make run        boot it interactively (Ctrl-A X quits QEMU)
#   make test-unit  host tests (cargo test) for crates/ and tools/
#   make test-qemu  in-QEMU tests under several machine configurations
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
RUNNER := target/debug/zoraya-test-runner
RUN_QEMU := tools/run-qemu.sh
QEMU_TIMEOUT ?= 60

KERNEL_ARGS := -p zoraya-kernel --target $(TARGET)

.PHONY: all build run debug test test-unit test-qemu fmt-check clippy adr-check clean \
	$(KERNEL) $(TEST_KERNEL) $(RUNNER)

all: build

build: $(KERNEL)

# Cargo decides what to rebuild; the copies give the ELFs stable names.
$(KERNEL):
	$(CARGO) build $(KERNEL_ARGS)
	@mkdir -p $(BUILD)
	cp target/$(TARGET)/debug/zoraya-kernel $@

# The test kernel has its own target directory so the two builds do not
# overwrite each other's output.
$(TEST_KERNEL):
	$(CARGO) build $(KERNEL_ARGS) --features qemu-test --target-dir target/qemu-test
	@mkdir -p $(BUILD)
	cp target/qemu-test/$(TARGET)/debug/zoraya-kernel $@

$(RUNNER):
	$(CARGO) build -p zoraya-test-runner

run: $(KERNEL)
	$(RUN_QEMU) $(KERNEL)

debug: $(KERNEL)
	@echo "Waiting for a debugger on localhost:1234. Attach with:"
	@echo "  lldb $(KERNEL) -o 'gdb-remote localhost:1234'   or"
	@echo "  gdb-multiarch $(KERNEL) -ex 'target remote localhost:1234'"
	$(RUN_QEMU) --debug $(KERNEL)

test-unit:
	$(CARGO) test --workspace --exclude zoraya-kernel

# One runner invocation per machine configuration. "auto" is HVF on an
# Apple Silicon Mac and TCG elsewhere; the tcg lines always use TCG, so on
# a Mac both accelerators are exercised.
EXPECT := --expect-panic harness_reports_panic_as_failure \
	--expect-panic harness_reports_exception_as_failure
RUN_TEST = $(RUNNER) --timeout $(QEMU_TIMEOUT) $(EXPECT)

test-qemu: $(TEST_KERNEL) $(RUNNER)
	$(RUN_TEST) --label auto-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "cpus: 1" \
		--expect-line "console: pl011 at 0x9000000 (/pl011@9000000)" \
		-- $(RUN_QEMU) --test --accel auto $(TEST_KERNEL)
	$(RUN_TEST) --label auto-smp4-2g \
		--expect-line "cpus: 4" --expect-line "2048 MiB  RAM" \
		-- $(RUN_QEMU) --test --accel auto --smp 4 --mem 2G $(TEST_KERNEL)
	$(RUN_TEST) --label auto-el2 \
		--expect-line "exception level at entry: EL2, now EL1" \
		-- $(RUN_QEMU) --test --accel auto --machine-opt virtualization=on $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-default \
		--expect-line "exception level at entry: EL1, now EL1" --expect-line "(GICv2)" \
		-- $(RUN_QEMU) --test --accel tcg $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-el2 \
		--expect-line "exception level at entry: EL2, now EL1" --expect-line "psci: arm,psci-1.0 via smc" \
		-- $(RUN_QEMU) --test --accel tcg --machine-opt virtualization=on $(TEST_KERNEL)
	$(RUN_TEST) --label tcg-smp4-gicv3 \
		--expect-line "cpus: 4" --expect-line "(GICv3)" \
		-- $(RUN_QEMU) --test --accel tcg --smp 4 --mem 2G --machine-opt gic-version=3 $(TEST_KERNEL)

fmt-check:
	$(CARGO) fmt --all --check

clippy:
	$(CARGO) clippy --workspace --exclude zoraya-kernel --all-targets -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) -- -D warnings
	$(CARGO) clippy $(KERNEL_ARGS) --features qemu-test --target-dir target/qemu-test -- -D warnings

# ADR-0001 invariants (docs/adr/invariants.md).
adr-check:
	$(CARGO) run -q -p zoraya-adr-check -- docs

test: fmt-check clippy test-unit adr-check test-qemu
	@echo "make test: all checks passed"

clean:
	$(CARGO) clean
	rm -rf $(BUILD)
