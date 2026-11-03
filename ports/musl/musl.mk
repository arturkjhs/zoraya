# Builds musl's retained files into objects (ADR-0100 item 2). Run by
# ports/Makefile once the source is unpacked and patched, with:
#   MUSL     the patched musl source tree
#   OBJ      where the objects go
#   ZCC      tools/zcc
#   LIST     where the list of objects is written
#
# Retained: every .c of the directories below, with musl's own rule that an
# arch/aarch64 file replaces the generic one of the same name, minus the files
# ports/musl/excluded.txt names. ADR-0141 gives the rule.

RETAINED_DIRS := string ctype multibyte locale stdlib math regex search prng \
	setjmp errno misc passwd fenv complex stdio time env exit conf temp internal
# musl's start-up needs these from directories that are otherwise replaced
# (ADR-0100 item 2).
EXCEPTIONS := src/thread/aarch64/__set_thread_area.s

EXCLUDED := $(shell sed -e 's/\#.*//' -e '/^[[:space:]]*$$/d' ports/musl/excluded.txt)

BASE_SRCS := $(sort $(foreach d,$(RETAINED_DIRS),$(wildcard $(MUSL)/src/$(d)/*.c)))
ARCH_SRCS := $(sort $(foreach d,$(RETAINED_DIRS),$(wildcard $(MUSL)/src/$(d)/aarch64/*.[csS]))) \
	$(addprefix $(MUSL)/,$(EXCEPTIONS))
BASE_OBJS := $(patsubst $(MUSL)/%,%.o,$(basename $(BASE_SRCS)))
ARCH_OBJS := $(patsubst $(MUSL)/%,%.o,$(basename $(ARCH_SRCS)))
REPLACED := $(sort $(subst /aarch64/,/,$(ARCH_OBJS)))
EXCLUDED_OBJS := $(addsuffix .o,$(basename $(EXCLUDED)))
OBJS := $(addprefix $(OBJ)/,$(filter-out $(REPLACED) $(EXCLUDED_OBJS),$(sort $(BASE_OBJS) $(ARCH_OBJS))))

# musl's configure's flags for clang, with -O2 -g as measured (ADR-0109 item 1).
CFLAGS := -std=c99 -ffreestanding -fexcess-precision=standard -frounding-math \
	-fno-strict-aliasing -Wa,--noexecstack -D_XOPEN_SOURCE=700 \
	-I$(MUSL)/arch/aarch64 -I$(MUSL)/arch/generic -I$(OBJ)/src/internal \
	-I$(MUSL)/src/include -I$(MUSL)/src/internal -I$(MUSL)/include \
	-O2 -g -pipe -fomit-frame-pointer -fno-unwind-tables -fno-asynchronous-unwind-tables \
	-ffunction-sections -fdata-sections -w -Wno-pointer-to-int-cast \
	-Werror=implicit-function-declaration -Werror=implicit-int -Werror=pointer-sign \
	-Werror=pointer-arith -Werror=int-conversion -Werror=incompatible-pointer-types

all: $(LIST)

# The list also follows the selection rule, so that a file dropped from it
# leaves libc.a even when its object is still on disk.
$(LIST): $(OBJS) ports/musl/musl.mk ports/musl/excluded.txt
	@printf '%s\n' $(OBJS) > $@

$(OBJ)/src/internal/version.h:
	@mkdir -p $(dir $@)
	printf '#define VERSION "1.2.5"\n' > $@

$(OBJ)/%.o: $(MUSL)/%.c $(OBJ)/src/internal/version.h
	@mkdir -p $(dir $@)
	$(ZCC) $(CFLAGS) -c -o $@ $<

$(OBJ)/%.o: $(MUSL)/%.s
	@mkdir -p $(dir $@)
	$(ZCC) $(CFLAGS) -c -o $@ $<

$(OBJ)/%.o: $(MUSL)/%.S
	@mkdir -p $(dir $@)
	$(ZCC) $(CFLAGS) -c -o $@ $<

# INV-0141-2: every excluded file is refused by the shim (compiling it fails).
check-excluded: $(OBJ)/src/internal/version.h
	@for f in $(EXCLUDED); do \
	  [ -f $(MUSL)/$$f ] || { echo "musl: $$f is excluded but does not exist"; exit 1; }; \
	  if $(ZCC) $(CFLAGS) -c -o /dev/null $(MUSL)/$$f > $(OBJ)/excluded.err 2>&1; then \
	    echo "musl: $$f is excluded but compiles with the shim"; exit 1; fi; \
	  grep -q -e "undeclared function '__zoraya_sys_" -e "undeclared function '__sys_open" \
	    -e "undeclared function 'sys_open" -e "undeclared function '__sys_wait4" \
	    -e "undeclared identifier 'SIOCGSTAMP_OLD'" $(OBJ)/excluded.err || \
	    { echo "musl: $$f fails for another reason than the shim:"; cat $(OBJ)/excluded.err; exit 1; }; done
	@echo "musl: all $(words $(EXCLUDED)) excluded files are refused by the shim"
