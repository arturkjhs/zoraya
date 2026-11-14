name     uname
summary  show the system name
usage    uname [-amnoprsv]
program  /bin/uname
option   -a --all  everything
option   -m --machine  the machine type
option   -n --nodename  the host name
option   -o --operating-system  the operating system
option   -p --processor  the processor
option   -r --kernel-release  the release
option   -s --kernel-name  the kernel name
option   -v --kernel-version  the version
exit     0  done
exit     1  failed
---
Show the system name: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  uname -sm

SEE ALSO
  date

Lets through: -a -m -n -o -p -r -s -v
