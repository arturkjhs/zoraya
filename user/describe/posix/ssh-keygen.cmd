name     ssh-keygen
summary  make a key pair for ssh, or forget a host's key (OpenSSH's ssh-keygen)
usage    ssh-keygen [-q] [-t TYPE] [-b BITS] [-C COMMENT] [-R HOST]
program  /bin/ssh-keygen
cwd      none
option   -t --type TYPE text  the kind of key: ed25519, ecdsa or rsa
option   -b --bits BITS text  the size of an rsa or ecdsa key
option   -C --comment COMMENT text  the comment written in the public key
option   -R --remove HOST text  remove the stored key of HOST from known_hosts
option   -q --quiet  print no progress or picture of the key
data     ssh as ~/.ssh
exit     0  done
exit     1  failed
---
OpenSSH's ssh-keygen, from the ports (ADR-0340), as a POSIX program with the toolchain set and a terminal. It is
given the random service and one directory of its own, /data/home/ssh (in a room, /data/ROOM/ssh), which it sees as
~/.ssh, the directory `ssh` is given too (ADR-0341). It has no network and no other file.

`ssh-keygen -t ed25519` writes id_ed25519 and id_ed25519.pub there, with the modes ssh checks, asks for a passphrase
twice (empty for none) on the terminal, and prints the fingerprint. The default file names are the only ones: -f is
not offered in stage 10. `-R HOST` removes HOST's key from known_hosts, which is what a person does who knows a host's
key has rightly changed; the next `ssh` to it asks as for a new host.

The program reads short options only: write -t, -b, -C, -R and -q, not their long names, which the
description keeps for `help`; the shell refuses a long form and says the short one.

EXAMPLES
  ssh-keygen -t ed25519
  ssh-keygen -R build.example.org

FILES
  /data/home/ssh  its own directory, seen as ~/.ssh

SEE ALSO
  ssh
