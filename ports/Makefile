# The ported programs and the sysroot (ADR-0109 item 6). Run from the
# repository's root through the top Makefile:
#
#   make fetch-ports   download every archive of ports/sources.lock into the
#                      cache build/ports/src/ (the only step with network)
#   make ports         build build/sysroot and build/rootfs-posix from the
#                      cache, then run the host checks over them
#   make check-ports   the host checks alone, over what `make ports` built
#
# Nothing here downloads: an archive missing from the cache stops the build
# with a message naming `make fetch-ports` (INV-0109-3). Each port's build
# output is in build/ports/logs/. Works with GNU make 3.81 (macOS) and later.

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
$(shell mkdir -p $(LOGS))

RUST_SYSROOT := $(shell $(RUSTC) --print sysroot)
RUST_HOST := $(shell $(RUSTC) -vV | sed -n 's/^host: //p')
LLD := $(RUST_SYSROOT)/lib/rustlib/$(RUST_HOST)/bin/gcc-ld/ld.lld

export ZORAYA_SYSROOT := $(SYSROOT)
export ZORAYA_CLANG := $(CLANG)
export ZORAYA_LD := $(LLD)
ZCC := $(ROOT)/tools/zcc
ZAR := $(ROOT)/target/debug/zoraya-ar
SURFACE_CHECK := $(ROOT)/target/debug/posix-surface-check
ELF_CHECK := $(ROOT)/target/debug/zoraya-elf-check
TIER_LIST := docs/adr/posix-surface-T1.txt
# The names libc.a may export: the generated list, and the blocks of the ADRs
# that add names (posix-surface-check's LIST+ADR form).
ADDED_ADRS := docs/adr/0142-surface-for-dash-job-control.md docs/adr/0144-surface-for-sort-and-wc.md \
	docs/adr/0210-surface-for-the-workloads-products.md docs/adr/0287-n4-posix-sockets-where-the-adrs-are-silent.md
empty :=
space := $(empty) $(empty)
TIER_SPEC := $(subst $(space),+,$(TIER_LIST) $(ADDED_ADRS))

# Columns of sources.lock: name version sha256 file url.
lock_file = $(shell awk '$$1 == "$(1)" { print $$4 }' $(LOCK))
lock_ver = $(shell awk '$$1 == "$(1)" { print $$2 }' $(LOCK))

# Runs a command with its output in $(LOGS)/$(2).log; on failure, says so and
# shows the log's end.
logged = $(1) > $(LOGS)/$(2).log 2>&1 || { echo "ports: $(2) failed: $(LOGS)/$(2).log"; tail -40 $(LOGS)/$(2).log; exit 1; }
# The same, appending: for a step that runs on every build and may then have
# nothing to do (musl), so that its log keeps the compile commands.
logged_append = $(1) >> $(LOGS)/$(2).log 2>&1 || { echo "ports: $(2) failed: $(LOGS)/$(2).log"; tail -40 $(LOGS)/$(2).log; exit 1; }

.PHONY: ports fetch-ports check-ports check-libc sysroot libc rootfs tools musl headers posix \
	dash make pdpmake toybox busybox tcc tcc-host workloads t2-probe clean-ports FORCE
.SUFFIXES:

fetch-ports:
	tools/fetch-ports.sh $(LOCK) $(CACHE)

ports: rootfs
	@echo "ports: built with $$($(CLANG) --version | head -1) and $$($(LLD) --version)"
	@$(MAKE) -f ports/Makefile check-ports

clean-ports:
	rm -rf $(WORK) $(OBJ) $(LOGS) $(PORTS)/bin $(SYSROOT) $(ROOTFS)

# The host tools. `tools` always asks cargo, which leaves an unchanged binary
# alone; a rule that uses a tool's output (libc.a) depends on the binary, so
# a changed tool rebuilds it, and the others take the tools order-only.
tools:
	$(CARGO) build -q -p zoraya-ar -p posix-surface-check -p zoraya-elf-check
$(ZAR) $(SURFACE_CHECK) $(ELF_CHECK): tools

FORCE:

# ---------------------------------------------------------------- sources

