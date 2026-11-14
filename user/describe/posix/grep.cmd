name     grep
summary  search for a pattern
usage    grep [-EFHLRcilnoqrsvwx] [-h] [-A N] [-B N] [-C N] PATTERN [FILE...]
program  /bin/grep
operand  PATTERN  text  the pattern to search for
operand  [FILE...]  read  the files to search, standard input if none (with -r, directories too)
option   -E --extended-regexp  PATTERN is an extended expression
option   -F --fixed-strings  PATTERN is a plain string
option   -i --ignore-case  match without case
option   -v --invert-match  select the lines that do not match
option   -c --count  print the number of matching lines
option   -l --files-with-matches  print only the names of files that match
option   -L --files-without-match  print only the names of files that do not match
option   -n --line-number  put the line number before each line
option   -o --only-matching  print only the matching part
option   -r --recursive  search directories and what is in them
option   -R --dereference-recursive  the same, following links
option   -s --no-messages  do not report unreadable files
option   -w --word-regexp  match whole words
option   -x --line-regexp  match whole lines
option   -q --quiet  print nothing, only set the exit status
option   -H --with-filename  put the file name before each line
option   -h --no-filename  leave the file name out
option   -A --after-context N text  also print N lines after a match
option   -B --before-context N text  also print N lines before a match
option   -C --context N text  also print N lines around a match
exit     0  a line matched
exit     1  no line matched
exit     2  an error
---
Search for a pattern: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
-e and -f (more patterns, a pattern file) are not listed: PATTERN is always the first operand.
Write the short options: the long names belong to the description.

EXAMPLES
  grep -rn TODO src
  grep -v '^#' config.txt

SEE ALSO
  sed, find, cut

Lets through: -E -F -i -v -c -l -L -n -o -r -R -s -w -x -q -H -h -A -B -C
