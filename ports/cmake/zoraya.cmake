# CMake toolchain file for aarch64-unknown-zoraya (TASK-30, proposed ADR-0367):
# a CMake project cross-builds for Zoraya with
#
#   cmake -DCMAKE_TOOLCHAIN_FILE=ports/cmake/zoraya.cmake -S SRC -B BUILD
#
# from this repository's checkout, once `make -f ports/Makefile cxx` has built
# the sysroot (libc.a, the crt objects, libc++, libc++abi, libunwind) and the
# triple-prefixed tools in build/ports/bin. C goes through tools/zcc, C++
# through tools/zc++ (ADR-0109, ADR-0360). Programs are static ET_EXEC
# (ADR-0033): there are no shared libraries, so add_library(... SHARED) and
# MODULE are errors, and find_library finds only .a files. Nothing built runs
# on the host: try_run needs an answer in the cache (CMAKE_CROSSCOMPILING).
set(CMAKE_SYSTEM_NAME Zoraya)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_SYSTEM_VERSION 1)

get_filename_component(ZORAYA_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
set(ZORAYA_SYSROOT "${ZORAYA_ROOT}/build/sysroot")

# Platform/Zoraya.cmake, beside this file.
list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_LIST_DIR}")

set(CMAKE_C_COMPILER "${ZORAYA_ROOT}/tools/zcc")
set(CMAKE_CXX_COMPILER "${ZORAYA_ROOT}/tools/zc++")
set(CMAKE_ASM_COMPILER "${ZORAYA_ROOT}/tools/zcc")
set(CMAKE_AR "${ZORAYA_ROOT}/build/ports/bin/aarch64-unknown-zoraya-ar" CACHE FILEPATH "zoraya-ar")
set(CMAKE_RANLIB "${ZORAYA_ROOT}/build/ports/bin/aarch64-unknown-zoraya-ranlib" CACHE FILEPATH "zoraya-ar")

# Headers, libraries and packages from the sysroot only; programs (code
# generators a build runs) from the host.
set(CMAKE_FIND_ROOT_PATH "${ZORAYA_SYSROOT}")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# The compiler checks link a whole static program: that works, and it
# checks the sysroot as well as the compiler.
set(CMAKE_TRY_COMPILE_TARGET_TYPE EXECUTABLE)