# Unpacks one archive of the cache into $@ and applies the patches of its
# patch directory in name order. $(1) is the name in sources.lock, $(2) the
# patch directory.
define unpack
	@f=$(CACHE)/$(call lock_file,$(1)); \
	if [ ! -f "$$f" ]; then \
	  echo "ports: $$f is missing: run 'make fetch-ports' first (it is the only step that downloads)"; \
	  exit 1; fi
	@tools/fetch-ports.sh --verify $(LOCK) $(CACHE) $(1)
	rm -rf $@.tmp && mkdir -p $@.tmp
	@# SQLite's amalgamation is a zip, which GNU tar does not read.
	cd $@.tmp && case $(call lock_file,$(1)) in *.zip) unzip -q $(CACHE)/$(call lock_file,$(1));; \
	  *) tar xf $(CACHE)/$(call lock_file,$(1));; esac
	rm -rf $@ && mv $@.tmp/* $@ && rmdir $@.tmp
	@for p in $(sort $(wildcard $(2)/*.patch)); do \
	  echo "patch $$p"; patch -s -p1 -d $@ < $$p || exit 1; done
endef

MUSL := $(WORK)/musl-$(call lock_ver,musl)
$(MUSL): $(LOCK) $(wildcard ports/musl/zoraya/*.patch) ports/musl/zoraya/syscall.h
	$(call unpack,musl,ports/musl/zoraya)
	cp ports/musl/zoraya/syscall.h $@/src/internal/syscall.h
	rm -rf $(OBJ)/musl $(LOGS)/musl.log
	@touch $@

# ---------------------------------------------------------------- headers

# musl's install-headers: include/, then arch/generic/bits overridden by
# arch/aarch64/bits, the generated bits/alltypes.h, and a bits/syscall.h with
# no Linux numbers in it (ADR-0109 item 2).
HEADERS_STAMP := $(OBJ)/headers.stamp
headers: $(HEADERS_STAMP)
$(HEADERS_STAMP): $(MUSL) $(wildcard ports/include/*/*.h)
	rm -rf $(SYSROOT)/usr/include && mkdir -p $(SYSROOT)/usr/include/bits
	cp -R $(MUSL)/include/. $(SYSROOT)/usr/include/
	rm -f $(SYSROOT)/usr/include/alltypes.h.in
	cp $(MUSL)/arch/generic/bits/*.h $(SYSROOT)/usr/include/bits/
	cp $(MUSL)/arch/aarch64/bits/*.h $(SYSROOT)/usr/include/bits/
	sed -f $(MUSL)/tools/mkalltypes.sed $(MUSL)/arch/aarch64/bits/alltypes.h.in \
	  $(MUSL)/include/alltypes.h.in > $(SYSROOT)/usr/include/bits/alltypes.h
	printf '/* Zoraya: no Linux system call numbers (ADR-0109 item 2). */\n' \
	  > $(SYSROOT)/usr/include/bits/syscall.h
	cp -R ports/include/. $(SYSROOT)/usr/include/
	@mkdir -p $(dir $@) && touch $@

# ---------------------------------------------------------------- musl

MUSL_LIST := $(OBJ)/musl/objects.txt
musl: $(MUSL_LIST)
$(MUSL_LIST): $(HEADERS_STAMP) ports/musl/musl.mk ports/musl/excluded.txt FORCE
	@$(call logged_append,$(MAKE) -f ports/musl/musl.mk MUSL=$(MUSL) OBJ=$(OBJ)/musl ZCC=$(ZCC) LIST=$@,musl)

# ---------------------------------------------------------------- zoraya-posix

# The Rust half of ADR-0100 (crates/zoraya-posix), built by the pinned Rust
# toolchain as a static library, which also brings compiler_builtins (the
# long double arithmetic musl's printf and strtod need). Cargo decides what
# to rebuild; its own target directory keeps these flags apart from the
# workspace's. One codegen unit per crate keeps zoraya-rt's `svc` in the one
# member INV-0100-2's scan permits.
POSIX_TARGET := $(ROOT)/target/posix
# The member of libc.a that may hold `svc` (INV-0100-2): zoraya-rt's object.
SVC_MEMBER := zoraya_rt-
POSIX_LIB := $(POSIX_TARGET)/aarch64-unknown-none/release/libzoraya_posix.a
# ADR-0110 item 7, ADR-0211: the `stats` feature and the call log are
# on in the image the stage 6 tests boot, which is the only one; POSIX_STATS=0
# builds libc.a without either.
POSIX_STATS ?= 1
POSIX_FEATURES := $(if $(filter 1,$(POSIX_STATS)),--features stats)
posix: $(POSIX_LIB)
$(POSIX_LIB): FORCE
	RUSTFLAGS="--remap-path-prefix=$(ROOT)=. -C codegen-units=1" CARGO_PROFILE_RELEASE_DEBUG=0 \
	  $(CARGO) rustc -q -p zoraya-posix --release --target aarch64-unknown-none \
	  --target-dir $(POSIX_TARGET) --crate-type staticlib $(POSIX_FEATURES)

# ---------------------------------------------------------------- libc.a

