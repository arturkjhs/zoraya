# Plan part F7 (ADR-0439): a Unix socket file in the program's own /tmp, in one process: bind, listen, connect from a second
# socket (it completes before accept: the pair is queued), accept, bytes both ways, the names, and the AF_INET refusal.
# Every socket has a timeout, so that a hang is a TimeoutError of 10 s and not a step that runs out its limit.
import errno
import os
import socket

socket.setdefaulttimeout(10)
path = "/tmp/pyu"
s = socket.socket(socket.AF_UNIX)
s.bind(path)
s.listen(1)
c = socket.socket(socket.AF_UNIX)
c.connect(path)
a, _ = s.accept()
c.sendall(b"hi")
assert a.recv(2) == b"hi"
a.sendall(b"yo")
assert c.recv(2) == b"yo"
assert s.getsockname() == path, s.getsockname()
assert c.getpeername() == path, c.getpeername()
for x in (a, c, s):
    x.close()
os.unlink(path)
t = socket.socket()
try:
    t.bind(("127.0.0.1", 0))
    raise SystemExit("unixsock: AF_INET bind worked")
except OSError as e:
    assert e.errno == errno.EOPNOTSUPP, e
print("unixsock: ok")
