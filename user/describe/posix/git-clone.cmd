name     git-clone
summary  copy a repository over HTTPS into a new directory
usage    git clone [OPTION...] URL [DIR]
program  /bin/git
sub      clone
cwd      read
operand  URL  url  the address, https://HOST[:PORT]/PATH; this command may reach that host and port only
operand  [DIR]  create-tree  the new directory; without it, the last part of the address without .git
derive   DIR URL .git
option   -b --branch BRANCH text  check out BRANCH instead of the remote's default
option   --depth DEPTH text  only the last DEPTH commits
option   -o --origin NAME text  call the remote NAME instead of origin
option   -j --jobs N text  fetch N submodules at a time (submodules are not cloned)
option   -q --quiet  say nothing
option   -v --verbose  say more
option   -n --no-checkout  make the repository and leave the files out
option   --single-branch  only the one branch
option   --no-tags  no tags
option   --progress  show progress even when the output is not the terminal
data     git as ~/.config/git
exit     0  cloned
exit     128  git failed: the host was not given, the certificate was refused, the directory exists
---
git clone, from the ports. The address on the line is the one host and port the command may reach, with /system/tls to
read: a certificate that is expired, is for another name or is made by no root of /system/tls/roots is refused, and
no option or variable turns that off. The new directory DIR is created in the current directory, and everything in it
is git's to change; the rest of the current directory is read only. A redirect to another host is refused by the
network service like any host the line does not give. Only https:// addresses clone; ssh:// and HOST:PATH addresses
are refused by the description (git over ssh waits for a way to give a started ssh its own directory).

EXAMPLES
  git clone https://example.org/ada/proj.git
  git clone --depth 1 https://example.org/ada/proj.git copy

SEE ALSO
  git, grants
