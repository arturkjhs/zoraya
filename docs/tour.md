# A tour of zosh

This is how a person new to Zoraya meets its shell, written as they would read it. Every example shows exactly what
they type (after the prompt) and what the screen shows. It is the tour of TASK-18 Part 1
(`docs/research/shell/tour.md`, kept as it was decided) brought to the shell as built; the differences are listed at
the end. `make test-posix` plays every example of this file on the serial console (`tests/posix/tour.script`) and
`make test-tour-gui` in the GUI terminal (`tests/posix/tour-gui.script`); both scripts are generated from this file
by `zoraya-tour`, and a host test fails when either differs from what this file gives (INV-0154-4), so the tour and
the shell cannot drift apart.

The tour runs on the POSIX image (stage 6e's `make posix-image`) without its session file, so `init` starts `zosh`
with the broker beside it, and starts on an empty `/home`. Messages written by TinyCC are TinyCC's own.

## Getting your bearings

The shell starts in your home directory, `/home`, which the prompt shows as `~`.

**1. The first screen.**
```
Type help to begin, or help tour.
~>
```

**2. What is there to do?** `help` lists the topics and every command with one line about it, a screen at a time:
the space bar shows the next screen, `q` stops.
```
~> help
Topics: language grants jobs keys settings errors tour
Commands:
  bg        continue a stopped job in the background
  cat       print files
  cd        change the current directory
  count     count arguments, or the lines of files
  cp        copy a file
  echo      print its arguments
  exit      end this shell; a new one starts
  export    give a variable to the programs you run
  fg        continue a job in the foreground
  find      find files by name
  grants    show what a command would be given, without running it
  head      print the first lines of files
  help      show help
  history   list the lines you have run
  jobs      list jobs
  kill      end a job
  ls        list a directory
  math      calculate
  mkdir     make a directory
  mv        move or rename
  poweroff  turn the machine off
-- space: next page, q: quit --
  pwd       print the current directory
  reboot    start the machine again
  rm        remove files
  room      list, enter, make or remove rooms
  search    find lines that contain text
  set       set a variable
  settings  show or change settings
  sleep     wait for some seconds
  slots     list the shell's handles
  source    run a script in this shell
  stat      describe a file
  status    describe how the last command ended
  sync      make written data durable
  test      check a condition
  unset     remove a variable
  uptime    time since boot
  wait      wait for background jobs
Demonstrations: echo-args exit-with fault hello spin stubborn
```

**3. Where am I?**
```
~> pwd
/home
~> ls
~>
```
An empty directory lists nothing. Commands that succeed say nothing more than their result.

## Files

**4. Write, add, read.** `>` writes a file, `>>` adds to it.
```
~> echo 'buy milk' > notes.txt
~> echo 'call Ada' >> notes.txt
~> cat notes.txt
buy milk
call Ada
```

**5. Directories.** Directories end in `/` when listed.
```
~> mkdir project
~> ls
notes.txt
project/
~> cd project
~/project> cd
~> cd project
~/project>
```

**6. Names with spaces.** A variable holds its value whole: it is never split at spaces.
```
~/project> set name 'meeting notes.txt'
~/project> echo agenda > $name
~/project> cp $name minutes.txt
~/project> ls
meeting notes.txt
minutes.txt
~/project> cat $name
agenda
```

**7. Patterns.** `*` matches names. A pattern that matches nothing is an error, so nothing runs by accident.
```
~/project> echo *.txt
meeting notes.txt minutes.txt
~/project> rm *.tmp
zosh: *.tmp: no file matches
```

**8. A habit from bash.** The shell says what to type instead.
```
~/project> x=1
zosh: x=1: not a command
  hint: to set a variable, type: set x 1
```

## Finding things

**9. By name.** `find` looks in the current directory and everything below it.
```
~/project> cd
~> find '*.txt'
notes.txt
project/meeting notes.txt
project/minutes.txt
```
The quotes keep the shell from matching `*.txt` itself.

**10. By content.**
```
~> search milk *.txt
notes.txt:1: buy milk
~> search cheese *.txt
~> echo $status
1
```
Finding nothing is a "no": status 1, nothing printed, and unlike a failure it does not stop the rest of the line.

**10a. Names from elsewhere.** The shell knows the names you may bring from other systems and says what to type here.
```
~> grep milk notes.txt
zosh: grep: no such command
  hint: on Zoraya this is search: search TEXT FILE...
~> man cp
zosh: man: no such command
  hint: on Zoraya this is help: help NAME
~> count --lines notes.txt
2
```
A name that is a program on the volume runs instead: this image has `wc`, a POSIX program (stage 6b's toybox),
so `wc -l notes.txt` counts as it does elsewhere.

## Getting help

**11. Any command explains itself.**
```
~> cp --help
cp: copy a file
usage: cp [--force] FROM TO
  -f, --force   replace TO if it exists
  --help        show this help
more: help cp
```

**12. Searching the help.**
```
~> help --search copy
cp        copy a file
```

**13. Tab.** Tab completes commands, file names and options; options come with their meaning.
```
~> cp --<Tab>
--force    replace TO if it exists
--help     show this help
~> cp --
```
While you type, the shell also shows in faint text the last matching line from your history; the right arrow accepts
it. A command that does not exist turns red before you press Enter.

## What a program is given

**14. Naming a file is giving it.** A program gets its input and output, the current directory to read, and the files
you name on its line, with the access its description asks for. `grants` shows this without running anything.
```
~> grants cp notes.txt project/notes.txt
cp gets:
  in, out, err         the terminal
  ~                    read (current directory)
  ~/notes.txt          read (operand FROM)
  ~/project            create or replace notes.txt (operand TO)
~> cp notes.txt project/notes.txt
```

**15. Compiling a program.** TinyCC is part of the base system and its description comes with it, so the source it
reads and the output it names are given to it: `-o hello` asks for "create or replace this one name", and compiling
again simply replaces the program. Here both names are in the current directory, which tcc is given whole; step 17's
`grants` shows that, and why.
```
~> echo '#include <stdio.h>' > hello.c
~> echo 'int main(void) { printf("hello, world\n"); return 0; }' >> hello.c
~> tcc hello.c -o hello
~> ./hello
hello, world
~> echo '#include <stdio.h>' > hello.c
~> echo 'int main(void) { printf("hello again\n"); return 0; }' >> hello.c
~> tcc hello.c -o hello
~> ./hello
hello again
```

**16. Giving a directory.** A file can name other files, which no description can know about. Here `main.c` includes
a header from `~/include`: TinyCC cannot see it until you give that directory with `+r` (read) or `+w` (read and
write) before the command. When a program reaches for something it was not given, the shell says what and how to
give it.
```
~> mkdir include
~> echo '#define GREETING "hi there"' > include/greet.h
~> cd project
~/project> echo '#include "../include/greet.h"' > main.c
~/project> echo '#include <stdio.h>' >> main.c
~/project> echo 'int main(void) { puts(GREETING); return 0; }' >> main.c
~/project> tcc main.c -o main
main.c:1: error: include file '../include/greet.h' not found
  hint: tcc tried to read ~/include/greet.h, which it was not given
  to allow it: +r ~/include tcc main.c -o main
~/project> +r ~/include tcc main.c -o main
~/project> ./main
hi there
```

**17. Seeing it.** `grants` shows every grant, including the read-only system directories every compiler-style
program gets.
```
~/project> grants +r ~/include tcc main.c -o main
tcc gets (a POSIX program):
  in, out, err         the terminal
  ~/project            read (current directory)
  ~/project/main.c     read (operand FILE)
  ~/project            create and change anything in (option -o)
  ~/include            read (+r)
  /bin /usr /etc       read (system)
  /proc/self/exe       read (system)
  /tmp                 change anything in (this line's own)
  /dev/null
```

**18. A grant never comes from data.** Only a `+r` or `+w` you type before a command is a grant.
```
~/project> set g '+w /'
~/project> $g rm notes.txt
zosh: +w /: no such command
```

## Pipelines

**19. Connecting programs.** `|` sends one program's output to the next.
```
~/project> cat ~/notes.txt | search Ada
call Ada
~/project> ls | head --lines 1
main
```

**20. Program output as a list.** `$(...)` gives one element per line.
```
~/project> set lines $(cat ~/notes.txt)
~/project> count $lines
2
~/project> echo $lines[2]
call Ada
```

**21. History is a pipeline's start too.**
```
~/project> history | search 'tcc main'
tcc main.c -o main
+r ~/include tcc main.c -o main
grants +r ~/include tcc main.c -o main
```

## Small scripts

**22. A loop.**
```
~/project> cd
~> for f in *.c *.txt { echo "$f: $(count --lines $f) lines" }
hello.c: 2 lines
notes.txt: 2 lines
```

**23. Your own command.** In a script, comment lines just above `fn` become its help.
```
~> fn backup file {
...>     cp $file $file.bak
...> }
~> backup notes.txt
~> ls *.bak
notes.txt.bak
~> backup
backup: missing operand FILE
usage: backup FILE
more: help backup
```
(`...>` is the prompt for a line that continues.)

**24. A script file.** `source` runs a file of commands in this shell.
```
~> echo 'for f in *.bak {' > tidy.zosh
~> echo '    rm $f' >> tidy.zosh
~> echo '}' >> tidy.zosh
~> source tidy.zosh
~> ls *.bak
zosh: *.bak: no file matches
```

## When things go wrong

**25. A failed command says why, and stops the rest of the line.**
```
~> cd nowhere
cd: nowhere: not found
~> mkdir project; echo 'not printed'
mkdir: project: already exists
(the rest of the line was not run)
~> echo $status
1
```

**26. Handling a failure yourself.** `||` runs only when the left side failed; `try` catches the first failure of a
block.
```
~> cat missing.txt || echo 'no file, carrying on'
cat: missing.txt: not found
no file, carrying on
~> try { cp notes.txt notes.old; cp notes.txt notes.old } catch { echo "stopped with status $status" }
cp: notes.old: already exists
  hint: cp --force notes.txt notes.old replaces it
stopped with status 1
```

**27. A script stops at its first unchecked failure, and says where.**
```
~> echo 'tcc broken.c -o broken' > build.zosh
~> echo 'echo built' >> build.zosh
~> source build.zosh
tcc: error: file 'broken.c' not found
zosh: build.zosh line 1: tcc: exit status 1
(the rest of build.zosh was not run)
```

## Jobs

**28. Stopping and continuing.** Ctrl-C interrupts the program in front; Ctrl-Z stops it and `fg` continues it.
```
~> spin
^C
spin: interrupted (Ctrl-C)
~> sleep 5
^Z
[1] stopped: sleep 5
~> fg
sleep 5
~>
```

**29. In the background.** `&` runs a command while you go on typing.
```
~> sleep 30 &
[1] sleep 30
~> jobs
[1]  running   sleep 30
~> kill %1
[1] sleep 30: terminated
```

**30. Leaving with work still running.**
```
~> sleep 60 &
[1] sleep 60
~> exit
1 job is still running; exit again to end it
~> kill %1
[1] sleep 60: terminated
```

## Settings

**31. Looking and changing.** `settings` with nothing else lists what you have changed.
```
~> settings
~> settings shell.suggestions
shell.suggestions = yes   (default)
  yes-no: suggestions from history
~> settings shell.suggestions no
~> settings
shell.suggestions = no
~> settings shell.sugestions yes
settings: shell.sugestions: no such setting
  hint: did you mean shell.suggestions?
~> settings --reset shell.suggestions
```

**32. Ending.** `exit` ends this shell; a new one starts in its place. `poweroff` writes everything to disk and turns
the machine off; `reboot` starts it again.
```
~> exit
Type help to begin, or help tour.
~> poweroff
```

## Differences from the tour of Part 1

Each is the shell as built, found by playing Part 1's tour (`docs/research/shell/tour.md`) in TASK-18 step 11.

- **2.** `help` shows its list a screen at a time, as every help page does (ADR-0156 item 5): a window of 24 rows
  has no scrollback, so the top of a longer list could not be read. The test programs (`probe`, `requests`) are
  described but listed by no `help`.
- **10a.** The image has `wc`, a POSIX program of the ports, so it runs; the hint is shown for `man`, which names
  nothing here.
- **17.** `-o main` names a file in the current directory, whose table entry is the current directory itself: a
  record holds one entry per directory, so rule 2's fallback gives the directory whole (`create and change anything
  in`), as Part 1's tour said would happen until the narrowed connection; `grants` says so. In example 14 the name is
  in another directory and stays narrowed.
- The tour runs on the POSIX image without its session file (TinyCC, the POSIX programs, the broker beside `zosh`),
  not on today's image plus stage 6 in parts, as Part 1's introduction planned.
