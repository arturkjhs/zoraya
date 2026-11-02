# The ported programs and the sysroot (ADR-0109 item 6). Run from the
# repository's root by the top Makefile:
#
#   make fetch-ports   download every archive of ports/sources.lock into the
#                      cache build/ports/src/ (the only step with network)
#   make ports         build build/sysroot and build/rootfs-posix from the
#                      cache, then run the host checks over them
#   make check-ports   the host checks alone
#
# Nothing here downloads: an archive missing from the cache stops the build
# with a message naming `make fetch-ports` (INV-0109-3). Works with GNU make
# 3.81 (macOS) and later.

CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
RUSTC ?= $(shell command -v rustc 2>/dev/null || echo $(HOME)/.cargo/bin/rustc)
CLANG ?= clang
HOSTCC ?= cc

ROOT := $(shell pwd)
PORTS := $(ROOT)/build/ports
CACHE := $(PORTS)/src
WORK := $(PORTS)/work
OBJ := $(PORTS)/obj
LOGS := $(PORTS)/logs
SYSROOT := $(ROOT)/build/sysroot
ROOTFS := $(ROOT)/build/rootfs-posix
LOCK := ports/sources.lock

RUST_SYSROOT := $(shell $(RUSTC) --print sysroot)
RUST_HOST := $(shell $(RUSTC) -vV | sed -n 's/^host: //p')
RUST_BIN := $(RUST_SYSROOT)/lib/rustlib/$(RUST_HOST)/bin
LLD := $(RUST_BIN)/gcc-ld/ld.lld

export ZORAYA_SYSROOT := $(SYSROOT)
export ZORAYA_CLANG := $(CLANG)
export ZORAYA_LD := $(LLD)
ZCC := $(ROOT)/tools/zcc
ZAR := $(ROOT)/target/debug/zoraya-ar
SURFACE_CHECK := $(ROOT)/target/debug/posix-surface-check
ELF_CHECK := $(ROOT)/target/debug/zoraya-elf-check

# name version file (the columns of sources.lock that the build needs)
lock_file = $(shell awk '$$1 == "$(1)" { print $$4 }' $(LOCK))
lock_ver = $(shell awk '$$1 == "$(1)" { print $$2 }' $(LOCK))

MUSL_VER := $(call lock_ver,musl)
MUSL := $(WORK)/musl-$(MUSL_VER)

.PHONY: ports fetch-ports check-ports sysroot musl headers tools stub
.SUFFIXES:

fetch-ports:
	tools/fetch-ports.sh $(LOCK) $(CACHE)

ports: sysroot

tools:
	$(CARGO) build -q -p zoraya-ar -p posix-surface-check -p zoraya-elf-check

# ---------------------------------------------------------------- sources

# Unpacks one archive of the cache into $(WORK) and applies the patches of
# its patch directory in name order. $(1) is the name in sources.lock, $(2)
# the patch directory.
define unpack
	@f=$(CACHE)/$(call lock_file,$(1)); \
	if [ ! -f "$$f" ]; then \
	  echo "ports: $$f is missing: run 'make fetch-ports' first (it is the only step that downloads)"; \
	  exit 1; fi
	@tools/fetch-ports.sh --verify $(LOCK) $(CACHE) $(1)
	rm -rf $(WORK)/$(1).tmp && mkdir -p $(WORK)/$(1).tmp
	cd $(WORK)/$(1).tmp && tar xf $(CACHE)/$(call lock_file,$(1))
	rm -rf $@ && mv $(WORK)/$(1).tmp/* $@ && rmdir $(WORK)/$(1).tmp
	@for p in $(sort $(wildcard $(2)/*.patch)); do \
	  echo "patch $$p"; patch -s -p1 -d $@ < $$p || exit 1; done
endef

$(MUSL): $(LOCK) $(wildcard ports/musl/zoraya/*.patch) ports/musl/zoraya/syscall.h
	$(call unpack,musl,ports/musl/zoraya)
	cp ports/musl/zoraya/syscall.h $@/src/internal/syscall.h
	@touch $@

# ---------------------------------------------------------------- headers

# musl's install-headers: include/, then arch/generic/bits overridden by
# arch/aarch64/bits, the generated bits/alltypes.h, and a bits/syscall.h with
# no Linux numbers in it (ADR-0109 item 2).
HEADERS_STAMP := $(SYSROOT)/usr/include/.stamp
headers: $(HEADERS_STAMP)
$(HEADERS_STAMP): $(MUSL)
	rm -rf $(SYSROOT)/usr/include && mkdir -p $(SYSROOT)/usr/include/bits
	cp -R $(MUSL)/include/. $(SYSROOT)/usr/include/
	rm -f $(SYSROOT)/usr/include/alltypes.h.in
	cp $(MUSL)/arch/generic/bits/*.h $(SYSROOT)/usr/include/bits/
	cp $(MUSL)/arch/aarch64/bits/*.h $(SYSROOT)/usr/include/bits/
	sed -f $(MUSL)/tools/mkalltypes.sed $(MUSL)/arch/aarch64/bits/alltypes.h.in \
	  $(MUSL)/include/alltypes.h.in > $(SYSROOT)/usr/include/bits/alltypes.h
	printf '/* Zoraya: no Linux system call numbers (ADR-0109 item 2). */\n' \
	  > $(SYSROOT)/usr/include/bits/syscall.h
	@touch $@

