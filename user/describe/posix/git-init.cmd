name     git-init
summary  make an empty repository in a new directory
usage    git init [-q] [-b BRANCH] [DIR]
program  /bin/git
sub      init
cwd      read
operand  [DIR]  create-tree  the new directory; without it git would make .git in the current directory, which needs +w .
option   -q --quiet  say nothing
option   -b --initial-branch BRANCH text  name the first branch BRANCH
option   --bare  a repository with no work tree
data     git as ~/.config/git
exit     0  made
exit     128  git failed
---
git init, from the ports. `git init DIR` creates DIR in the current directory, and everything in it is git's to change.
`git init` alone makes .git in the current directory, which a line may only change with +w .: write `+w . git init`.

EXAMPLES
  git init proj
  +w . git init

SEE ALSO
  git
