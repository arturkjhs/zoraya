name     vi
summary  edit text files (nvi, a POSIX program)
usage    vi [-R] [-c COMMAND] [FILE...]
program  /bin/vi
cwd      none
operand  [FILE...]  create  the files to edit; each may be created or replaced
option   -R --readonly  open the files to read only
when     -R FILE read
option   -c --command COMMAND text  an ex command to run first
exit     0  done
exit     1  failed
---
vi is the editor of a terminal: nvi 2.2.2 (BSD-3-Clause) as a POSIX program with the toolchain set (ADR-0335). It needs the
terminal; on the serial console use ex.
cwd none (ADR-0335 item 3): the line gives vi the files it names and no other name of their directory. :e of another
file, :w to another name and :r of another file are refused and the terminal says so; name them on the line, or give
the directory (+w . vi notes.txt). With no file on the line vi starts in /bin, read only, and :w notes.txt is refused.
Saving is in place and not atomic: :w cuts the file and writes it again, so a crash, a kill or a full volume during :w
can leave it short, and nothing else holds a copy. There is no recovery (vi -r is not offered) and no start-up file
(.exrc is never read; EXINIT and NEXINIT are, if exported). :! and filters run /bin/sh with vi's own grants, no more.
Write the short options: the long names belong to the description.

EXAMPLES
  vi notes.txt
  vi -R /config/settings
  vi -c '/TODO' todo.txt

SEE ALSO
  ex, view, less, edit

Lets through: -R -c
