# Plan part F2 (ADR-0412, ADR-0430): CPython's test_socket cases over a Unix
# pair: BasicSocketPairTest (socketpair, send, recv, sendall). Left out:
# PurePythonSocketPairTest, which tests socket.py's loopback fallback (an AF_INET
# listener: bind, listen, accept, plan part F7), and the rest of test_socket,
# which needs server or datagram sockets. The class is loaded by name, because
# regrtest's -m filter did not narrow the module in this port.
import sys
import unittest
from test import test_socket

suite = unittest.TestSuite()
for cls in (test_socket.BasicSocketPairTest,):
    suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(cls))
result = unittest.TextTestRunner(verbosity=1).run(suite)
print("unixpair:", "ok" if result.wasSuccessful() and result.testsRun > 0 else "FAILED", result.testsRun)
sys.exit(0 if result.wasSuccessful() else 1)
