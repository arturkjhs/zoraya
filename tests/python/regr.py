# TASK-27 9c, ADR-0320: the driver of the standard library's own tests in
# Zoraya. It runs CPython's regrtest (test.libregrtest) over the modules
# listed in MODULES, one process, no worker processes (the interpreter is the
# unit under test, not the job control). The list is a choice, not the suite:
# the modules that use what this port adds or changes (the interpreter's core
# types and the `re`, `json`, `zlib`, `hashlib`, `sqlite3`, `threading`,
# `asyncio` modules). The rest of the suite needs what Zoraya does not offer
# (AF_UNIX, a server socket, ctypes, shared extension modules, a terminal) or
# is slow for nothing it would show; ADR-0320 records the reason.
import sys

MODULES = [
    "test_bisect", "test_heapq", "test_int", "test_dict", "test_list",
    "test_string", "test_re", "test_json", "test_zlib", "test_hashlib",
    "test_sqlite3", "test_threading", "test_asyncio.test_tasks",
    "test_asyncio.test_futures", "test_asyncio.test_locks",
]

from test.libregrtest.main import main

main(MODULES + sys.argv[1:], verbose=False, verbose3=False,
     use_resources=[], timeout=300)
