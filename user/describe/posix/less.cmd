name     less
summary  page through files or a pipe (less)
usage    less [-FiINRSsX] [FILE...]
program  /bin/less
cwd      none
operand  [FILE...]  read  the files to page through, standard input if none
option   -F --quit-if-one-screen  quit at once if the text fits on one screen
option   -i --ignore-case  searches ignore case unless the pattern has capitals
option   -I --ignore-case-always  searches ignore case always
option   -N --line-numbers  show line numbers
option   -R --raw-control-chars  show colour sequences as colour
option   -S --chop-long-lines  cut long lines instead of wrapping them
option   -s --squeeze-blank-lines  show runs of blank lines as one
option   -X --no-init  do not use the alternate screen
exit     0  done
exit     1  failed
---
less 710 (the Less License) as a POSIX program with the toolchain set (ADR-0335 item 8). It pages on the terminal; with its input from a pipe
(tcc -v &| less) it reads its keys from the terminal, not from the pipe.
cwd none: the line gives less the files it names and nothing else. It has no history file, no key files, no tags and no
input preprocessor (LESSOPEN is ignored); v starts vi and ! a shell, each with less's own grants and no more.
Keys: space and b page, / and ? search, n and N repeat, g and G go to the ends, q quits, h shows less's own help.
Put the options first, and write a command as an option (no +G or +/text): a word that starts with + is refused.
Two files of one directory are refused until the grant can name both: give the directory (+r . less a b) or name one.
Write the short options: the long names belong to the description.

EXAMPLES
  less big.log
  less -N notes.txt
  tcc -v &| less

SEE ALSO
  vi, cat

Lets through: -F -i -I -N -R -S -s -X
