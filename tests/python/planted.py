# TASK-27 9c, ADR-0320 item 6: a module planted in the current directory, as a
# download would leave it. `python3 -c` and `python3 -m` must not import it;
# a script of this directory (usesplanted.py) imports from its own directory.
print("PLANTED module imported")
