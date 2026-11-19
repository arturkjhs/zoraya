name     programs
summary  list installed programs and their saved grants
usage    programs
usage    programs NAME
usage    programs grant NAME WORD...
usage    programs revoke NAME WORD...
usage    programs revoke NAME --all
usage    programs revoke --kill NAME
usage    programs remove NAME [--data]
usage    programs --log [NAME]
operand  [WORDS...]  text  a program, or grant, revoke or remove with its words
option   --all   with revoke: every saved grant of the program
option   --kill  with revoke: also end the running copies of the program
option   --data  with remove: also delete its own files in every room
option   --log   print what was installed, granted, revoked and removed
cwd      none
exit     0  done
exit     1  no such program, a refused word, or nothing changed
---
With no word, one row for each installed program: its short hash, the date, how many grants
are saved and how many copies run now; changed marks a program whose files are not the ones
installed (it does not start). programs NAME shows the page: the description's summary, the
files, the hash, each saved grant in words, what the package asked for and was not given, and
the copies that run.

programs grant saves more words, as install does. programs revoke removes exactly the words
it names from the record at once; a copy that runs keeps what it holds until it ends. The
line says how many copies keep access: programs revoke --kill NAME ends them. programs remove
deletes the package and its record; its own files under /data are kept unless --data is given.

The record is /config/apps and the log is /config/apps.log, one line a change (never a start).
Only install and programs write them, in a shell that holds both and from words you typed.

EXAMPLES
  programs
  programs ed
  programs revoke ed +net sync.example.org:443
  programs revoke --kill ed
  programs --log

SEE ALSO
  help install