# musl's objects first, then zoraya-posix's members, each with the names outside
# the tier list hidden (ADR-0141): posix-surface-check renames them in every
# object and writes the list of renames.
LIBC := $(SYSROOT)/usr/lib/libc.a
LIBC_DIR := $(OBJ)/libc
# The call log's order: function i of the bitmap is line i (ADR-0110 item 7);
# tools/posix-stats decodes POSIX-CALLS with it.
LIBC_CALLS := $(SYSROOT)/usr/lib/zoraya-calls.txt
$(LIBC): $(MUSL_LIST) $(POSIX_LIB) $(TIER_LIST) $(ADDED_ADRS) $(ZAR) $(SURFACE_CHECK)
	rm -rf $(LIBC_DIR) && mkdir -p $(LIBC_DIR)/stub $(LIBC_DIR)/out
	cd $(LIBC_DIR)/stub && $(ZAR) x $(POSIX_LIB)
	$(SURFACE_CHECK) hide $(TIER_SPEC) $(LIBC_DIR)/out $$(cat $(MUSL_LIST)) \
	  $$(for m in $$($(ZAR) t $(POSIX_LIB)); do echo $(LIBC_DIR)/stub/$$m; done) > $(LIBC_DIR)/hidden.txt
	@echo "libc.a: hid $$(wc -l < $(LIBC_DIR)/hidden.txt | tr -d ' ') names outside the tier list: $(LIBC_DIR)/hidden.txt"
	@mkdir -p $(dir $@)
ifeq ($(POSIX_STATS),1)
	@# The call log: each exported function renamed to __zoraya_real_NAME in
	@# every member, a wrapper member zw-NAME.o in its place, the bitmap in
	@# zoraya-calls.o.
	mkdir -p $(LIBC_DIR)/logged $(LIBC_DIR)/calls
	$(SURFACE_CHECK) calllog $(TIER_SPEC) $(LIBC_DIR)/logged $(LIBC_DIR)/calls \
	  $$(for o in $$(cat $(MUSL_LIST)); do echo $(LIBC_DIR)/out/$$(basename $$o); done) \
	  $$(for m in $$($(ZAR) t $(POSIX_LIB)); do echo $(LIBC_DIR)/out/$$m; done) > $(LIBC_DIR)/calls.txt
	cd $(LIBC_DIR)/calls && for f in *.S; do echo $$f; done | sed 's/\.S$$//' | \
	  xargs -P 8 -I{} sh -c '$(ZCC) -c -o zw-{}.o {}.S' && mv zw-zoraya-calls.o zoraya-calls.o
	@echo "libc.a: the call log wraps $$(wc -l < $(LIBC_DIR)/calls.txt | tr -d ' ') functions: $(LIBC_DIR)/calls.txt"
	rm -f $@ && cd $(LIBC_DIR)/logged && $(ZAR) rcs $@ \
	  $$(for o in $$(cat $(MUSL_LIST)); do basename $$o; done) $$($(ZAR) t $(POSIX_LIB)) \
	  $$(cd ../calls && ls zw-*.o zoraya-calls.o | sed 's|^|../calls/|')
	cp $(LIBC_DIR)/calls.txt $(LIBC_CALLS)
else
	rm -f $@ $(LIBC_CALLS) && cd $(LIBC_DIR)/out && $(ZAR) rcs $@ \
	  $$(for o in $$(cat $(MUSL_LIST)); do basename $$o; done) $$($(ZAR) t $(POSIX_LIB))
endif

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
$(EMPTY_LIBS): | tools
	@mkdir -p $(dir $@)
	rm -f $@ && $(ZAR) rcs $@

# ---------------------------------------------------------------- common

# Programs built for the host see Zoraya's signal numbers through this header.
TARGET_SIGNALS := $(OBJ)/target-signals.h
$(TARGET_SIGNALS): ports/target-signals.sh $(HEADERS_STAMP)
	@mkdir -p $(dir $@)
	ports/target-signals.sh $(HOSTCC) $(ZCC) > $@.tmp && mv $@.tmp $@

# The cross tools under their triple-prefixed names, first on PATH for the
# ports' builds, so that configure finds them as autoconf expects
# (aarch64-unknown-zoraya-cc, -ar, -ranlib, -ld) and nothing of the host's.
TOOLBIN := $(PORTS)/bin
TOOLS_STAMP := $(TOOLBIN)/.stamp
$(TOOLS_STAMP): tools/zcc | tools
	rm -rf $(TOOLBIN) && mkdir -p $(TOOLBIN)
	ln -s $(ZCC) $(TOOLBIN)/aarch64-unknown-zoraya-cc
	ln -s $(ZAR) $(TOOLBIN)/aarch64-unknown-zoraya-ar
	ln -s $(ZAR) $(TOOLBIN)/aarch64-unknown-zoraya-ranlib
	printf '#!/bin/sh\nexec "%s" "$$@"\n' $(LLD) > $(TOOLBIN)/aarch64-unknown-zoraya-ld
	chmod +x $(TOOLBIN)/aarch64-unknown-zoraya-ld
	@touch $@

