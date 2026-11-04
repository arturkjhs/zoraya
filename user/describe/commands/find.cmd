name     find
summary  find files by name
usage    find PATTERN [DIR]
operand  PATTERN  text  a pattern for the name: * any characters, ? one; quote it
operand  [DIR]    read  where to look; the current directory if none is given
exit     0  listed
exit     1  DIR could not be read
---
Prints every file and directory in DIR and below whose name matches PATTERN, as paths from DIR,
sorted byte by byte. Names starting with . match only a pattern that starts with one.

EXAMPLES
  find '*.txt'
  find 'notes*' project

SEE ALSO
  ls, search
