name     count
summary  count arguments, or the lines of files
usage    count [VALUE...]
usage    count --lines FILE...
operand  [VALUE...]  text  what to count; with --lines, the files whose lines to count
option   -l --lines  count the lines of the files named, together
when     --lines VALUE read
exit     0  counted
exit     1  a file could not be read
---
Prints how many arguments it was given: count $files is the number of values in files.
With --lines, prints the number of lines in the named files together.

EXAMPLES
  count $files
  count --lines notes.txt

SEE ALSO
  set
