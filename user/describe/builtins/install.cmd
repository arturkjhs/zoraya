name     install
summary  install a program from a package directory, with the grants you type
usage    install PACKAGE [WORD...]
usage    install --keep PACKAGE [WORD...]
operand  PACKAGE  read  the package: a directory with the program NAME, its description NAME.cmd and, if it asks for grants, request
operand  [WORDS...]  text  the grants to save for it: +r DIR, +w DIR, +net HOST[:PORT], +web, +own ~/PATH
option   -k --keep  on an upgrade, keep the grants saved for the old package
cwd      none
exit     0  installed
exit     1  refused: nothing was installed
---
Copies the package to /apps/NAME and records its hash in /config/apps. The program is found
as NAME, with a help page, and starts only while its bytes are the ones installed. It gets no
grant of its own: the words you type on this line are saved, each as you typed it, and added
to every line that starts it. Nothing else is saved; a program cannot ask for more, and what
the package's request file asks for is only shown.

A package is a directory with NAME (an ELF program or a script that begins with #!),
NAME.cmd (its description, which gives help and completion and no access), optionally a
request file with one grant a line, and the files the program reads. NAME is a lower-case
letter, then letters, digits or -; an image command, a builtin, a function, a room or a
program of the path has it already, or it is refused. Installing a package of the same
name again replaces it and drops the saved grants, unless --keep is given.

install and programs work only in a shell that holds /apps and /config, never in an
untrusted room. A +w over /apps or / is refused on every line: installed programs are
changed with install.

EXAMPLES
  install ./ed-1.0
  install ./ed-1.0 +own ~/.config/ed +w ~/notes
  install --keep ./ed-1.1

SEE ALSO
  help programs
  help grants
