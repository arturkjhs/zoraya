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
# that add names (posix-surface-check's LIST+ADR form). CI's ports cache
# names the same files (.github/workflows/test.yml, its build job).
ADDED_ADRS := docs/adr/posix-surface-T9.txt docs/adr/0142-surface-for-dash-job-control.md docs/adr/0144-surface-for-sort-and-wc.md \
	docs/adr/0210-surface-for-the-workloads-products.md docs/adr/0287-n4-posix-sockets-where-the-adrs-are-silent.md \
	docs/adr/0310-surface-for-tls-curl-and-wget.md docs/adr/0317-the-window-of-a-posix-program.md docs/adr/0320-cpython-port.md \
	docs/adr/0366-surface-for-cxx.md
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

.PHONY: cxx gtest python python-trees check-python-cache hostpython netsurf netsurf-libs netsurf-src host-pkgconfig suites apps tlsthreads cryptothreads zlib openssl curl wget ports fetch-ports check-ports check-libc sysroot libc rootfs tools musl headers posix \
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

# ---------------------------------------------------------------- C++ (TASK-30)

# LLVM's C++ runtime (proposed ADR-0360): libunwind, libc++abi and libc++,
# static, by tools/zc++ into the sysroot (ports/cxx/cxx.mk), with the headers
# in usr/include/c++/v1 and libunwind's in usr/include, from LLVM's tree
# (libc++abi includes libc++'s src/, libc++ LLVM libc's shared/). `make cxx` builds it; zc++ links every C++ program with it.
LLVMVER := $(call lock_ver,llvm)
LLVM_SRC := $(WORK)/llvm-$(LLVMVER)
ZCXX := $(ROOT)/tools/zc++
# Four directories of the archive, not its 1.6 GB: the three runtimes and
# LLVM libc, whose shared/ headers libc++'s from_chars includes.
LLVM_DIRS := libcxx libcxxabi libunwind libc
$(LLVM_SRC): $(LOCK) ports/patches/llvm $(wildcard ports/patches/llvm/*.patch)
	@f=$(CACHE)/$(call lock_file,llvm); \
	if [ ! -f "$$f" ]; then \
	  echo "ports: $$f is missing: run 'make fetch-ports' first (it is the only step that downloads)"; \
	  exit 1; fi
	@tools/fetch-ports.sh --verify $(LOCK) $(CACHE) llvm
	rm -rf $@.tmp && mkdir -p $@.tmp
	cd $@.tmp && tar xf $(CACHE)/$(call lock_file,llvm) $(addprefix llvm-project-$(LLVMVER).src/,$(LLVM_DIRS))
	rm -rf $@ && mv $@.tmp/llvm-project-$(LLVMVER).src $@ && rmdir $@.tmp
	@for p in $(sort $(wildcard ports/patches/llvm/*.patch)); do \
	  echo "patch $$p"; patch -s -p1 -d $@ < $$p || exit 1; done
	@touch $@
# The headers: libc++'s include/ without its build files, the site
# configuration (ports/cxx/__config_site) and LLVM's default assertion
# handler, libc++abi's two, libunwind's four.
CXX_HEADERS_STAMP := $(OBJ)/cxx-headers.stamp
$(CXX_HEADERS_STAMP): $(LLVM_SRC) ports/cxx/__config_site $(HEADERS_STAMP)
	rm -rf $(SYSROOT)/usr/include/c++ && mkdir -p $(SYSROOT)/usr/include/c++/v1
	cp -R $(LLVM_SRC)/libcxx/include/. $(SYSROOT)/usr/include/c++/v1/
	rm -f $(SYSROOT)/usr/include/c++/v1/CMakeLists.txt $(SYSROOT)/usr/include/c++/v1/__config_site.in \
	  $(SYSROOT)/usr/include/c++/v1/module.modulemap.in
	cp ports/cxx/__config_site $(SYSROOT)/usr/include/c++/v1/__config_site
	cp $(LLVM_SRC)/libcxx/vendor/llvm/default_assertion_handler.in $(SYSROOT)/usr/include/c++/v1/__assertion_handler
	cp $(LLVM_SRC)/libcxxabi/include/cxxabi.h $(LLVM_SRC)/libcxxabi/include/__cxxabi_config.h $(SYSROOT)/usr/include/c++/v1/
	cp $(addprefix $(LLVM_SRC)/libunwind/include/,unwind.h libunwind.h __libunwind_config.h unwind_itanium.h unwind_arm_ehabi.h) \
	  $(SYSROOT)/usr/include/
	@mkdir -p $(dir $@) && touch $@
CXX_STAMP := $(OBJ)/cxx.stamp
cxx: $(CXX_STAMP)
# The objects are made again whenever the stamp is: the unpacked sources keep
# the archive's old times, and cxx.mk's rules do not list the headers.
$(CXX_STAMP): $(CXX_HEADERS_STAMP) $(SYSROOT_STAMP) ports/cxx/cxx.mk ports/cxx/zoraya-shim.c tools/zcc
	rm -rf $(OBJ)/cxx
	@$(call logged,$(MAKE) -j3 -f ports/cxx/cxx.mk SRC=$(LLVM_SRC) OBJ=$(OBJ)/cxx ZCXX=$(ZCXX) ZAR=$(ZAR) SYSROOT=$(SYSROOT),cxx)
	@ls -l $(SYSROOT)/usr/lib/libc++.a $(SYSROOT)/usr/lib/libc++abi.a $(SYSROOT)/usr/lib/libunwind.a
	ln -sf $(ZCXX) $(TOOLBIN)/aarch64-unknown-zoraya-c++
	@touch $@

# GoogleTest (TASK-30): built by CMake with ports/cmake/zoraya.cmake, as any
# CMake project would be, with its own tests (gtest_build_tests); patch 0001
# names Zoraya among the systems with pthreads and death tests. The test
# programs that check themselves (cxx_test in googletest/CMakeLists.txt, not
# the ones a Python driver reads) go to $(GTEST_OUT)/bin; make test-cxx-gtest
# runs them inside Zoraya. Not gmock (BUILD_GMOCK=OFF): GoogleTest's own tests
# are what TASK-30 asks for.
CMAKE ?= cmake
GTEST_SRC := $(WORK)/googletest-$(call lock_ver,googletest)
GTEST_BUILD := $(WORK)/googletest-build
GTEST_OUT := $(ROOT)/build/gtest-tests
GTEST_STAMP := $(OBJ)/gtest.stamp
GTEST_TESTS := googletest-death-test-test gtest_environment_test googletest-filepath-test googletest-listener-test \
	gtest_main_unittest googletest-message-test gtest_no_test_unittest googletest-options-test googletest-param-test-test \
	googletest-port-test gtest_pred_impl_unittest gtest_premature_exit_test googletest-printers-test gtest_prod_test \
	gtest_repeat_test gtest_sole_header_test gtest_stress_test googletest-test-part-test gtest_throw_on_failure_ex_test \
	gtest-typed-test_test gtest_unittest gtest-unittest-api_test gtest_skip_in_environment_setup_test gtest_skip_test \
	gtest-death-test_ex_nocatch_test gtest-death-test_ex_catch_test gtest_no_rtti_unittest
gtest: $(GTEST_STAMP)
$(GTEST_SRC): $(LOCK) $(wildcard ports/patches/googletest/*.patch)
	$(call unpack,googletest,ports/patches/googletest)
	@touch $@
$(GTEST_STAMP): $(GTEST_SRC) $(CXX_STAMP) ports/cmake/zoraya.cmake ports/cmake/Platform/Zoraya.cmake
	rm -rf $(GTEST_BUILD) $(GTEST_OUT) && mkdir -p $(GTEST_BUILD) $(GTEST_OUT)/bin
	@$(call logged,$(CMAKE) -S $(GTEST_SRC) -B $(GTEST_BUILD) -G "Unix Makefiles" \
	  -DCMAKE_TOOLCHAIN_FILE=$(ROOT)/ports/cmake/zoraya.cmake -DCMAKE_BUILD_TYPE=Release \
	  -DCMAKE_EXE_LINKER_FLAGS=-Wl$(comma)--strip-debug -DBUILD_GMOCK=OFF -Dgtest_build_tests=ON -Dgtest_build_samples=ON,gtest-configure)
	@$(call logged,$(MAKE) -C $(GTEST_BUILD) -j3,gtest)
	for t in $(GTEST_TESTS) sample1_unittest sample10_unittest; do \
	  cp $(GTEST_BUILD)/googletest/$$t $(GTEST_OUT)/bin/ || exit 1; done
	printf '%s\n' $(GTEST_TESTS) > $(GTEST_OUT)/list
	$(ELF_CHECK) $(GTEST_OUT)/bin
	@touch $@

# ---------------------------------------------------------------- TLS for C programs, curl, wget (TASK-27 9a)

# zlib, OpenSSL, curl and wget for the volume (ADR-0310), built by zcc into
# $(APPS_ROOT) (bin/curl, bin/wget). The libraries go into $(DEPS), not into
# the sysroot: they are for these two programs, not for every program that
# tcc builds in Zoraya. OpenSSL is static, with threads (musl pthreads in libposix, ADR-0302; 9a step 2 of ADR-0309),
# shared objects, dynamic loading, engines or assembly; its entropy is
# libposix's getrandom (ports/patches/openssl/0001-*), its roots file is
# /system/tls/roots (ADR-0228 item 3) and curl's and wget's are the same.
DEPS := $(PORTS)/deps
APPS_ROOT := $(ROOT)/build/rootfs-apps
APPS_STAMP := $(OBJ)/apps.stamp
CROSS_ROOTS := /system/tls/roots
# No debug information: curl and wget with OpenSSL are 17 MiB with it, 6.7 MiB without.
APPS_CFLAGS := -O2

ZLIB := $(WORK)/zlib-$(call lock_ver,zlib)
zlib: $(DEPS)/lib/libz.a
$(ZLIB): $(LOCK) $(wildcard ports/patches/zlib/*.patch)
	$(call unpack,zlib,ports/patches/zlib)
	@touch $@
$(DEPS)/lib/libz.a: $(ZLIB) $(SYSROOT_STAMP)
	@mkdir -p $(DEPS)
	@cd $(ZLIB) && $(call logged,$(CROSS_ENV) CC=$(ZCC) AR=$(TOOLBIN)/aarch64-unknown-zoraya-ar \
	  RANLIB=$(TOOLBIN)/aarch64-unknown-zoraya-ranlib CFLAGS="$(APPS_CFLAGS)" ./configure --static --prefix=$(DEPS),zlib-configure)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(ZLIB) AR=$(TOOLBIN)/aarch64-unknown-zoraya-ar ARFLAGS=rc libz.a,zlib)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(ZLIB) install,zlib-install)
	@touch $@

OPENSSL := $(WORK)/openssl-$(call lock_ver,openssl)
openssl: $(DEPS)/lib/libssl.a
$(OPENSSL): $(LOCK) $(wildcard ports/patches/openssl/*.patch)
	$(call unpack,openssl,ports/patches/openssl)
	@touch $@
# linux-generic64 is OpenSSL's portable 64-bit target (no assembly, no CPU
# probing); no-secure-memory leaves out mmap/mlock tricks, no-async the
# ucontext coroutines. TLS 1.2 and 1.3 are the only protocols compiled in
# (no SSL 3, TLS 1.0, TLS 1.1 or DTLS: INV-0310-5), and the only entropy
# source is getrandom (--with-rand-seed=getrandom leaves out the /dev/urandom
# and /dev/random code: INV-0310-5). The patches under __zoraya__ also make the
# library read no environment variable and load no openssl.cnf; curl and wget read no
# configuration file or environment variable for trust (INV-0310-4).
OPENSSL_OPTS := no-shared no-dso threads no-engine no-asm no-async no-afalgeng no-ui-console no-secure-memory \
	no-sock no-dgram no-quic no-tests no-docs no-comp no-ssl3 no-ssl3-method no-weak-ssl-ciphers no-legacy no-autoload-config \
	no-tls1 no-tls1_1 no-tls1-method no-tls1_1-method no-dtls no-dtls1-method no-dtls1_2-method --with-rand-seed=getrandom
# The configuration the library was built with: the options, the compile flags
# and the hash of the patch set. The stamp file changes only when that text
# does, so a change of OPENSSL_OPTS rebuilds libssl.a (make reads times, not
# options). CI's cache key hashes ports/**, which holds this Makefile, so a
# cache of another configuration is not restored as current either.
OPENSSL_STAMP := $(WORK)/openssl-config.stamp
OPENSSL_CONFIG = $(OPENSSL_OPTS) / $(APPS_CFLAGS) / $(shell cat $(sort $(wildcard ports/patches/openssl/*.patch)) | cksum)
$(OPENSSL_STAMP): FORCE
	@mkdir -p $(WORK)
	@printf '%s\n' '$(OPENSSL_CONFIG)' > $@.new
	@if cmp -s $@.new $@; then rm -f $@.new; else mv $@.new $@; fi
$(DEPS)/lib/libssl.a: $(OPENSSL) $(OPENSSL_STAMP) $(DEPS)/lib/libz.a $(SYSROOT_STAMP)
	@mkdir -p $(DEPS)
	@if [ -f $(OPENSSL)/Makefile ]; then $(MAKE) -C $(OPENSSL) clean >/dev/null 2>&1; fi
	@cd $(OPENSSL) && $(call logged,$(CROSS_ENV) CC=$(ZCC) AR=$(TOOLBIN)/aarch64-unknown-zoraya-ar \
	  RANLIB=$(TOOLBIN)/aarch64-unknown-zoraya-ranlib ./Configure linux-generic64 $(OPENSSL_OPTS) \
	  --prefix=$(DEPS) --openssldir=/system/tls --libdir=lib $(APPS_CFLAGS) -D__STDC_NO_ATOMICS__,openssl-configure)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(OPENSSL) -j3 build_libs,openssl)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(OPENSSL) install_dev,openssl-install)
	@touch $@

# The names the two programs reference that libc.a does not export
# (ports/apps/zshim.c): linked from here, after everything else.
$(DEPS)/lib/libzshim.a: ports/apps/zshim.c $(SYSROOT_STAMP)
	@mkdir -p $(DEPS)/lib $(OBJ)/apps
	$(ZCC) $(APPS_CFLAGS) -c -o $(OBJ)/apps/zshim.o ports/apps/zshim.c
	rm -f $@ && $(TOOLBIN)/aarch64-unknown-zoraya-ar rcs $@ $(OBJ)/apps/zshim.o

# The test program of ADR-0309 item 4 (OpenSSL with threads): four threads
# share one context and run handshakes at the same time. It is built here,
# against the OpenSSL above, but is not part of $(APPS_ROOT): `make test-apps`
# copies it to its own volume.
TLSTHREADS_BIN := $(OBJ)/apps/tlsthreads
tlsthreads: $(TLSTHREADS_BIN)
$(TLSTHREADS_BIN): tests/apps/tlsthreads.c $(DEPS)/lib/libssl.a $(DEPS)/lib/libzshim.a
	@mkdir -p $(OBJ)/apps
	$(ZCC) $(APPS_CFLAGS) -I$(DEPS)/include -o $@ tests/apps/tlsthreads.c -L$(DEPS)/lib -lssl -lcrypto -lz -lzshim

CRYPTOTHREADS_BIN := $(OBJ)/apps/cryptothreads
cryptothreads: $(CRYPTOTHREADS_BIN)
$(CRYPTOTHREADS_BIN): tests/apps/cryptothreads.c $(DEPS)/lib/libssl.a $(DEPS)/lib/libzshim.a
	@mkdir -p $(OBJ)/apps
	$(ZCC) $(APPS_CFLAGS) -I$(DEPS)/include -o $@ tests/apps/cryptothreads.c -L$(DEPS)/lib -lssl -lcrypto -lz -lzshim

CURL := $(WORK)/curl-$(call lock_ver,curl)
CURL_BIN := $(CURL)/src/curl
curl: $(CURL_BIN)
$(CURL): $(LOCK) $(wildcard ports/patches/curl/*.patch)
	$(call unpack,curl,ports/patches/curl)
	@touch $@
# HTTP and HTTPS only, synchronous name resolution (no threads), no IPv6
# (ADR-0225 offers IPv4), OpenSSL's roots from the system image, no `-k`
# surprises left to a configuration file: the description lists the options.
# No passwd lookups (getpwuid): the volume has no /etc/passwd, and curl's search
# for a .curlrc would only make the shell report a refused read of it. OpenSSL's
# configuration file is not loaded either (no /system/tls/openssl.cnf).
CURL_OPTS := --disable-shared --enable-static --with-openssl=$(DEPS) --with-zlib=$(DEPS) \
	--with-ca-bundle=$(CROSS_ROOTS) --without-ca-path --without-ca-fallback \
	--disable-threaded-resolver --disable-ipv6 --disable-unix-sockets --disable-docs --disable-manual \
	--disable-ftp --disable-file --disable-ldap --disable-ldaps --disable-rtsp --disable-dict \
	--disable-telnet --disable-tftp --disable-pop3 --disable-imap --disable-smtp --disable-gopher \
	--disable-smb --disable-mqtt --disable-websockets --disable-ntlm --disable-ares --disable-libcurl-option \
	--disable-netrc --disable-cookies --disable-hsts --disable-alt-svc --disable-proxy --disable-sspi \
	--without-libpsl --without-brotli --without-zstd --without-nghttp2 --without-nghttp3 --without-ngtcp2 \
	--without-libidn2 --without-librtmp --without-libssh2 --without-libssh --without-gssapi --without-winidn
$(CURL_BIN): $(CURL) $(DEPS)/lib/libssl.a $(DEPS)/lib/libzshim.a ports/config.site
	@cd $(CURL) && $(call logged,$(CROSS_ENV) ac_cv_func_getpwuid=no ac_cv_func_getpwuid_r=no ./configure --host=aarch64-unknown-zoraya \
	  $(CURL_OPTS) $(CROSS_TOOLS) CFLAGS="$(APPS_CFLAGS)" CPPFLAGS="-DCURL_DISABLE_OPENSSL_AUTO_LOAD_CONFIG" LDFLAGS="-L$(DEPS)/lib" LIBS="-lzshim",curl-configure)
	rm -f $(CURL_BIN)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(CURL) -j3 V=1,curl)
	@touch $@

WGET := $(WORK)/wget-$(call lock_ver,wget)
WGET_BIN := $(WGET)/src/wget
wget: $(WGET_BIN)
$(WGET): $(LOCK) $(wildcard ports/patches/wget/*.patch)
	$(call unpack,wget,ports/patches/wget)
	@touch $@
# HTTP and HTTPS over OpenSSL (the one built above), IPv6 code compiled in (it
# resolves with getaddrinfo, which libposix answers with IPv4 only), no name
# translation, no PCRE, no libpsl, no digest or NTLM authentication, no
# metalink or libuuid. The host triple names musl, not Zoraya: wget's gnulib
# answers its run tests (mbrtowc and the like) by the C library's name when it
# cannot run them, and Zoraya's libc is musl's; the compiler is still zcc,
# which defines nothing of Linux (INV-0143-1).
WGET_OPTS := --sysconfdir=/etc --with-ssl=openssl --with-libssl-prefix=$(DEPS) --with-zlib --disable-nls --enable-ipv6 \
	--disable-iri --disable-pcre --disable-pcre2 --disable-opie --disable-digest --disable-ntlm \
	--disable-debug --disable-threads --disable-rpath --without-libpsl --without-libuuid --without-metalink \
	--without-cares --without-libidn --without-gpgme --without-included-regex
$(WGET_BIN): $(WGET) $(DEPS)/lib/libssl.a $(DEPS)/lib/libzshim.a ports/config.site
	@cd $(WGET) && $(call logged,$(CROSS_ENV) PKG_CONFIG=false ac_cv_header_uchar_h=no ac_cv_func_rewinddir=yes ./configure --host=aarch64-unknown-linux-musl \
	  $(WGET_OPTS) $(CROSS_TOOLS) CFLAGS="$(APPS_CFLAGS)" CPPFLAGS="-I$(DEPS)/include -D_PYTHREAD_NAME_MAXLEN=15" \
	  LDFLAGS="-L$(DEPS)/lib" LIBS="-lzshim" ZLIB_CFLAGS="-I$(DEPS)/include" ZLIB_LIBS="-L$(DEPS)/lib -lz",wget-configure)
	rm -f $(WGET_BIN)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(WGET)/lib -j3,wget-lib)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(WGET)/src -j3 V=1,wget)
	@touch $@

# build/rootfs-apps: the two programs as they go into the volume's /bin
# (ADR-0310), and nothing else. `make image EXTRA_ROOT=build/rootfs-apps`
# (tools/run-disk.sh) is how an image takes them; their descriptions are
# user/describe/posix/curl.cmd and wget.cmd, in the ramdisk with the others.
apps: $(APPS_STAMP)
$(APPS_STAMP): $(CURL_BIN) $(WGET_BIN)
	mkdir -p $(APPS_ROOT)/bin
	cp $(CURL_BIN) $(APPS_ROOT)/bin/curl
	cp $(WGET_BIN) $(APPS_ROOT)/bin/wget
	$(ELF_CHECK) $(APPS_ROOT)/bin
	@echo "== no entropy source but getrandom, and no TLS before 1.2 (INV-0310-5)"
	@for b in curl wget; do \
	  if LC_ALL=C grep -a -q -e '/dev/urandom' -e '/dev/random' $(APPS_ROOT)/bin/$$b; then \
	    echo "$$b names /dev/urandom or /dev/random"; exit 1; fi; \
	  nm $(APPS_ROOT)/bin/$$b > $(APPS_ROOT)/$$b.nm || { echo "nm failed on $$b"; exit 1; }; \
	  grep -q -e ' T TLS_method' $(APPS_ROOT)/$$b.nm || { echo "$$b: nm shows no TLS_method (control)"; exit 1; }; \
	  if grep -q -e ' T TLSv1_method' -e ' T TLSv1_1_method' -e ' T SSLv3_method' -e ' T DTLSv1_method' $(APPS_ROOT)/$$b.nm; then \
	    echo "$$b links a TLS method before 1.2"; exit 1; fi; rm -f $(APPS_ROOT)/$$b.nm; done
	@echo "== no app was compiled with __linux__, and every compile ran zcc (INV-0310-3, INV-0143-1)"
	@for l in zlib openssl curl wget; do \
	  grep -q -e '$(ZCC)' $(LOGS)/$$l.log || { echo "$$l.log shows no compile commands"; exit 1; }; \
	  if grep -l -e '-D__linux' -e '-D__gnu_linux' $(LOGS)/$$l.log; then exit 1; fi; done
	@echo "== every change of the OpenSSL patches is under __zoraya__ (INV-0310-3)"
	@for p in ports/patches/openssl/*.patch; do \
	  grep -q '^+.*__zoraya__' $$p || { echo "$$p changes OpenSSL for every system"; exit 1; }; done
	@touch $@

# ---------------------------------------------------------------- CPython (TASK-27 9c)

# CPython (ADR-0320), cross-built with zcc, static: every extension module is
# linked into the one program (MODULE_BUILDTYPE=static, so no module is a
# shared object), and the library has no dlopen to load one. The standard
# library goes to $(PYTHON_ROOT)/usr/lib/python3.X (a directory of its own,
# not build/rootfs-apps: see docs/design.md); `make python` leaves both there.
# A build with the host's compiler of the same version is the build python
# CPython's cross build requires (--with-build-python): it runs the freeze,
# deepfreeze and compileall steps. Its prefix is build/ports/hostpython, not
# NetSurf's build/ports/host (9d): CI's artifact leaves the host python out and
# keeps NetSurf's host tools, and NetSurf's build puts that bin/ first in PATH.
PYVER := $(call lock_ver,python)
PYMM := $(basename $(PYVER))
PYTHON_SRC := $(WORK)/Python-$(PYVER)
PYTHON_ROOT := $(ROOT)/build/rootfs-python
PYTHON_BUILD := $(WORK)/python-build
HOSTPY_PREFIX := $(PORTS)/hostpython
HOSTPY := $(HOSTPY_PREFIX)/bin/python$(PYMM)
$(PYTHON_SRC): $(LOCK) $(wildcard ports/patches/python/*.patch)
	$(call unpack,python,ports/patches/python)
	@touch $@
# What the python stamp depends on are markers, not the trees: CI's test jobs
# take build/ without the source and host trees (they are 500 MB the tests do
# not read, W10), and a missing tree would make make rebuild the whole port.
# A marker is older than the output built from it. A marker does not say its
# tree is there: when the stamp must be rebuilt and a tree is missing (CI's
# build job after a cache hit, whose cache leaves the trees out), the stamp's
# recipe makes the tree again first (PYTHON_SRC_TREE, HOSTPY_TREE; make
# check-python-cache).
PYTHON_SRC_STAMP := $(OBJ)/python-src.stamp
$(PYTHON_SRC_STAMP): $(LOCK) $(wildcard ports/patches/python/*.patch)
	rm -rf $(PYTHON_SRC)
	@$(MAKE) -f ports/Makefile $(PYTHON_SRC)
	@mkdir -p $(dir $@) && touch $@
# The host build is made from an unpatched tree of its own: the patches are
# for Zoraya, and the host python only has to be the same version. It is minimal
# on purpose: no ensurepip (it needs the zlib module, and the CI container has no
# zlib headers; compileall and the freeze steps do not need zlib).
HOSTPY_SRC := $(WORK)/Python-$(PYVER)-host
$(HOSTPY_SRC): $(LOCK)
	$(call unpack,python,ports/patches/none)
	@touch $@
HOSTPY_STAMP := $(OBJ)/hostpython.stamp
hostpython: $(HOSTPY_STAMP)
$(HOSTPY_STAMP): $(LOCK)
	@$(MAKE) -f ports/Makefile $(HOSTPY)
	@mkdir -p $(dir $@) && touch $@
$(HOSTPY): $(HOSTPY_SRC)
	rm -rf $(WORK)/hostpython-build && mkdir -p $(WORK)/hostpython-build
	@cd $(WORK)/hostpython-build && $(call logged,$(HOSTPY_SRC)/configure --prefix=$(HOSTPY_PREFIX) --without-ensurepip,hostpython-configure)
	@$(call logged,$(MAKE) -C $(WORK)/hostpython-build -j3,hostpython)
	@$(call logged,$(MAKE) -C $(WORK)/hostpython-build install,hostpython-install)
	@touch $@

# SQLite's amalgamation for _sqlite3: threadsafe, no run-time extensions
# (libc.a has no dlopen, ADR-0210 item 4), no WAL (it needs a shared mapping of
# the database file, which Zoraya has not: 9b's threadtest3 leaves it out too).
$(DEPS)/lib/libsqlite3.a: $(PORTS)/workloads/sqlite $(SYSROOT_STAMP)
	@mkdir -p $(DEPS)/lib $(DEPS)/include $(OBJ)/apps
	$(ZCC) $(APPS_CFLAGS) -w -DSQLITE_THREADSAFE=1 -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_WAL -DSQLITE_ENABLE_FTS5 \
	  -c -o $(OBJ)/apps/sqlite3.o $(PORTS)/workloads/sqlite/sqlite3.c
	cp $(PORTS)/workloads/sqlite/sqlite3.h $(PORTS)/workloads/sqlite/sqlite3ext.h $(DEPS)/include/
	rm -f $@ && $(TOOLBIN)/aarch64-unknown-zoraya-ar rcs $@ $(OBJ)/apps/sqlite3.o

# The names CPython's sources reference that libc.a does not export
# (ports/apps/pyshim.c), after zshim.
$(DEPS)/lib/libpyshim.a: ports/apps/pyshim.c $(SYSROOT_STAMP)
	@mkdir -p $(DEPS)/lib $(OBJ)/apps
	$(ZCC) $(APPS_CFLAGS) -c -o $(OBJ)/apps/pyshim.o ports/apps/pyshim.c
	rm -f $@ && $(TOOLBIN)/aarch64-unknown-zoraya-ar rcs $@ $(OBJ)/apps/pyshim.o

# The configure answers a cross build cannot run (ac_cv_file__dev_ptmx and the
# like: Zoraya has no /dev/ptmx), MACHDEP=zoraya so that sys.platform is
# "zoraya", not "linux" (the host triple only has to pass configure's list of
# cross targets and is a musl one, as wget's; the compiler is zcc, which
# defines nothing of Linux). IPv4 only (ADR-0225); no readline, curses, bz2,
# lzma, uuid, ffi, gdbm, ensurepip; mimalloc and the JIT off.
# _PYTHREAD_NAME_MAXLEN: configure knows the limit of pthread_setname_np only for
# the systems it names, and without it CPython passes a long name whole; libposix
# keeps 15 bytes and a NUL and refuses a longer one with ERANGE (threads.rs, as
# musl), so CPython truncates to 15 (N8).
# No <sys/ioctl.h>: libposix's ioctl serves terminals only, so os.set_blocking and
# socket.setblocking must take their fcntl(F_SETFL, O_NONBLOCK) path, not FIONBIO.
# The functions configure finds only by their declaration in musl's headers
# (a compile test), which libc.a does not define: said absent here, so that the
# module code that needs them is left out instead of failing to link.
# No x86 SIMD for HACL*'s BLAKE2 (the SSE and AVX2 variants): configure asks
# only whether the compiler accepts -msse.../-mavx2, and the answer depends on
# the host's clang, not on the target. Apple clang refuses them for an aarch64
# target; Ubuntu's clang 18 (CI) only warns that they are unused, which zcc's
# -Qunused-arguments silences, so configure said yes there and the x86-only
# Hacl_Hash_Blake2b_Simd256.c failed to compile (ADR-0320).
PYTHON_ABSENT := posix_spawn posix_spawnp chroot epoll_create epoll_create1 eventfd fchdir fdatasync getpagesize link prlimit setgroups socketpair timerfd_create
PYTHON_NO_X86_SIMD := ax_cv_check_cflags__Werror__msse__msse2__msse3__msse4_1__msse4_2=no ax_cv_check_cflags__Werror__mavx2=no
PYTHON_CACHE := ac_cv_file__dev_ptmx=no ac_cv_file__dev_ptc=no ac_cv_buggy_getaddrinfo=no ac_cv_header_libintl_h=no ac_cv_header_sys_ioctl_h=no py_cv_module_syslog=n/a $(addsuffix =no,$(addprefix ac_cv_func_,$(PYTHON_ABSENT))) $(PYTHON_NO_X86_SIMD)
PYTHON_OPTS := --host=aarch64-unknown-linux-musl --build=$(shell $(HOSTCC) -dumpmachine) --prefix=/usr \
	--with-build-python=$(HOSTPY) --disable-shared --disable-ipv6 --without-ensurepip \
	--with-openssl=$(DEPS) --without-system-ffi --without-readline --without-mimalloc \
	--without-doc-strings --disable-test-modules
python: $(PYTHON_ROOT)/.stamp
PYTHON_STAGE := $(PORTS)/python-stage
# What the volume does not need: the test suite (make test-python takes its
# own subset from the stage), IDLE, tkinter and turtle, ensurepip, the
# embedding's configuration and the documentation topics. Sizes: docs/design.md.
PYTHON_TRIM := test idlelib idle_test tkinter turtledemo turtle.py ensurepip config-$(PYMM)-* pydoc_data __phello__
# The trees the recipe reads, made again if a marker outlived them (above).
PYTHON_SRC_TREE = [ -x $(PYTHON_SRC)/configure ] || { echo "python: no $(PYTHON_SRC), unpacking it again"; \
	  rm -f $(PYTHON_SRC_STAMP) && $(MAKE) -f ports/Makefile $(PYTHON_SRC_STAMP); }
HOSTPY_TREE = [ -x $(HOSTPY) ] || { echo "python: no $(HOSTPY), building the host python again"; \
	  rm -f $(HOSTPY_STAMP) && $(MAKE) -f ports/Makefile $(HOSTPY_STAMP); }
# The '+' makes them run under make -n too, which is how check-python-cache
# sees what a cache hit would make, without making it.
python-trees:
	@+$(PYTHON_SRC_TREE)
	@+$(HOSTPY_TREE)
# CI's build job after a cache hit (E1): both markers there, neither tree, the
# stamp out of date. Dry run in a scratch directory: the trees must be made
# again (the unpack and the host build's configure are what make would run);
# with both trees there, nothing.
PYCACHE_CHECK := $(ROOT)/build/check-python-cache
PYCACHE_VARS = WORK=$(PYCACHE_CHECK)/work OBJ=$(PYCACHE_CHECK)/obj LOGS=$(PYCACHE_CHECK)/logs HOSTPY_PREFIX=$(PYCACHE_CHECK)/host
check-python-cache:
	@rm -rf $(PYCACHE_CHECK) && mkdir -p $(PYCACHE_CHECK)/obj $(PYCACHE_CHECK)/logs
	@touch $(PYCACHE_CHECK)/obj/python-src.stamp $(PYCACHE_CHECK)/obj/hostpython.stamp
	@# make -n still runs the host build's $(MAKE) -C lines (as dry runs): give them a directory.
	@mkdir -p $(PYCACHE_CHECK)/work/hostpython-build && printf 'all install:\n\t@:\n' > $(PYCACHE_CHECK)/work/hostpython-build/Makefile
	@$(MAKE) -s -n -f ports/Makefile python-trees $(PYCACHE_VARS) > $(PYCACHE_CHECK)/hit.txt 2>&1 || \
	  { echo "check-python-cache: make -n failed"; tail -20 $(PYCACHE_CHECK)/hit.txt; exit 1; }
	@for w in "unpacking it again" "building the host python again" "Python-$(PYVER)-host/configure --prefix=$(PYCACHE_CHECK)/host" \
	  "hostpython-install"; do grep -q -F -e "$$w" $(PYCACHE_CHECK)/hit.txt || \
	  { echo "check-python-cache: a cache hit would not make the trees again (no '$$w'):"; cat $(PYCACHE_CHECK)/hit.txt; exit 1; }; done
	@grep -q -e 'tar xf .*Python-$(PYVER)' $(PYCACHE_CHECK)/hit.txt || \
	  { echo "check-python-cache: a cache hit would not unpack the source again:"; cat $(PYCACHE_CHECK)/hit.txt; exit 1; }
	@mkdir -p $(PYCACHE_CHECK)/work/Python-$(PYVER) $(PYCACHE_CHECK)/host/bin
	@printf '#!/bin/sh\n' > $(PYCACHE_CHECK)/work/Python-$(PYVER)/configure && cp $(PYCACHE_CHECK)/work/Python-$(PYVER)/configure $(PYCACHE_CHECK)/host/bin/python$(PYMM)
	@chmod +x $(PYCACHE_CHECK)/work/Python-$(PYVER)/configure $(PYCACHE_CHECK)/host/bin/python$(PYMM)
	@$(MAKE) -s -n -f ports/Makefile python-trees $(PYCACHE_VARS) > $(PYCACHE_CHECK)/present.txt 2>&1
	@if grep -q -e 'again$$' -e 'tar xf' -e 'configure --prefix' $(PYCACHE_CHECK)/present.txt; then \
	  echo "check-python-cache: trees there, yet make would make them again:"; cat $(PYCACHE_CHECK)/present.txt; exit 1; fi
	@rm -rf $(PYCACHE_CHECK)
	@echo "check-python-cache: a cache hit unpacks the source and builds the host python again; with both there, nothing"
$(PYTHON_ROOT)/.stamp: $(PYTHON_SRC_STAMP) $(HOSTPY_STAMP) $(DEPS)/lib/libssl.a $(DEPS)/lib/libsqlite3.a $(DEPS)/lib/libpyshim.a $(DEPS)/lib/libzshim.a ports/config.site
	@$(PYTHON_SRC_TREE)
	@$(HOSTPY_TREE)
	rm -rf $(PYTHON_BUILD) $(PYTHON_STAGE) $(PYTHON_ROOT) && mkdir -p $(PYTHON_BUILD)
	@cd $(PYTHON_BUILD) && $(call logged,$(CROSS_ENV) $(PYTHON_CACHE) MACHDEP=zoraya MODULE_BUILDTYPE=static PKG_CONFIG=false OPT="-DNDEBUG -O2" \
	  $(PYTHON_SRC)/configure $(PYTHON_OPTS) $(CROSS_TOOLS) CFLAGS="$(APPS_CFLAGS)" CPPFLAGS="-I$(DEPS)/include -D_PYTHREAD_NAME_MAXLEN=15" \
	  LDFLAGS="-L$(DEPS)/lib" LIBS="-lpyshim -lzshim" \
	  ZLIB_CFLAGS="-I$(DEPS)/include" ZLIB_LIBS="-L$(DEPS)/lib -lz" \
	  LIBSQLITE3_CFLAGS="-I$(DEPS)/include" LIBSQLITE3_LIBS="-L$(DEPS)/lib -lsqlite3",python-configure)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(PYTHON_BUILD) -j3,python)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(PYTHON_BUILD) install DESTDIR=$(PYTHON_STAGE),python-install)
	@# The byte code is compiled once, by the host's python of the same version, and
	@# is never checked against the source (unchecked-hash): a volume keeps no
	@# modification time the check could rely on, and nothing here writes a .pyc.
	find $(PYTHON_STAGE)/usr/lib/python$(PYMM) -name __pycache__ -type d -prune -exec rm -rf {} +
	@$(call logged,$(HOSTPY) -I -m compileall -q -f -j3 --invalidation-mode unchecked-hash -d /usr/lib/python$(PYMM) $(PYTHON_STAGE)/usr/lib/python$(PYMM),python-compileall)
	mkdir -p $(PYTHON_ROOT)/bin $(PYTHON_ROOT)/usr/lib
	cp $(PYTHON_STAGE)/usr/bin/python$(PYMM) $(PYTHON_ROOT)/bin/python3
	cp -R $(PYTHON_STAGE)/usr/lib/python$(PYMM) $(PYTHON_ROOT)/usr/lib/
	cd $(PYTHON_ROOT)/usr/lib/python$(PYMM) && rm -rf $(PYTHON_TRIM)
	$(ELF_CHECK) $(PYTHON_ROOT)/bin
	@echo "python: image part $$(du -sk $(PYTHON_ROOT) | cut -f1) KiB, $$(find $(PYTHON_ROOT) -type f | wc -l | tr -d ' ') files; stage $$(du -sk $(PYTHON_STAGE)/usr/lib/python$(PYMM) | cut -f1) KiB"
	@touch $@
# ---------------------------------------------------------------- NetSurf (TASK-27 9d, ADR-0317, ADR-0318)

# `make netsurf`: libpng, libjpeg and curl's header and library for NetSurf into
# $(DEPS); NetSurf 3.11's own libraries into $(DEPS); then `netsurf` (the framebuffer
# front end, its bitmap font, curl with OpenSSL from 9a) into $(APPS_ROOT)/bin, and
# its resources (default.css, Messages, ...) into $(APPS_ROOT)/usr/share/netsurf.
# Host tools, built into $(HOSTPFX) from the pinned archives so that the build needs no
# more of the host than a C compiler, perl, gperf, flex and bison: pkg-config (only
# when the host has none; ADR-0318 item 1) and zlib and libpng, which NetSurf's
# image converter (tools/convert_image.c) links. Not built, by decision (ADR-0318 and
# the report's 9d step 2): JavaScript (so no nsgenbind), SVG (libsvgtiny needs an XML
# binding of libdom, which needs expat), RISC OS sprites, libjxl, WebP, video.
HOSTPFX := $(PORTS)/host
NS_PKGCONFIG := $(shell command -v pkg-config 2>/dev/null)
ifeq ($(NS_PKGCONFIG),)
NS_PKGCONFIG := $(HOSTPFX)/bin/pkg-config
endif
# bison 3 or later (libnslog's grammar; macOS ships 2.3): the host's, or the pinned one built into $(HOSTPFX).
NS_BISON_OK := $(shell bison --version 2>/dev/null | awk 'NR == 1 { split($$NF, v, "."); print (v[1] >= 3) ? "yes" : "no" }')
NS_BISON := $(if $(filter yes,$(NS_BISON_OK)),,$(HOSTPFX)/bin/bison)
NS_TRIPLE := aarch64-unknown-zoraya
NETSURF := $(WORK)/netsurf-all-$(call lock_ver,netsurf)
NS_STAMP := $(OBJ)/netsurf-libs.stamp
NETSURF_BIN := $(NETSURF)/netsurf/nsfb
# The NetSurf libraries, in dependency order (libsvgtiny, librosprite and the RISC OS
# ones are left out).
NS_LIBS := buildsystem libnslog libwapcaplet libparserutils libcss libhubbub libdom libnsbmp libnsgif libnsutils libutf8proc libnspsl libnsfb
# What every NetSurf make runs with: no host pkg-config path (only $(DEPS)'s), and the
# tools of the cross environment first. HOST and BUILD both name the target, so that
# the buildsystem takes its native path (its cross path wants a GCC toolchain and
# /bin/which) and uses the CC and AR given.
NS_ENV := $(CROSS_ENV) PKG_CONFIG_LIBDIR=$(DEPS)/lib/pkgconfig PKG_CONFIG_PATH= \
	PATH=$(HOSTPFX)/bin:$(dir $(NS_PKGCONFIG)):$(TOOLBIN):$$PATH
NS_MK := HOST=$(NS_TRIPLE) BUILD=$(NS_TRIPLE) CC=$(ZCC) AR=$(TOOLBIN)/aarch64-unknown-zoraya-ar \
	PREFIX=$(DEPS) WARNFLAGS='-Wall -W -Wno-error' Q=@ V=1

host-pkgconfig: $(HOSTPFX)/bin/pkg-config
BISON_SRC := $(WORK)/bison-$(call lock_ver,bison)
$(BISON_SRC): $(LOCK)
	$(call unpack,bison,ports/patches/none)
	@touch $@
$(HOSTPFX)/bin/bison: $(BISON_SRC)
	@cd $(BISON_SRC) && $(call logged,env CC=$(HOSTCC) ./configure --prefix=$(HOSTPFX) --disable-nls --disable-yacc,host-bison-configure)
	@$(call logged,$(MAKE) -C $(BISON_SRC) -j3 install,host-bison)
PKGCONFIG_SRC := $(WORK)/pkg-config-$(call lock_ver,pkgconfig)
$(PKGCONFIG_SRC): $(LOCK)
	$(call unpack,pkgconfig,ports/patches/none)
	@touch $@
# glib is the one inside the archive; 0.29.2's sources use constructs modern clang
# rejects by default, hence the flags.
$(HOSTPFX)/bin/pkg-config: $(PKGCONFIG_SRC)
	@cd $(PKGCONFIG_SRC) && $(call logged,env CC=$(HOSTCC) CFLAGS="-O1 -Wno-int-conversion -Wno-error=int-conversion -Wno-incompatible-function-pointer-types" \
	  ./configure --prefix=$(HOSTPFX) --with-internal-glib --disable-host-tool,host-pkgconfig-configure)
	@$(call logged,$(MAKE) -C $(PKGCONFIG_SRC) -j3 install,host-pkgconfig)

# zlib and libpng for the host (NetSurf's build tools).
HOSTZLIB := $(WORK)/hostzlib-$(call lock_ver,zlib)
$(HOSTZLIB): $(LOCK)
	$(call unpack,zlib,ports/patches/none)
	@touch $@
HOSTPNG := $(WORK)/hostlibpng-$(call lock_ver,libpng)
$(HOSTPNG): $(LOCK)
	$(call unpack,libpng,ports/patches/none)
	@touch $@
$(HOSTPFX)/lib/libpng16.a: $(HOSTZLIB) $(HOSTPNG)
	@cd $(HOSTZLIB) && $(call logged,env CC=$(HOSTCC) ./configure --static --prefix=$(HOSTPFX),host-zlib-configure)
	@$(call logged,$(MAKE) -C $(HOSTZLIB) install,host-zlib)
	@cd $(HOSTPNG) && $(call logged,env CC=$(HOSTCC) CPPFLAGS=-I$(HOSTPFX)/include LDFLAGS=-L$(HOSTPFX)/lib \
	  ./configure --prefix=$(HOSTPFX) --disable-shared,host-libpng-configure)
	@$(call logged,$(MAKE) -C $(HOSTPNG) -j3 install,host-libpng)

# libpng and libjpeg for NetSurf.
LIBPNG := $(WORK)/libpng-$(call lock_ver,libpng)
$(LIBPNG): $(LOCK) $(wildcard ports/patches/libpng/*.patch)
	$(call unpack,libpng,ports/patches/libpng)
	@touch $@
$(DEPS)/lib/libpng16.a: $(LIBPNG) $(DEPS)/lib/libz.a $(SYSROOT_STAMP) ports/config.site
	@cd $(LIBPNG) && $(call logged,$(CROSS_ENV) ./configure --host=aarch64-unknown-linux-musl --prefix=$(DEPS) --disable-shared --enable-static \
	  --enable-arm-neon=off $(CROSS_TOOLS) CFLAGS="$(APPS_CFLAGS)" CPPFLAGS="-I$(DEPS)/include" LDFLAGS="-L$(DEPS)/lib",libpng-configure)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(LIBPNG) -j3 libpng16.la install-libLTLIBRARIES install-pkgconfigDATA \
	  install-pkgincludeHEADERS install-nodist_pkgincludeHEADERS install-header-links install-library-links install-libpng-pc,libpng)
	@touch $@
LIBJPEG := $(WORK)/jpeg-$(call lock_ver,libjpeg)
$(LIBJPEG): $(LOCK) $(wildcard ports/patches/libjpeg/*.patch)
	$(call unpack,libjpeg,ports/patches/libjpeg)
	@touch $@
$(DEPS)/lib/libjpeg.a: $(LIBJPEG) $(SYSROOT_STAMP) ports/config.site
	@cd $(LIBJPEG) && $(call logged,$(CROSS_ENV) ./configure --host=aarch64-unknown-linux-musl --prefix=$(DEPS) --disable-shared --enable-static \
	  $(CROSS_TOOLS) CFLAGS="$(APPS_CFLAGS)",libjpeg-configure)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(LIBJPEG) -j3 install-libLTLIBRARIES install-includeHEADERS,libjpeg)
	@cp $(LIBJPEG)/jconfig.h $(DEPS)/include/
	@touch $@

# libcurl for NetSurf: the library, its headers and its .pc, from the tree `make curl`
# built. libcurl.pc's `Libs:` also names what a static link needs (-lssl ...), which
# pkg-config would leave in `Libs.private`.
$(DEPS)/lib/libcurl.a: $(CURL_BIN)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(CURL)/lib install prefix=$(DEPS),libcurl-install)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(CURL)/include install prefix=$(DEPS),libcurl-include-install)
	@$(call logged,$(CROSS_ENV) $(MAKE) -C $(CURL) install-pkgconfigDATA prefix=$(DEPS),libcurl-pc-install)
	sed -i.bak 's|^Libs: .*|Libs: -L$${libdir} -lcurl -lssl -lcrypto -lz -lzshim|' $(DEPS)/lib/pkgconfig/libcurl.pc && rm -f $(DEPS)/lib/pkgconfig/libcurl.pc.bak
	@touch $@

netsurf-src: $(NETSURF)
$(NETSURF): $(LOCK) $(wildcard ports/patches/netsurf/*.patch) ports/netsurf/zoraya.c ports/netsurf/zoraya_window.h
	$(call unpack,netsurf,ports/patches/netsurf)
	cp ports/netsurf/zoraya.c ports/netsurf/zoraya_window.h $@/libnsfb/src/surface/
	@touch $@

# The libraries, one make each, installed into $(DEPS). Tests are not built.
netsurf-libs: $(NS_STAMP)
$(NS_STAMP): $(NETSURF) $(NS_PKGCONFIG) $(NS_BISON) $(DEPS)/lib/libpng16.a $(DEPS)/lib/libjpeg.a $(DEPS)/lib/libcurl.a $(SYSROOT_STAMP)
	@mkdir -p $(DEPS)/include $(DEPS)/lib/pkgconfig $(OBJ)
	@$(call logged,for l in $(NS_LIBS); do echo "== $$l"; $(NS_ENV) $(MAKE) -C $(NETSURF)/$$l install $(NS_MK) \
	  WITH_EXPAT_BINDING=no WITH_LIBXML_BINDING=no || exit 1; done,netsurf-libs)
	@touch $@

# NetSurf itself. Makefile.config is ours (ports/netsurf/Makefile.config); the build
# tools (convert_image and others) are host programs linked against $(HOSTPFX).
NETSURF_MK := TARGET=framebuffer HOST=zoraya CC=$(ZCC) PREFIX=/ ZORAYA_DEPS=$(DEPS) \
	BUILD_CC=$(HOSTCC) BUILD_CFLAGS="-g -W -Wall -Wno-unused-parameter -I$(HOSTPFX)/include" \
	BUILD_LDFLAGS="-L$(HOSTPFX)/lib" BUILD_LIBPNG_LDFLAGS="-L$(HOSTPFX)/lib -lpng16 -lz -lm" Q=@ V=1 SHELL=/bin/bash
# The program and its resources into the volume's tree (`make image EXTRA_ROOT=build/rootfs-apps`):
# /bin/netsurf and /usr/share/netsurf/ (NETSURF_FB_RESPATH of Makefile.config).
NETSURF_STAMP := $(OBJ)/netsurf.stamp
NETSURF_RES := adblock.css default.css internal.css quirks.css netsurf.png favicon.png welcome.html credits.html licence.html
netsurf: $(NETSURF_STAMP)
$(NETSURF_STAMP): $(NETSURF_BIN)
	mkdir -p $(APPS_ROOT)/bin $(APPS_ROOT)/usr/share/netsurf
	cp $(NETSURF_BIN) $(APPS_ROOT)/bin/netsurf
	for f in $(NETSURF_RES); do cp $(NETSURF)/netsurf/frontends/framebuffer/res/$$f $(APPS_ROOT)/usr/share/netsurf/; done
	cp $(NETSURF)/netsurf/frontends/framebuffer/res/en/Messages $(APPS_ROOT)/usr/share/netsurf/Messages
	$(ELF_CHECK) $(APPS_ROOT)/bin
	@echo "== no NetSurf code was compiled with __linux__ (INV-0318-2)"
	@# A missing (or renamed) build log fails: the check never passes on nothing (9d re-review, N3).
	@for l in netsurf-libs netsurf; do \
	  if [ ! -s $(LOGS)/$$l.log ]; then echo "INV-0318-2: $(LOGS)/$$l.log is missing; the check cannot run"; exit 1; fi; \
	  if grep -l -e '-D__linux' -e '-D__gnu_linux' $(LOGS)/$$l.log; then exit 1; fi; done
	@touch $@
$(DEPS)/lib/libnsshim.a: ports/apps/nsshim.c $(SYSROOT_STAMP)
	@mkdir -p $(DEPS)/lib $(OBJ)/apps
	$(ZCC) $(APPS_CFLAGS) -c -o $(OBJ)/apps/nsshim.o ports/apps/nsshim.c
	rm -f $@ && $(TOOLBIN)/aarch64-unknown-zoraya-ar rcs $@ $(OBJ)/apps/nsshim.o
$(NETSURF_BIN): $(NS_STAMP) $(HOSTPFX)/lib/libpng16.a $(DEPS)/lib/libzshim.a $(DEPS)/lib/libnsshim.a ports/netsurf/Makefile.config
	cp ports/netsurf/Makefile.config $(NETSURF)/netsurf/Makefile.config
	@$(call logged,$(NS_ENV) $(MAKE) -C $(NETSURF)/netsurf -j3 $(NETSURF_MK),netsurf)

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
# The thread suites of part 9b (ADR-0309): musl's libc-test and SQLite's source
# tree (its test/threadtest3.c), unpacked for `make test-posix-suites`. Test
# inputs only: nothing of them enters libc.a or the rootfs.
suites: $(WORKLOADS)/libctest $(WORKLOADS)/sqlitesrc
$(WORKLOADS)/libctest: $(LOCK)
	@mkdir -p $(WORKLOADS)
	$(call unpack,libctest,ports/patches/libctest)
	@touch $@
$(WORKLOADS)/sqlitesrc: $(LOCK)
	@mkdir -p $(WORKLOADS)
	$(call unpack,sqlitesrc,ports/patches/sqlitesrc)
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