# What every port links against.
SYSROOT_STAMP := $(OBJ)/sysroot.stamp
$(SYSROOT_STAMP): $(LIBC) $(CRT_OBJS) $(EMPTY_LIBS) $(TOOLS_STAMP)
	@touch $@

CROSS_ENV := CONFIG_SITE=$(ROOT)/ports/config.site PATH=$(TOOLBIN):$$PATH
CROSS_TOOLS := CC=$(ZCC) AR=$(TOOLBIN)/aarch64-unknown-zoraya-ar \
	RANLIB=$(TOOLBIN)/aarch64-unknown-zoraya-ranlib
CROSS_CFLAGS := -O2 -g

# Turns the options of a Kconfig .config on (toybox, busybox); BSD sed and
# GNU sed both run this.
kconfig_on = for a in $(2); do u=$$(echo "$$a" | tr 'a-z-' 'A-Z_'); \
	sed -e "s/^\# CONFIG_$$u is not set/CONFIG_$$u=y/" $(1)/.config > $(1)/.config.new \
	&& mv $(1)/.config.new $(1)/.config || exit 1; done

# ---------------------------------------------------------------- dash

DASH := $(WORK)/dash-$(call lock_ver,dash)
DASH_BIN := $(DASH)/src/dash
dash: $(DASH_BIN)
$(DASH): $(LOCK) $(wildcard ports/patches/dash/*.patch)
	$(call unpack,dash,ports/patches/dash)
	@touch $@

# dash's helpers (mksignames and the others) run on the host, with Zoraya's
# signal numbers; configure needs no config.sub (it does not canonicalise
# the host).
$(DASH_BIN): $(DASH) $(SYSROOT_STAMP) $(TARGET_SIGNALS) ports/config.site
	@cd $(DASH) && $(call logged,$(CROSS_ENV) ./configure --host=aarch64-unknown-zoraya \
	  $(CROSS_TOOLS) CFLAGS="$(CROSS_CFLAGS)" \
	  CC_FOR_BUILD="$(HOSTCC) -include $(TARGET_SIGNALS)",dash-configure)
	rm -f $(DASH_BIN)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(DASH) V=1,dash)
	@touch $@

# ---------------------------------------------------------------- GNU make

GMAKE := $(WORK)/make-$(call lock_ver,make)
GMAKE_BIN := $(GMAKE)/make
make: $(GMAKE_BIN)
$(GMAKE): $(LOCK) $(wildcard ports/patches/make/*.patch)
	$(call unpack,make,ports/patches/make)
	@touch $@

# As ADR-0109 item 6 says: no `load` (so no dlopen), no Guile; no NLS either,
# since Zoraya has one locale and the measured build had no catalogues.
$(GMAKE_BIN): $(GMAKE) $(SYSROOT_STAMP) ports/config.site
	@cd $(GMAKE) && $(call logged,$(CROSS_ENV) ./configure --host=aarch64-unknown-zoraya \
	  --disable-load --without-guile --disable-nls \
	  $(CROSS_TOOLS) CFLAGS="$(CROSS_CFLAGS)",make-configure)
	rm -f $(GMAKE_BIN)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(GMAKE) V=1,make)
	@touch $@

# ---------------------------------------------------------------- pdpmake

# pdpmake, TASK-02's comparison make (W2 and W4 run with it, ADR-0110): its
# own Makefile, unpatched; recipes go through system(), ADR-0210.
PDPMAKE := $(WORK)/pdpmake
PDPMAKE_BIN := $(PDPMAKE)/make
pdpmake: $(PDPMAKE_BIN)
$(PDPMAKE): $(LOCK) $(wildcard ports/patches/pdpmake/*.patch)
	$(call unpack,pdpmake,ports/patches/pdpmake)
	@touch $@
$(PDPMAKE_BIN): $(PDPMAKE) $(SYSROOT_STAMP)
	rm -f $(PDPMAKE_BIN)
	@$(call logged,$(MAKE) -C $(PDPMAKE) make CC=$(ZCC) CFLAGS="$(CROSS_CFLAGS)",pdpmake)
	@touch $@

# ---------------------------------------------------------------- toybox

# The T1 applets of applets-by-tier.tsv, plus sort, wc and yes (ADR-0109
# item 6), from allnoconfig as TASK-02's minimal builds were.
TOYBOX_APPLETS := cat head ls mv rm sleep touch true uname sort wc yes
TOYBOX := $(WORK)/toybox-$(call lock_ver,toybox)
TOYBOX_BIN := $(TOYBOX)/toybox
toybox: $(TOYBOX_BIN)
$(TOYBOX): $(LOCK) $(wildcard ports/patches/toybox/*.patch)
	$(call unpack,toybox,ports/patches/toybox)
	@touch $@

# toybox's scripts need GNU sed, which macOS does not have as `sed`.
GSED := $(shell command -v gsed 2>/dev/null || { sed --version 2>/dev/null | grep -q GNU && command -v sed; })

# On a macOS host toybox's scripts pick Mach-O link options and strip; they
# are given the ELF ones, and the binary is not stripped (-g as measured).
$(TOYBOX_BIN): $(TOYBOX) $(SYSROOT_STAMP)
	@if [ -z "$(GSED)" ]; then echo "ports: toybox's build needs GNU sed, which this host lacks." \
	  "On macOS: brew install gnu-sed (it installs gsed, which toybox's scripts use)."; exit 1; fi
	@$(call logged,$(MAKE) -C $(TOYBOX) allnoconfig HOSTCC=$(HOSTCC) SED=$(GSED),toybox-config)
	@$(call kconfig_on,$(TOYBOX),$(TOYBOX_APPLETS))
	@yes '' | $(MAKE) -C $(TOYBOX) oldconfig HOSTCC=$(HOSTCC) SED=$(GSED) > /dev/null 2>&1
	rm -f $(TOYBOX_BIN)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(TOYBOX) toybox V=1 CC=$(ZCC) HOSTCC=$(HOSTCC) \
	  CFLAGS="$(CROSS_CFLAGS)" SED=$(GSED) LDOPTIMIZE=-Wl$(comma)--gc-sections NOSTRIP=1,toybox)
	@touch $@
comma := ,

# ---------------------------------------------------------------- busybox

BUSYBOX_APPLETS := tr diff
BUSYBOX := $(WORK)/busybox-$(call lock_ver,busybox)
BUSYBOX_BIN := $(BUSYBOX)/busybox
busybox: $(BUSYBOX_BIN)
$(BUSYBOX): $(LOCK) $(wildcard ports/patches/busybox/*.patch)
	$(call unpack,busybox,ports/patches/busybox)
	@touch $@

# allnoconfig with the shell choice forced to none, as TASK-02's minimal
# builds did, then tr and diff. KCONFIG_NOTIMESTAMP keeps the build time out
# of the binary; SKIP_STRIP keeps the debug information.
$(BUSYBOX_BIN): $(BUSYBOX) $(SYSROOT_STAMP)
	@$(call logged,$(MAKE) -C $(BUSYBOX) allnoconfig HOSTCC=$(HOSTCC) KCONFIG_NOTIMESTAMP=1,busybox-config)
	@sed -e 's/^CONFIG_SH_IS_ASH=y/# CONFIG_SH_IS_ASH is not set/' \
	  -e 's/^# CONFIG_SH_IS_NONE is not set/CONFIG_SH_IS_NONE=y/' \
	  $(BUSYBOX)/.config > $(BUSYBOX)/.config.new && mv $(BUSYBOX)/.config.new $(BUSYBOX)/.config
	@$(call kconfig_on,$(BUSYBOX),$(BUSYBOX_APPLETS))
	@yes '' | $(MAKE) -C $(BUSYBOX) oldconfig HOSTCC=$(HOSTCC) KCONFIG_NOTIMESTAMP=1 > /dev/null 2>&1
	rm -f $(BUSYBOX_BIN) $(BUSYBOX)/busybox_unstripped
	@$(call logged,$(CROSS_ENV) KCONFIG_NOTIMESTAMP=1 $(MAKE) -C $(BUSYBOX) V=1 HOSTCC=$(HOSTCC) \
	  CC=$(ZCC) AR=$(TOOLBIN)/aarch64-unknown-zoraya-ar LD=$(TOOLBIN)/aarch64-unknown-zoraya-ld \
	  SKIP_STRIP=y,busybox)
	@touch $@

# ---------------------------------------------------------------- tcc

# tcc for Zoraya (ADR-0109 items 6 and 7): its paths in Zoraya, static links
# always (the patch's configure case), libtcc1.a in its own directory, which
# is why the library path starts with {B}, tcc's directory.
TCC_CONFIG = --cpu=aarch64 --targetos=Zoraya --prefix=/usr --os-release=0 --elfinterp=
TCC_PATHS = --tccdir=$(1)/usr/lib/tcc --crtprefix=$(1)/usr/lib \
	--sysincludepaths=$(1)/usr/include:$(1)/usr/lib/tcc/include --libpaths={B}:$(1)/usr/lib
TCC_DIR := $(WORK)/tcc
TCC_BIN := $(TCC_DIR)/tcc
LIBTCC1 := $(TCC_DIR)/libtcc1.a
TCC_STAMP := $(OBJ)/tcc.stamp
tcc: $(TCC_STAMP)
$(TCC_DIR): $(LOCK) $(wildcard ports/patches/tcc/*.patch)
	$(call unpack,tcc,ports/patches/tcc)
	@touch $@

# tcc's c2str runs on the host, so it is built first with the host's
# compiler. GITHASH=no: this checkout's git must not name tcc's version.
$(TCC_STAMP): $(TCC_DIR) $(SYSROOT_STAMP)
	@cd $(TCC_DIR) && $(call logged,./configure $(TCC_CONFIG) $(call TCC_PATHS,) \
	  --cc=$(ZCC) --ar=$(TOOLBIN)/aarch64-unknown-zoraya-ar --extra-cflags="$(CROSS_CFLAGS)",tcc-configure)
	cd $(TCC_DIR) && $(HOSTCC) -DC2STR conftest.c -o c2str.exe
	rm -f $(TCC_BIN) $(LIBTCC1)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(TCC_DIR) tcc GITHASH=no,tcc)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(TCC_DIR) libtcc1.a arm64-libtcc1-usegcc=yes GITHASH=no,libtcc1)
	@touch $@

# The same source, patches and configuration, compiled by the host's compiler
# with the sysroot as its root: the Zoraya tcc cannot run on the host, so
# this one links the hello world of the host check (ADR-0109 item 7).
TCC_HOST_DIR := $(WORK)/tcc-host
TCC_HOST := $(TCC_HOST_DIR)/tcc
tcc-host: $(TCC_HOST)
$(TCC_HOST_DIR): $(LOCK) $(wildcard ports/patches/tcc/*.patch)
	$(call unpack,tcc,ports/patches/tcc)
	@touch $@
$(TCC_HOST): $(TCC_HOST_DIR)
	@cd $(TCC_HOST_DIR) && $(call logged,./configure $(TCC_CONFIG) $(call TCC_PATHS,$(SYSROOT)) \
	  --cc=$(HOSTCC),tcc-host-configure)
	cd $(TCC_HOST_DIR) && $(HOSTCC) -DC2STR conftest.c -o c2str.exe
	@$(call logged,$(MAKE) -C $(TCC_HOST_DIR) tcc GITHASH=no,tcc-host)
	@touch $@

# ---------------------------------------------------------------- sysroot and root file system

TCC_SYSROOT := $(SYSROOT)/usr/lib/tcc/libtcc1.a
$(TCC_SYSROOT): $(TCC_STAMP)
	rm -rf $(SYSROOT)/usr/lib/tcc && mkdir -p $(SYSROOT)/usr/lib/tcc/include
	cp $(LIBTCC1) $@
	cp $(TCC_DIR)/include/*.h $(SYSROOT)/usr/lib/tcc/include/

sysroot: $(SYSROOT_STAMP) $(TCC_SYSROOT)

# libc.a and the crt objects alone: what a C program built with zcc needs
# (make test-libposix).
libc: $(SYSROOT_STAMP)

# build/rootfs-posix (ADR-0109 item 6): the programs in /bin, toybox and
# busybox with a link per applet (ADR-0108 item 6), the sysroot as /usr.
ROOTFS_STAMP := $(OBJ)/rootfs.stamp
rootfs: $(ROOTFS_STAMP)
$(ROOTFS_STAMP): $(DASH_BIN) $(GMAKE_BIN) $(PDPMAKE_BIN) $(TOYBOX_BIN) $(BUSYBOX_BIN) $(TCC_STAMP) $(TCC_SYSROOT) $(SYSROOT_STAMP)
	rm -rf $(ROOTFS) && mkdir -p $(ROOTFS)/bin $(ROOTFS)/usr
	cp $(DASH_BIN) $(ROOTFS)/bin/dash && ln -s dash $(ROOTFS)/bin/sh
	cp $(GMAKE_BIN) $(ROOTFS)/bin/make
	cp $(PDPMAKE_BIN) $(ROOTFS)/bin/pdpmake
	cp $(TCC_BIN) $(ROOTFS)/bin/tcc && ln -s tcc $(ROOTFS)/bin/cc
	cp $(TOYBOX_BIN) $(ROOTFS)/bin/toybox
	for a in $(TOYBOX_APPLETS); do ln -s toybox $(ROOTFS)/bin/$$a; done
	cp $(BUSYBOX_BIN) $(ROOTFS)/bin/busybox
	for a in $(BUSYBOX_APPLETS); do ln -s busybox $(ROOTFS)/bin/$$a; done
	cp -R $(SYSROOT)/usr/include $(SYSROOT)/usr/lib $(ROOTFS)/usr/
	@touch $@

# ---------------------------------------------------------------- the workloads' sources

# The trees the acceptance workloads build inside Zoraya (ADR-0110 item 1),
# as sources.lock pins them, in build/ports/workloads/: tcc (with the ports'
# patches, for its Zoraya target, ADR-0109 item 8), Lua, SQLite's
# amalgamation and GNU hello (W4). Nothing is built here.
WORKLOADS := $(PORTS)/workloads
WORKLOADS_STAMP := $(OBJ)/workloads.stamp
workloads: $(WORKLOADS_STAMP)
$(WORKLOADS)/tcc: $(LOCK) $(wildcard ports/patches/tcc/*.patch)
	@mkdir -p $(WORKLOADS)
	$(call unpack,tcc,ports/patches/tcc)
	@touch $@
$(WORKLOADS)/lua: $(LOCK)
	@mkdir -p $(WORKLOADS)
	$(call unpack,lua,ports/patches/lua)
	@touch $@
$(WORKLOADS)/sqlite: $(LOCK)
	@mkdir -p $(WORKLOADS)
	$(call unpack,sqlite,ports/patches/sqlite)
	@touch $@
$(WORKLOADS)/hello: $(LOCK)
	@mkdir -p $(WORKLOADS)
	$(call unpack,hello,ports/patches/hello)
	@touch $@
$(WORKLOADS_STAMP): $(addprefix $(WORKLOADS)/,tcc lua sqlite hello)
	@mkdir -p $(dir $@) && touch $@

# ---------------------------------------------------------------- T2's applets, probed

# For make measure-w4's report (ADR-0110 item 10): T2's toybox and busybox
# applets of applets-by-tier.tsv (with sort, wc and yes), built against this
# libc.a, which exports T1's list and the added blocks only. The links are
# expected to fail: the names they lack go to $(T2_PROBE). Nothing is
# installed; T2's rows are not stage 6's.
T2_TOYBOX_APPLETS := arch basename cat chmod cp cpio dirname echo env find grep head hostname id \
	install ln ls mkdir mktemp mv rm rmdir sed sleep sort tar touch true uname wc xargs yes
T2_BUSYBOX_APPLETS := awk diff expr gzip tr
T2_PROBE := $(LOGS)/t2-probe.txt
T2_TOYBOX := $(WORK)/toybox-t2
T2_BUSYBOX := $(WORK)/busybox-t2
t2-probe: $(T2_PROBE)
$(T2_TOYBOX): $(LOCK) $(wildcard ports/patches/toybox/*.patch)
	$(call unpack,toybox,ports/patches/toybox)
	@touch $@
$(T2_BUSYBOX): $(LOCK)
	$(call unpack,busybox,ports/patches/busybox)
	@touch $@
$(T2_PROBE): $(T2_TOYBOX) $(T2_BUSYBOX) $(SYSROOT_STAMP)
	@$(MAKE) -C $(T2_TOYBOX) allnoconfig HOSTCC=$(HOSTCC) SED=$(GSED) > $(LOGS)/t2-toybox.log 2>&1
	@$(call kconfig_on,$(T2_TOYBOX),$(T2_TOYBOX_APPLETS))
	@yes '' | $(MAKE) -C $(T2_TOYBOX) oldconfig HOSTCC=$(HOSTCC) SED=$(GSED) > /dev/null 2>&1
	-@$(CROSS_ENV) $(MAKE) -C $(T2_TOYBOX) toybox V=1 CC=$(ZCC) HOSTCC=$(HOSTCC) \
	  CFLAGS="$(CROSS_CFLAGS)" SED=$(GSED) LDOPTIMIZE=-Wl$(comma)--gc-sections NOSTRIP=1 \
	  >> $(LOGS)/t2-toybox.log 2>&1
	@$(MAKE) -C $(T2_BUSYBOX) allnoconfig HOSTCC=$(HOSTCC) KCONFIG_NOTIMESTAMP=1 > $(LOGS)/t2-busybox.log 2>&1
	@sed -e 's/^CONFIG_SH_IS_ASH=y/# CONFIG_SH_IS_ASH is not set/' \
	  -e 's/^# CONFIG_SH_IS_NONE is not set/CONFIG_SH_IS_NONE=y/' \
	  $(T2_BUSYBOX)/.config > $(T2_BUSYBOX)/.config.new && mv $(T2_BUSYBOX)/.config.new $(T2_BUSYBOX)/.config
	@$(call kconfig_on,$(T2_BUSYBOX),$(T2_BUSYBOX_APPLETS))
	@yes '' | $(MAKE) -C $(T2_BUSYBOX) oldconfig HOSTCC=$(HOSTCC) KCONFIG_NOTIMESTAMP=1 > /dev/null 2>&1
	-@$(CROSS_ENV) KCONFIG_NOTIMESTAMP=1 $(MAKE) -C $(T2_BUSYBOX) V=1 HOSTCC=$(HOSTCC) \
	  CC=$(ZCC) AR=$(TOOLBIN)/aarch64-unknown-zoraya-ar LD=$(TOOLBIN)/aarch64-unknown-zoraya-ld \
	  SKIP_STRIP=y >> $(LOGS)/t2-busybox.log 2>&1
	@for p in toybox busybox; do \
	  names=$$(sed -n 's/.*undefined symbol: \([A-Za-z0-9_]*\).*/\1/p' $(LOGS)/t2-$$p.log | sort -u); \
	  echo "$$p: $$(echo $$names | wc -w | tr -d ' ') names: $$(echo $$names)"; done > $@
	@cat $@

# ---------------------------------------------------------------- the host checks

HELLO_DIR := $(OBJ)/tcc-hello
# The export check alone (INV-0100-1, INV-0100-2), for make test-libposix.
check-libc: | tools
	@$(SURFACE_CHECK) list --check docs/research/task-02/libc-surface.csv $(TIER_LIST)
	@$(SURFACE_CHECK) check $(TIER_SPEC) $(LIBC) --allow-svc $(SVC_MEMBER)

check-ports: | tools
	@echo "== the tier list is the one ADR-0100 item 7's rule gives"
	$(SURFACE_CHECK) list --check docs/research/task-02/libc-surface.csv $(TIER_LIST)
	@echo "== libc.a against the tier list (INV-0100-1, INV-0100-2, INV-0005-1, INV-0005-2)"
	$(SURFACE_CHECK) check $(TIER_SPEC) $(LIBC) --allow-svc $(SVC_MEMBER)
	@echo "== the excluded musl files are exactly the ones the shim refuses (INV-0141-2)"
	@$(MAKE) -s -f ports/musl/musl.mk MUSL=$(MUSL) OBJ=$(OBJ)/musl ZCC=$(ZCC) LIST=$(MUSL_LIST) check-excluded
	@echo "== the same check fails on broken copies of libc.a"
	ports/check-broken-archives.sh $(SURFACE_CHECK) $(ZAR) $(ZCC) \
	  $(subst $(space),+,$(addprefix $(ROOT)/,$(TIER_LIST) $(ADDED_ADRS))) $(LIBC) $(OBJ)/broken \
	  $(SVC_MEMBER)
	@echo "== every program of the root file system loads (INV-0109-1)"
	$(ELF_CHECK) $(ROOTFS)/bin
	@echo "== no port was compiled with __linux__ (INV-0109-2, INV-0143-1)"
	@for l in dash make pdpmake busybox toybox tcc libtcc1 musl; do \
	  grep -q -e '$(ZCC)' $(LOGS)/$$l.log || { echo "$$l.log shows no compile commands"; exit 1; }; \
	  if grep -l -e '-D__linux' -e '-D__gnu_linux' $(LOGS)/$$l.log; then exit 1; fi; done
	@if $(ZCC) -D__linux__ -E -x c /dev/null > /dev/null 2>&1; then echo "zcc accepted -D__linux__"; exit 1; fi
	@echo "== tcc links a hello world against the sysroot (ADR-0109 item 7)"
	@$(MAKE) -f ports/Makefile tcc-host
	rm -rf $(HELLO_DIR) && mkdir -p $(HELLO_DIR)
	$(TCC_HOST) -dumpmachine | grep -x aarch64-unknown-zoraya
	ports/compare-predefs.sh $(ZCC) $(TCC_HOST) $(HELLO_DIR)
	$(TCC_HOST) -o $(HELLO_DIR)/a.out ports/tests/hello.c
	$(ELF_CHECK) $(HELLO_DIR)/a.out
	@echo "== tcc for Zoraya has no -run: it calls itself a cross compiler (INV-0143-2)"
	LC_ALL=C grep -a -q 'run is not available in a cross compiler' $(TCC_BIN)
	@echo "check-ports: all passed"
