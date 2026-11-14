name     sed
summary  edit a stream
usage    sed [-nEr] [-i] SCRIPT [FILE...]
program  /bin/sed
cwd      none
operand  SCRIPT  text  the commands: 's/old/new/g', '/x/d'
operand  [FILE...]  read  the files to edit, standard input if none
option   -n --quiet  print only what p prints
option   -i --in-place  change the files themselves (no backup suffix)
option   -E --regexp-extended  extended expressions
option   -r --regex-extended  the same as -E
when     -i FILE write
exit     0  done
exit     1  failed
---
Edit a stream: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
cwd none (ADR-0336 item 6): each FILE arrives narrowed to the name written, read, or with -i changed in
place (write). The change is not atomic. -e and -f are not listed: SCRIPT is always the first operand.
Write the short options: the long names belong to the description.

EXAMPLES
  sed 's/foo/bar/g' in.txt
  sed -i 's/foo/bar/' config.txt
  sed -n '1,5p' notes.txt

SEE ALSO
  grep, tr, awk

Lets through: -n -i -E -r
