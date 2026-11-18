name     git
summary  version control: the repository of the current directory (git, from the ports)
usage    git [--version] [--no-pager] [-C DIR] COMMAND [ARGUMENT...]
program  /bin/git
cwd      read
repository
operand  COMMAND  text  the git command: status, add, commit, log, diff, branch, ...
operand  [ARGUMENT...]  rest  the command's own words, taken as they are: none is an option of the shell
option   --version  print git's version
option   --no-pager  do not start the pager (less) for this command
option   -C --directory DIR read  start in DIR instead of the current directory (git has no long form: write -C)
data     git as ~/.config/git
exit     0  the command succeeded
exit     1  the command failed or found differences (as git says)
exit     128  git itself failed: no repository, a refused file, no network
---
git 2.56.1 from the ports (ADR-0339, ADR-0347), as a POSIX program with the toolchain set and a terminal. It is given what
the line says and, as a base-system command, the repository of the current directory (or of -C DIR): the nearest
directory that has a .git directory, inside what this shell holds, to change anything in (to read, if that is all this
shell holds there). Nothing above it is given, a .git that is a file is refused, and a repository that is your whole
home directory is refused: for those write the grant yourself (+w ~ git status). git's own files are one directory,
/data/home/git (in a room /data/ROOM/git), which it sees as ~/.config/git: its global configuration, nothing secret.
The image's /etc/gitconfig sets no detached garbage collection and no guessed author: set yours once with
`git config --global user.name 'Ada'` and `git config --global user.email ada@example.org`.

It starts with no network. `git clone` and `git ls-remote` take their URL from the line (see `help git-clone`); `fetch`,
`pull` and `push` take their address from the repository's configuration, which is data and gives nothing: write the
host on the line, `+net example.org:443 git push`, or keep it in a project room. A password or token is asked on the
terminal each time and never stored: no credential helper is built, and `credential.helper`, `core.askPass`,
`SSH_ASKPASS`, `GIT_ASKPASS` and `GIT_SSH_COMMAND` do nothing. `git -c key=value` and GIT_CONFIG_* set nothing either: a line
carries no configuration (ADR-0347). The certificate must come from /system/tls/roots; `http.sslVerify=false` and the
variables that name another root are ignored. Only https:// is a transport: no ssh, git:// or file URL clones.
Hooks, the editor (vi) and the pager (less) are children of git with its table: they can change the repository, can read
git's configuration (not change it), and have no network connection.

EXAMPLES
  git init proj
  cd proj
  git add notes.txt
  git commit -m 'first notes'
  git log
  git -C ~/proj status
  git clone https://example.org/ada/proj.git
  +net example.org:443 git push

FILES
  /data/home/git/config  the global configuration (seen as ~/.config/git/config)
  /etc/gitconfig  the system configuration, read only

SEE ALSO
  git-clone, git-init, git-ls-remote, grants, vi, less
