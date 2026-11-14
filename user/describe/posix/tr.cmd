name     tr
summary  translate or delete characters
usage    tr [-cCds] SET1 [SET2]
program  /bin/tr
operand  SET1  text  the characters to change
operand  [SET2]  text  what they become
option   -c --complement  use the characters not in SET1
option   -C --complement-chars  the same, by characters
option   -d --delete  delete the characters in SET1
option   -s --squeeze-repeats  squeeze repeated characters
exit     0  done
exit     1  failed
---
Translate or delete characters: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
toybox's tr is from its pending directory, admitted by ADR-0336 item 2.
Write the short options: the long names belong to the description.

EXAMPLES
  tr a-z A-Z < in.txt
  tr -d '\r' < dos.txt

SEE ALSO
  sed, cut

Lets through: -c -C -d -s
