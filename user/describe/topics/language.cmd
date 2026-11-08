topic    language
summary  how zosh reads what you type
---
A line is commands separated by ; or a new line. A command is words separated by spaces.
# starts a comment to the end of the line.

QUOTING
  'text'        exactly as written; it cannot hold a '
  "text"        one word in which $name and $(...) are replaced; \" \\ \$ \n \t are escapes
  \c            the character c itself, outside quotes

VARIABLES
  set name value...    a variable holds a list; set name alone makes it empty
  $name                one argument per value: never split at spaces, never a pattern
  $name[2] $name[-1]   one value, counted from 1, or from the end
  count $name          how many values
  export name value    give it to the programs you run, as name=value
  NAME=value command   for that one command only

PATTERNS
  *  any characters but /      ?  one character      **  any depth of directories
  A pattern that matches nothing is an error, except after for ... in and set.
  A name starting with - is given as ./-name, so it is never an option.

COMMAND OUTPUT
  $(command)      one value per line of its output
  "$(command)"    the whole output as one value

BLOCKS
  if COMMAND { ... } else if COMMAND { ... } else { ... }
  for name in WORDS { ... }        while COMMAND { ... }        break  continue
  fn name param... { ... }         return [N]
  try { ... } catch { ... }

FAILURES
  A command that fails stops the rest of its line, block, function or script, unless
  something checks it: if or while, !, && and ||, or try. $status says how the last
  command ended: its exit code, or a word such as interrupted or fault.

REDIRECTION
  > FILE  replace       >> FILE  add to       2> FILE  errors       &> FILE  both

SEE ALSO
  help errors, help tour
