# Builds LLVM's C++ runtime for aarch64-unknown-zoraya (TASK-30, proposed
# ADR-0360, ADR-0361): libunwind, libc++abi and libc++, static, by tools/zc++
# against build/sysroot's libc.a. Run by ports/Makefile once the three source
# trees are unpacked and patched, with:
#   SRC      the directory that holds libcxx/, libcxxabi/ and libunwind/
#   OBJ      where the objects go
#   ZCXX     tools/zc++ (compiles; the C sources of libunwind with -x c)
#   ZAR      zoraya-ar
#   SYSROOT  build/sysroot: the headers go to usr/include/c++/v1 (libc++ and
#            libc++abi) and usr/include (libunwind's), the archives to usr/lib
#
# The file lists are those of each project's src/CMakeLists.txt for a static
# build with exceptions, RTTI, threads, localization, std::filesystem and the
# random device, and without the time zone database, the experimental library
# or new/delete in libc++ (libc++abi defines them). The flags are those
# LLVM's CMake passes for that build, with -O2 and no debug information
# (ADR-0360: sizes in docs/design.md).

LIBCXX := $(SRC)/libcxx
LIBCXXABI := $(SRC)/libcxxabi
LIBUNWIND := $(SRC)/libunwind
INC := $(SYSROOT)/usr/include
CXXINC := $(INC)/c++/v1
LIB := $(SYSROOT)/usr/lib

COMMON := -O2 -DNDEBUG -ffunction-sections -fdata-sections -funwind-tables -w

UNWIND_CXX := libunwind.cpp Unwind-EHABI.cpp Unwind-seh.cpp
UNWIND_C := UnwindLevel1.c UnwindLevel1-gcc-ext.c Unwind-sjlj.c Unwind-wasm.c
UNWIND_S := UnwindRegistersRestore.S UnwindRegistersSave.S
# No dladdr (libc.a has no dynamic linker, ADR-0100): unw_get_proc_name finds
# no names, which only libunwind's own logging and backtraces would show.
UNWIND_FLAGS := $(COMMON) -I$(LIBUNWIND)/include -D_LIBUNWIND_IS_NATIVE_ONLY -D_LIBUNWIND_USE_DLADDR=0 \
	-D_LIBUNWIND_DISABLE_VISIBILITY_ANNOTATIONS
UNWIND_OBJS := $(addprefix $(OBJ)/unwind/,$(UNWIND_CXX:.cpp=.o) $(UNWIND_C:.c=.o) $(UNWIND_S:.S=.o))

ABI_SRCS := cxa_aux_runtime.cpp cxa_default_handlers.cpp cxa_demangle.cpp cxa_exception_storage.cpp \
	cxa_guard.cpp cxa_handlers.cpp cxa_vector.cpp cxa_virtual.cpp stdlib_exception.cpp \
	stdlib_stdexcept.cpp stdlib_typeinfo.cpp abort_message.cpp fallback_malloc.cpp private_typeinfo.cpp \
	stdlib_new_delete.cpp cxa_exception.cpp cxa_personality.cpp cxa_thread_atexit.cpp
ABI_FLAGS := $(COMMON) -std=c++23 -nostdinc++ -isystem $(CXXINC) -I$(LIBCXXABI)/include -I$(LIBCXX)/src \
	-I$(LIBUNWIND)/include -D_LIBCXXABI_BUILDING_LIBRARY -D_LIBCPP_BUILDING_LIBRARY \
	-D_LIBCXXABI_DISABLE_VISIBILITY_ANNOTATIONS -D_LIBCPP_DISABLE_VISIBILITY_ANNOTATIONS= \
	-DLIBCXXABI_USE_LLVM_UNWINDER -D_XOPEN_SOURCE=700 -fstrict-aliasing
ABI_OBJS := $(addprefix $(OBJ)/abi/,$(ABI_SRCS:.cpp=.o))

CXX_SRCS := algorithm.cpp any.cpp bind.cpp call_once.cpp charconv.cpp chrono.cpp error_category.cpp \
	exception.cpp expected.cpp filesystem/filesystem_clock.cpp filesystem/filesystem_error.cpp \
	filesystem/path.cpp functional.cpp hash.cpp memory.cpp memory_resource.cpp new_handler.cpp \
	new_helpers.cpp optional.cpp print.cpp random_shuffle.cpp ryu/d2fixed.cpp ryu/d2s.cpp ryu/f2s.cpp \
	stdexcept.cpp string.cpp system_error.cpp typeinfo.cpp valarray.cpp variant.cpp vector.cpp \
	verbose_abort.cpp \
	atomic.cpp barrier.cpp condition_variable_destructor.cpp condition_variable.cpp future.cpp \
	mutex_destructor.cpp mutex.cpp shared_mutex.cpp thread.cpp \
	random.cpp \
	fstream.cpp ios.cpp ios.instantiations.cpp iostream.cpp locale.cpp ostream.cpp regex.cpp strstream.cpp \
	filesystem/directory_entry.cpp filesystem/directory_iterator.cpp filesystem/operations.cpp \
	filesystem/int128_builtins.cpp