# ---------------------------------------------------------------- musl

MUSL_LIST := $(OBJ)/musl/objects.txt
musl: $(MUSL_LIST)
$(MUSL_LIST): $(HEADERS_STAMP) ports/musl/musl.mk ports/musl/excluded.txt FORCE
	$(MAKE) -f ports/musl/musl.mk MUSL=$(MUSL) OBJ=$(OBJ)/musl ZCC=$(ZCC) LIST=$@

FORCE:

# ---------------------------------------------------------------- the stub

# zoraya-posix until TASK 6c writes it (ADR-0140): generated from
# ports/posix-stub/stubs.txt, built as a static library of the pinned Rust
# toolchain, which also brings compiler_builtins (the long double arithmetic
# musl's printf and strtod need).
STUB := $(OBJ)/posix-stub
STUB_LIB := $(STUB)/libzoraya_posix.a
stub: $(STUB_LIB)
$(STUB_LIB): ports/posix-stub/stubs.txt ports/posix-stub/stubs.awk
	@mkdir -p $(STUB)
	awk -f ports/posix-stub/stubs.awk ports/posix-stub/stubs.txt > $(STUB)/lib.rs
	$(RUSTC) --edition 2024 --crate-type staticlib --crate-name zoraya_posix \
	  --target aarch64-unknown-none -C panic=abort -C opt-level=2 -C debuginfo=0 \
	  -C codegen-units=1 --remap-path-prefix=$(ROOT)/= $(STUB)/lib.rs -o $@

# ---------------------------------------------------------------- libc.a

# musl's objects first, then the stub's members, each with the names outside
# the tier list hidden (ADR-0141): posix-surface-check renames them in every
# object and writes the list of renames.
LIBC_DIR := $(OBJ)/libc
TIER_LIST := docs/adr/posix-surface-T1.txt
$(SYSROOT)/usr/lib/libc.a: $(MUSL_LIST) $(STUB_LIB) $(TIER_LIST) tools
	rm -rf $(LIBC_DIR) && mkdir -p $(LIBC_DIR)/stub $(LIBC_DIR)/out
	cd $(LIBC_DIR)/stub && $(ZAR) x $(STUB_LIB)
	$(SURFACE_CHECK) hide $(TIER_LIST) $(LIBC_DIR)/out $$(cat $(MUSL_LIST)) \
	  $$(for m in $$($(ZAR) t $(STUB_LIB)); do echo $(LIBC_DIR)/stub/$$m; done) > $(LIBC_DIR)/hidden.txt
	@echo "hid $$(wc -l < $(LIBC_DIR)/hidden.txt | tr -d ' ') names: $(LIBC_DIR)/hidden.txt"
	@mkdir -p $(dir $@)
	rm -f $@ && cd $(LIBC_DIR)/out && $(ZAR) rcs $@ \
	  $$(for o in $$(cat $(MUSL_LIST)); do basename $$o; done) $$($(ZAR) t $(STUB_LIB))

# ---------------------------------------------------------------- crt

CRT_OBJS := $(addprefix $(SYSROOT)/usr/lib/,crt1.o crti.o crtn.o)
MUSL_CFLAGS = -std=c99 -ffreestanding -D_XOPEN_SOURCE=700 -O2 -g \
	-I$(MUSL)/arch/aarch64 -I$(MUSL)/arch/generic -I$(MUSL)/src/include \
	-I$(MUSL)/src/internal -I$(MUSL)/include

# crt1.o: ports/crt/crt1.S's _start and musl's crt/crt1.c's _start_c, with
# musl's own _start left out by an empty crt_arch.h.
$(SYSROOT)/usr/lib/crt1.o: ports/crt/crt1.S ports/musl/zoraya/crt_arch.h $(HEADERS_STAMP)
	@mkdir -p $(OBJ)/crt $(dir $@)
	$(ZCC) -c -o $(OBJ)/crt/crt1-start.o ports/crt/crt1.S
	$(ZCC) -Iports/musl/zoraya $(MUSL_CFLAGS) -c -o $(OBJ)/crt/crt1-c.o $(MUSL)/crt/crt1.c
	$(LLD) -r -o $@ $(OBJ)/crt/crt1-start.o $(OBJ)/crt/crt1-c.o

$(SYSROOT)/usr/lib/crti.o $(SYSROOT)/usr/lib/crtn.o: $(SYSROOT)/usr/lib/%.o: $(HEADERS_STAMP)
	@mkdir -p $(dir $@)
	$(ZCC) -c -o $@ $(MUSL)/crt/aarch64/$*.s

# musl's convention: the libraries a program may name are in libc.a, and
# these are empty so that -lm and the others resolve (ADR-0109 item 2).
EMPTY_LIBS := $(addprefix $(SYSROOT)/usr/lib/lib,$(addsuffix .a,m rt pthread crypt util xnet resolv dl))
$(EMPTY_LIBS): tools
	@mkdir -p $(dir $@)
	rm -f $@ && $(ZAR) rcs $@

# ---------------------------------------------------------------- the sysroot

sysroot: $(SYSROOT)/usr/lib/libc.a $(CRT_OBJS) $(EMPTY_LIBS)
