# TASK-27 9c, ADR-0320: what the interpreter in Zoraya does before any
# test suite: the policy of the environment, the built-in modules and their
# first use. Prints one "ok NAME" line per check; the shell script reads them.
import sys

print("ok flags", sys.flags.ignore_environment, sys.dont_write_bytecode, sys.flags.no_user_site)

import os, math, json, re, hashlib, zlib, datetime, struct, threading

assert math.isclose(math.sin(math.pi / 6), 0.5)
assert json.loads(json.dumps({"a": [1, 2.5, None]})) == {"a": [1, 2.5, None]}
assert re.sub(r"(\d+)", lambda m: str(int(m.group(1)) * 2), "a1b22") == "a2b44"
assert hashlib.sha256(b"abc").hexdigest().startswith("ba7816bf")
assert zlib.decompress(zlib.compress(b"zoraya" * 1000)) == b"zoraya" * 1000
print("ok basics")

import sqlite3
db = sqlite3.connect(":memory:")
db.execute("create table t (k integer primary key, v text)")
db.executemany("insert into t (v) values (?)", [("a",), ("b",), ("c",)])
assert db.execute("select group_concat(v, '') from t").fetchone()[0] == "abc"
print("ok sqlite", sqlite3.sqlite_version)

r = []
t = threading.Thread(target=lambda: r.append(os.getpid()))
t.start(); t.join()
assert r == [os.getpid()]
print("ok threads")

import ssl
ctx = ssl.create_default_context()
print("ok ssl", ssl.OPENSSL_VERSION.split()[1], ctx.verify_mode == ssl.CERT_REQUIRED, ctx.check_hostname)

import asyncio
async def work(n):
    await asyncio.sleep(0.01 * n)
    return n * n
async def main():
    return await asyncio.gather(*(work(n) for n in range(1, 5)))
assert asyncio.run(main()) == [1, 4, 9, 16]
print("ok asyncio")

# A child by fork (no exec: a program started by another has no connection to
# the random service, so a second python3 cannot start from here; ADR-0310).
pid = os.fork()
if pid == 0:
    os._exit(7)
_, status = os.waitpid(pid, 0)
assert os.WIFEXITED(status) and os.WEXITSTATUS(status) == 7, status
print("ok fork")

# The status of a child that ends through the interpreter (not os._exit): the
# thread the interpreter runs on ends the process with it (patch 0004), also in
# a fork child, where that thread is the only one left.
def child_status(body):
    pid = os.fork()
    if pid == 0:
        body()
        os._exit(99)
    _, st = os.waitpid(pid, 0)
    assert os.WIFEXITED(st), st
    return os.WEXITSTATUS(st)

assert child_status(lambda: sys.exit(5)) == 5
def boom():
    sys.stderr = open(os.devnull, "w")
    raise RuntimeError("expected: an uncaught exception in a child")
assert child_status(boom) == 1
print("ok fork status")

# urllib takes no proxy from the environment (the interpreter runs as with -E,
# patch 0006); the variable is set here, after start, as a program could.
import urllib.request
os.environ["https_proxy"] = os.environ["http_proxy"] = "http://127.0.0.1:9"
assert urllib.request.getproxies() == {}, urllib.request.getproxies()
print("ok no proxy")
print("smoke done")
