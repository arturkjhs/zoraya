# TASK-27 9c, ADR-0320: what the interpreter in Zoraya does before any
# test suite: the policy of the environment, the built-in modules and their
# first use. Prints one "ok NAME" line per check; the shell script reads them.
import sys

print("ok flags", sys.flags.ignore_environment, sys.dont_write_bytecode, sys.flags.no_user_site)

import os, math, json, re, hashlib, zlib, datetime, struct, threading, subprocess

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

p = subprocess.run(["/bin/echo", "child"], capture_output=True)
assert p.stdout == b"child\n", p
print("ok subprocess")
print("smoke done")