CXX_FLAGS := $(COMMON) -std=c++23 -nostdinc++ -isystem $(CXXINC) -I$(LIBCXX)/src -I$(LIBCXXABI)/include -I$(SRC)/libc \
	-D_LIBCPP_BUILDING_LIBRARY -D_LIBCPP_REMOVE_TRANSITIVE_INCLUDES -DLIBCXX_BUILDING_LIBCXXABI -DLIBC_NAMESPACE=__llvm_libc_common_utils \
	-D_LIBCPP_DISABLE_VISIBILITY_ANNOTATIONS= -fvisibility-inlines-hidden -faligned-allocation
CXX_OBJS := $(addprefix $(OBJ)/cxx/,$(CXX_SRCS:.cpp=.o))

# The names libc.a does not export that libc++ needs, defined in libc++.a
# itself (ADR-0366): each does what the call says through libposix, or fails
# as the call would where it has no meaning.
SHIM_OBJ := $(OBJ)/cxx/zoraya-shim.o

all: $(LIB)/libunwind.a $(LIB)/libc++abi.a $(LIB)/libc++.a $(OBJ)/whole.check

# INV-0366-1: every member of the three archives links against libc.a with no
# undefined name (a program that links all of them, not only what one program
# reaches). INV-0366-2: the shim defines only its five names, each weak.
SHIM_NAMES := W link W pathconf W sched_yield W statvfs W truncate
$(OBJ)/whole.check: $(LIB)/libunwind.a $(LIB)/libc++abi.a $(LIB)/libc++.a
	printf 'int main(void) { return 0; }\n' > $(OBJ)/whole-main.c
	$(ZCXX) -x c -c -o $(OBJ)/whole-main.o $(OBJ)/whole-main.c
	$(ZCXX) -nostdlib++ -o $(OBJ)/whole.elf $(OBJ)/whole-main.o -Wl,--error-limit=0 \
	  -Wl,--whole-archive $(LIB)/libc++.a $(LIB)/libc++abi.a $(LIB)/libunwind.a -Wl,--no-whole-archive
	@# Defined global names only: GNU nm also lists AArch64's local mapping
	@# symbols ($$x, $$d: types t, r, n), which Apple's does not.
	nm $(SHIM_OBJ) | awk 'NF == 3 && $$2 ~ /^[A-Z]$$/ { print $$2, $$3 }' | sort > $(OBJ)/shim.nm
	@[ "$$(tr '\n' ' ' < $(OBJ)/shim.nm)" = "$(SHIM_NAMES) " ] || \
	  { echo "INV-0366-2: the shim defines other names, or not weakly:"; cat $(OBJ)/shim.nm; exit 1; }
	@# INV-0109-2 for zc++: it refuses -D__linux__ and defines nothing of
	@# Linux in C++ (the g++ driver mode, _GNU_SOURCE).
	@if $(ZCXX) -D__linux__ -E -x c++ /dev/null > /dev/null 2>&1; then echo "zc++ accepted -D__linux__"; exit 1; fi
	@if $(ZCXX) -dM -E -x c++ /dev/null | grep -E '__linux|__gnu_linux'; then echo "zc++ defines a Linux macro"; exit 1; fi
	@# INV-0361-1: a -nodefaultlibs link that names the runtime itself still
	@# gets --eh-frame-hdr.
	@$(ZCXX) -nodefaultlibs -### -o $(OBJ)/whole-ndl.elf $(OBJ)/whole-main.o 2>&1 | grep -q -e '--eh-frame-hdr' || \
	  { echo "INV-0361-1: a -nodefaultlibs link of zc++ has no --eh-frame-hdr"; exit 1; }
	@touch $@

$(OBJ)/unwind/%.o: $(LIBUNWIND)/src/%.cpp
	@mkdir -p $(dir $@)
	$(ZCXX) $(UNWIND_FLAGS) -std=c++17 -fno-exceptions -fno-rtti -nostdinc++ -c -o $@ $<
$(OBJ)/unwind/%.o: $(LIBUNWIND)/src/%.c
	@mkdir -p $(dir $@)
	$(ZCXX) -x c $(UNWIND_FLAGS) -std=c99 -fexceptions -c -o $@ $<
$(OBJ)/unwind/%.o: $(LIBUNWIND)/src/%.S
	@mkdir -p $(dir $@)
	$(ZCXX) -x assembler-with-cpp $(UNWIND_FLAGS) -c -o $@ $<
$(OBJ)/abi/%.o: $(LIBCXXABI)/src/%.cpp
	@mkdir -p $(dir $@)
	$(ZCXX) $(ABI_FLAGS) -c -o $@ $<
$(OBJ)/cxx/%.o: $(LIBCXX)/src/%.cpp
	@mkdir -p $(dir $@)
	$(ZCXX) $(CXX_FLAGS) -c -o $@ $<
$(SHIM_OBJ): ports/cxx/zoraya-shim.c
	@mkdir -p $(dir $@)
	$(ZCXX) -x c -O2 -Wall -Werror -c -o $@ $<

$(LIB)/libunwind.a: $(UNWIND_OBJS)
	rm -f $@ && $(ZAR) rcs $@ $^
$(LIB)/libc++abi.a: $(ABI_OBJS)
	rm -f $@ && $(ZAR) rcs $@ $^
$(LIB)/libc++.a: $(CXX_OBJS) $(SHIM_OBJ)
	rm -f $@ && $(ZAR) rcs $@ $^

.PHONY: all
