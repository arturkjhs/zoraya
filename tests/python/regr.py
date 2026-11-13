# TASK-27 9c, ADR-0320: the driver of the standard library's own tests in
# Zoraya. It runs CPython's regrtest (test.libregrtest) over the modules
# listed in regr-modules.txt, one process, no worker processes (the interpreter is the
# unit under test, not the job control). The list is a choice, not the suite:
# the modules that use what this port adds or changes (the interpreter's core
# types and the `re`, `json`, `zlib`, `hashlib`, `sqlite3`, `threading`,
# `asyncio` modules). The rest of the suite needs what Zoraya does not offer
# (AF_UNIX, a server socket, ctypes, shared extension modules, a terminal) or
# is slow for nothing it would show; ADR-0320 records the reason.
import sys

# One module per line, in regr-modules.txt next to this file (the Makefile
# copies the same list's files to the volume).
with open("regr-modules.txt") as f:
    MODULES = [line.strip() for line in f if line.strip()]

from test.libregrtest.main import main

# Options of regrtest come from the command line (sys.argv[1:]).
main(MODULES)
