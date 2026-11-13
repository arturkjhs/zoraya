# TASK-27 9c, ADR-0320: the driver of the standard library's own tests in
# Zoraya. It runs CPython's regrtest (test.libregrtest) over the modules
# listed in regr-modules.txt, one process, no worker processes (regrtest's default without -j; the interpreter is the
# unit under test, not the job control). The list is a choice, not the suite:
# the modules that use what this port adds or changes (the interpreter's core
# types and the `re`, `json`, `zlib`, `hashlib`, `sqlite3`, `threading`,
# `asyncio` modules). The rest of the suite needs what Zoraya does not offer
# (AF_UNIX, a server socket, ctypes, shared extension modules, a terminal) or
# is slow for nothing it would show; ADR-0320 records the reason.
#
# regr-ignore.txt lists the test cases left out of the listed modules, and why:
#   - they start another interpreter (assert_python_ok and the like): a program
#     started by Python has no connection to the random service, so it fails at
#     start with "failed to get random numbers" (ADR-0310; test_deopt_from_append_list,
#     test_lazy_import, test_tool, test_ctx_mgr_rollback_if_commit_failed,
#     test_autocommit_disabled_implicit_shutdown), or they set PYTHONDEBUG-like
#     environment variables, which this build ignores (test_env_var_debug);
#   - they need a locale the system does not have (en_US.ISO8859-1:
#     test_locale_caching, test_locale_compiled);
#   - they ask for a very large thread stack, which does not fit the machine
#     (test_highly_nested_objects_decoding: MemoryError).
# test_threading is NOT in the CI subset (regr-modules.txt): on HVF it took 15.5 of the 16 minutes of the run and ended 'env changed' from an OSError in do_fork_and_wait (fork out of memory), so under TCG it would pass the 30 minute job limit; run it by hand. Its notes below still hold: with the cases that start another interpreter or
# name a thread over 15 bytes left out (regr-ignore.txt). It once failed with
# "OSError: Out of memory"; that was os.fork (no copy-on-write: a child copies the
# whole writable space, and the interpreter's stack was 8 MiB then, ADR-0320
# item 5), not thread creation.
import sys

# One module per line, in the list named by the first argument if it ends in
# .txt (regr-long.txt), else regr-modules.txt, next to this file (the Makefile
# copies both lists' files to the volume).
LIST = "regr-modules.txt"
if len(sys.argv) > 1 and sys.argv[1].endswith(".txt"):
    LIST = sys.argv.pop(1)
with open(LIST) as f:
    MODULES = [line.strip() for line in f if line.strip()]

from test.libregrtest.main import main

# Options of regrtest come from the command line (sys.argv[1:]). Its work
# directory is the program's own /tmp (regrtest otherwise builds one from the
# build directory it was configured in, which does not exist here), and its
# subdirectory (os.makedirs asks for `/` itself first, and the program is not
# given a stat of `/`, so the directory is one level below /tmp).
if not any(a.startswith("--tempdir") for a in sys.argv[1:]):
    sys.argv[1:1] = ["--tempdir", "/tmp/regr"]
sys.argv[1:1] = ["--ignorefile", "regr-ignore.txt"]
main(MODULES)
