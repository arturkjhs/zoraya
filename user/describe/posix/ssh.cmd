name     ssh
summary  log in to another machine, or run a command there (OpenSSH's client)
usage    ssh [-p PORT] [-l USER] [-tTv] HOST [COMMAND...]
program  /bin/ssh
cwd      none
operand  HOST  host  the machine, as HOST or USER@HOST; this command may reach that host on port 22 or the one -p gives, and nothing else
operand  [COMMAND...]  rest  the command for the far side, taken as it is: every word after HOST is the far side's, none is an option of ssh
reach    HOST tcp 22 -p
option   -p --port PORT text  the port, if not 22
option   -l --login USER text  the name to log in as; without it, write USER@HOST
option   -t --tty  ask for a terminal on the far side even when a command is given
option   -T --no-tty  ask for none
option   -v --verbose  say what the client is doing, on the error stream
data     ssh as ~/.ssh
exit     0  the far side's command, or the session, ended well
exit     255  ssh itself failed: no route, the host key refused, no way to log in
---
OpenSSH's ssh, from the ports (ADR-0340), on the OpenSSL of the image (ADR-0312), as a POSIX program with the
toolchain set and a terminal (ADR-0331). It is given what the line says and nothing more: a connection that reaches
HOST on port 22 (or the port -p gives) and nothing else, the random service, /system/tls, and one directory of its
own, /data/home/ssh (in a room, /data/ROOM/ssh), which it sees as ~/.ssh (ADR-0341). That is where `known_hosts` and
the keys `ssh-keygen` makes are; no other program is given them unless a line names them.

It reads no configuration file: neither /etc/ssh/ssh_config nor ~/.ssh/config, and -F and -o are not offered. What
would go in a Host block goes in a function: `fn build { ssh -l ada build.example.org $args }`. SSH_ASKPASS and
SSH_ASKPASS_REQUIRE are not honoured, so a line cannot make ssh run a program. Every word after HOST belongs to the
far side's command; an -o, -J, -F or -i written there is sent to the far side as a word of its command and changes
nothing here. The options that could change the destination or run a program (-J -W -o -F -L -R -D -A -E -S -w -I -i)
are not listed, and the shell refuses them before ssh runs.

A name is needed: -l USER or USER@HOST, as the image's one identity is not a name on another machine. The first
connection to a host shows the fingerprint of its key and asks; `yes` stores it in known_hosts. A key that differs
from the stored one is refused with ssh's warning and nothing on the line overrides that; `ssh-keygen -R HOST` removes
the old one. A passphrase or password is asked on the terminal each time: there is no agent. `~.` ends a stuck
session.

The program reads short options only: write -p, -l, -t and the rest, not their long names, which the
description keeps for `help`; the shell refuses a long form and says the short one.

EXAMPLES
  ssh -l ada build.example.org
  ssh -l ada build.example.org ls -l
  ssh -l ada 10.0.2.50 'uname -a'
  ssh -p 2222 -l ada 10.0.2.50

FILES
  /data/home/ssh/known_hosts  the hosts it has been told to trust (seen as ~/.ssh/known_hosts)
  /data/home/ssh/id_ed25519  the private key `ssh-keygen -t ed25519` made (seen as ~/.ssh/id_ed25519)

SEE ALSO
  ssh-keygen
