# CMake's platform module for Zoraya (TASK-30, proposed ADR-0367), found by
# ports/cmake/zoraya.cmake through CMAKE_MODULE_PATH. Zoraya runs POSIX
# programs through libposix (ADR-0100), so UNIX is set and the Unix path
# layout is used; every program is a static ET_EXEC (ADR-0033).
set(UNIX 1)
set(ZORAYA 1)
include(Platform/UnixPaths)

set_property(GLOBAL PROPERTY TARGET_SUPPORTS_SHARED_LIBS FALSE)
set(CMAKE_FIND_LIBRARY_PREFIXES "lib")
set(CMAKE_FIND_LIBRARY_SUFFIXES ".a")
set(CMAKE_STATIC_LIBRARY_PREFIX "lib")
set(CMAKE_STATIC_LIBRARY_SUFFIX ".a")
set(CMAKE_EXECUTABLE_SUFFIX "")
# No -rdynamic, no -fPIC or -fPIE for position-independent targets: zcc
# refuses the first and the programs are not position-independent.
set(CMAKE_SHARED_LIBRARY_LINK_C_FLAGS "")
set(CMAKE_SHARED_LIBRARY_LINK_CXX_FLAGS "")
set(CMAKE_C_COMPILE_OPTIONS_PIC "")
set(CMAKE_CXX_COMPILE_OPTIONS_PIC "")
set(CMAKE_C_COMPILE_OPTIONS_PIE "")
set(CMAKE_CXX_COMPILE_OPTIONS_PIE "")
set(CMAKE_C_LINK_OPTIONS_PIE "")
set(CMAKE_CXX_LINK_OPTIONS_PIE "")
set(CMAKE_C_LINK_OPTIONS_NO_PIE "")
set(CMAKE_CXX_LINK_OPTIONS_NO_PIE "")
