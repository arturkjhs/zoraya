name     python3
summary  run a Python program (CPython 3.15, a POSIX program)
usage    python3 [-bBIOPqsuV] [-W ARG] [-c CODE | -m MODULE | SCRIPT] [ARG...]
program  /bin/python3
operand  [SCRIPT]  read  the program to run (a module next to it is imported only if the line also gives its directory, +r DIR)
operand  [ARG...]  text  the program's arguments (sys.argv); a file a program opens by one of them needs a +r or +w of the line
option   -c --command CODE text  run CODE, a program given on the line
option   -m --module MODULE text  run a module of the standard library or of a directory on sys.path, as a script
when     --command SCRIPT text
when     --module SCRIPT text
option   -b --bytes-warning  warn when bytes and str are compared
option   -B --dont-write-bytecode  do not write .pyc files (the default here; the option only says it again)
option   -I --isolated  leave the script's directory out of sys.path
option   -O --optimize  remove assert statements and code that depends on __debug__
option   -P --safe-path  do not put the script's directory or the current directory on sys.path
option   -q --quiet  do not print the version at the start of an interactive session
option   -s --no-user-site  the per-user site directory is not used (never is, here)
option   -u --unbuffered  write standard output and standard error without a buffer
option   -V --version  print the version and exit
option   -W --warn ARG text  warning control, for example -W error or -W ignore::DeprecationWarning
exit     0  the program ended without an error
exit     1  an exception ended the program
exit     2  a usage mistake or a file that could not be opened
---
CPython 3.15 from the ports (TASK-27 9c, ADR-0320), as a POSIX program with the toolchain set: /bin, /usr (the standard
library, /usr/lib/python3.15) and /etc to read, /dev/null and a /tmp of its own, the current directory to read, and what
its operands and the line's +r, +w and +net words designate. It is a program like any other here: it can open the
files and reach the hosts and ports the line gives it, and nothing else. `+net example.org:443 python3 fetch.py` lets the
script open a connection to that host and port; the system's roots (/system/tls) come with the network, and the ssl
module trusts those roots and nothing a PYTHON* or SSL_* variable says.

The environment decides nothing: PYTHONPATH, PYTHONHOME, PYTHONSTARTUP, PYTHONSAFEPATH, PYTHONWARNINGS and the rest are
not read (the program runs as if with -E), so the files and directories it imports from are the ones the line names.
Neither is a per-user site directory used, and no .pyc file is written (the standard library comes with its own). Of
Python's options, -E, -X, -i, -d, -v, -x and the interactive session are not offered: there is no terminal from zosh
(ADR-0153), so a program is a script, -c or -m, or standard input read to its end.

Built in, as one program: _socket, select (select only: no poll or epoll), _ssl and _hashlib (OpenSSL 3.5, threads),
_sqlite3 (SQLite 3.54, no run-time extensions, no WAL), zlib, _asyncio, _thread and threading (ADR-0302). Not here: ctypes,
shared extension modules (there is no dlopen), readline, curses, tkinter, bz2, lzma, syslog, os.posix_spawn,
socketpair, AF_UNIX, and bind, listen and accept (a Python program is a client). os.fork works. A program that Python starts (subprocess, os.system, exec) has no connection to the random service, so another
python3 cannot start from it and a C program with TLS cannot seed (ADR-0310: the same as for a program started by dash).

EXAMPLES
  python3 -c 'print(sum(range(10)))'
  python3 -c 'import sqlite3, ssl, asyncio; print(sqlite3.sqlite_version, ssl.OPENSSL_VERSION)'
  python3 report.py data.csv
  +net example.org:443 python3 fetch.py

SEE ALSO
  grants, curl, wget
