name     diff
summary  compare files
usage    diff [-NabBiqrsuw] [-U N] FILE1 FILE2
program  /bin/diff
operand  FILE1  read  the first file or directory
operand  FILE2  read  the second file or directory
option   -u --unified  unified output, 3 lines of context
option   -U --unified-context N text  unified output with N lines of context
option   -r --recursive  compare directories below
option   -N --new-file  treat a missing file as empty
option   -q --brief  only say whether the files differ
option   -i --ignore-case  ignore case
option   -b --ignore-space-change  ignore changes in white space
option   -w --ignore-all-space  ignore all white space
option   -B --ignore-blank-lines  ignore blank lines
option   -s --report-identical-files  say when the files are the same
option   -a --text  treat all files as text
exit     0  no differences
exit     1  differences
exit     2  an error
---
Compare files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
toybox's diff is from its pending directory, admitted by ADR-0336 item 2.
Write the short options: the long names belong to the description.

EXAMPLES
  diff -u old.c new.c
  diff -ru a b

SEE ALSO
  cmp, patch

Lets through: -u -U -r -N -q -i -b -w -B -s -a
