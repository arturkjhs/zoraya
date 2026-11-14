name     cxxtest
summary  the test program of the C++ runtime (a POSIX program built by zc++)
usage    cxxtest MODE [ARG...]
program  /bin/cxxtest
operand  MODE  text  the group to run: exceptions, rtti, statics, alloc, threads, tls, exit-order, iostream DIR, fsops DIR, fs DIR..., uncaught, noexcept, terminate, hardening
operand  [ARG...]  text  the mode's arguments; a directory a mode reads or writes needs a +r or +w of the line, or the program's own /tmp
exit     0  the group passed
exit     1  the group failed, or a directory could not be listed
exit     2  a usage mistake
---
cxxtest is the test program of TASK-30 (proposed ADR-0360 to ADR-0366): C++ built by zc++ against libc++, libc++abi and
libunwind, run as a POSIX program. Each mode runs one group of checks and prints "cxxtest: MODE passed": exceptions
thrown across frames and threads, RTTI, static constructors and destructors, std::thread and its kin, thread_local
destructors, iostreams, the C locale, std::filesystem. fs lists the directories it is given, which it can read only
when the line grants them (+r). uncaught, noexcept and terminate end the program as an exception nobody catches does:
libc++abi's message on standard error and abort().

EXAMPLES
  cxxtest exceptions
  +r /work/granted cxxtest fs /work/granted /work/secret
