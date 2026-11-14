name     cpio
summary  copy files in and out of archives
usage    cpio (-i | -o | -t) [-dv]
program  /bin/cpio
option   -i --extract  extract an archive from standard input
option   -o --create  make an archive on standard output, from the names read on standard input
option   -t --list  list the archive on standard input
option   -d --make-directories  make directories as needed
option   -v --verbose  list the files as they are handled
exit     0  done
exit     1  failed
---
Copy files in and out of archives: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
The archive is standard input or output (use < and >). -F, -p and -R are not listed. Extracting needs +w on where
it writes; -o reads the files named on its input, so they need +r.
Write the short options: the long names belong to the description.

EXAMPLES
  find . | cpio -o > ../a.cpio
  cpio -i -d < a.cpio    (with +w .)

SEE ALSO
  tar, find

Lets through: -i -o -t -d -v
