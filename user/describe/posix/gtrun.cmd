name     gtrun
summary  run one of GoogleTest's own test programs (a POSIX program, the test of the CMake toolchain file)
usage    gtrun NAME [FILTER]
program  /bin/gtrun
operand  NAME  text  the test program, a file of /bin/gt
operand  [FILTER]  text  the tests to run, as GoogleTest's --gtest_filter takes them (POSITIVE-NEGATIVE patterns)
exit     0  every test passed
exit     1  a test failed
exit     2  a usage mistake
exit     127  the program could not be executed
---
gtrun is the launcher of make test-cxx-gtest (TASK-30): GoogleTest 1.18.0 built by CMake with ports/cmake/zoraya.cmake, its
own test programs in /bin/gt. It executes /bin/gt/NAME, with --gtest_filter=FILTER when a filter is given, as a POSIX program with what the line gives
every POSIX program: /bin, /usr and /etc to read, a /tmp of its own, the current directory to read. The death tests fork and
execute the program again.

EXAMPLES
  gtrun gtest_unittest
  gtrun googletest-death-test-test 'TestForDeathTest.*'
