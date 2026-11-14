name     true
summary  do nothing, succeed
usage    true
program  /bin/true
exit     0  always
exit     1  never; only if true cannot run
---
Do nothing, succeed: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).

EXAMPLES
  true && echo ok

SEE ALSO
  test

Lets through: (no options)
