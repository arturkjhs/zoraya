name     git-ls-remote
summary  list the references of a repository over HTTPS
usage    git ls-remote [OPTION...] URL [PATTERN...]
program  /bin/git
sub      ls-remote
cwd      read
operand  URL  url  the address, https://HOST[:PORT]/PATH; this command may reach that host and port only
operand  [PATTERN...]  text  only the references that match
option   -q --quiet  say nothing
option   --heads  only branches
option   --tags  only tags
option   --refs  leave out the peeled tags
data     git as ~/.config/git
exit     0  listed
exit     128  git failed: the host was not given, the certificate was refused
---
git ls-remote, from the ports. The address on the line is the one host and port the command may reach, with
/system/tls to read, as for git clone.

EXAMPLES
  git ls-remote https://example.org/ada/proj.git

SEE ALSO
  git-clone, git
