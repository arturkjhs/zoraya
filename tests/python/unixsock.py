# Plan part F7 (ADR-0439): a Unix socket file in the program's own /tmp, in one process: bind, listen, connect from a second
# socket (it completes before accept: the pair is queued), accept, bytes both ways, the names.
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
# An AF_INET socket binds (the address is recorded) and its `listen` is refused by `netd`'s grant: the line has no
# +listen word (plan part F7b; the served case is tests/shell/python-serve.script's).
t = socket.socket()
t.bind(("127.0.0.1", 0))
try:
    t.listen(1)
    raise SystemExit("unixsock: AF_INET listen worked with no listen grant")
except PermissionError as e:
    assert e.errno == errno.EACCES, e
print("unixsock: ok")
