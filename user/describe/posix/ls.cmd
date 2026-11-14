name     ls
summary  list files
usage    ls [-AFRSadhilprst1] [PATH...]
program  /bin/ls
operand  [PATH...]  read  the files or directories to list, the current directory if none
option   -a --all  show names that start with .
option   -A --almost-all  the same, without . and ..
option   -l --long  one line per file with mode, owner, size and time
option   -1 --one-per-line  one name per line
option   -d --directory  list a directory itself, not what is in it
option   -h --human-readable  sizes as 1K, 2M
option   -R --recursive  list directories below as well
option   -F --classify  add / * @ after names
option   -p --slash  add / after directories
option   -r --reverse  sort the other way
option   -t --time  sort by time
option   -S --size  sort by size
option   -s --size-blocks  show the blocks used
option   -i --inode  show inode numbers
exit     0  done
exit     1  failed
---
ls is a native command in zosh; /bin/ls is toybox's, for scripts (type /bin/ls to run it).
List files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  ls -l
  ls -a /bin

SEE ALSO
  stat, find, du

Lets through: -a -A -l -1 -d -h -R -F -p -r -t -S -s -i
