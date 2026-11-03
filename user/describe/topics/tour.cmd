topic    tour
summary  a short tour of the shell
---
  pwd                          where you are; the prompt shows it, with ~ for home
  ls                           what is here
  echo 'buy milk' > notes.txt  write a file; >> adds to it
  cat notes.txt                read it
  mkdir project; cd project    make a directory and go in
  set name 'meeting notes.txt' a variable keeps spaces: $name is one argument
  echo *.txt                   patterns; one that matches nothing is an error
  count --lines notes.txt      count lines
  for f in *.txt { echo $f }   a loop
  fn hi who { echo "hello, $who" }   your own command
  mkdir project || echo there  handle a failure yourself
  try { ... } catch { ... }    or catch it
  sleep 30 &                   run in the background; jobs, kill %1
  help cp                      every command explains itself; Tab completes
  settings                     change the prompt, history and colours
  exit                         a new shell starts; poweroff turns the machine off

SEE ALSO
  help language, help keys, help errors
