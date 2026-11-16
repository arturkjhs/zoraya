# Plan part F5 (ADR-0415 item 1, ADR-0437): the mappings of a process, from
# CPython, in a 2 GiB guest (make test-python-mem), and ADR-0320's mapping
# measurement repeated: threads and forks leave the count of free mappings
# flat; a deep recursion no longer ends at about 33,000 frames; and the plan's
# "done when", the five million bytearrays, runs to its end.
import mmap
import os
import sys
import threading


def free():
    """How many one-page private mappings the process can still make."""
    held = []
    try:
        while True:
            held.append(mmap.mmap(-1, 4096, flags=mmap.MAP_PRIVATE))
    except (OSError, ValueError, MemoryError):
        pass
    n = len(held)
    for m in held:
        m.close()
    return n


start = free()
print("free mappings at the start", start, flush=True)

# 2000 threads started and joined, 2000 detached ones (daemon) that end alone.
for _ in range(2000):
    t = threading.Thread(target=lambda: None)
    t.start()
    t.join()
done = threading.Event()
count = [0]
lock = threading.Lock()


def daemon():
    with lock:
        count[0] += 1
        if count[0] == 2000:
            done.set()


for _ in range(2000):
    threading.Thread(target=daemon, daemon=True).start()
assert done.wait(120), count
print("4000 threads", flush=True)

# 30 forks and their waits.
for _ in range(30):
    pid = os.fork()
    if pid == 0:
        os._exit(0)
    assert os.waitpid(pid, 0)[1] == 0
print("30 forks", flush=True)

after = free()
print("free mappings after", after, flush=True)
assert abs(after - start) <= 2, (start, after)

# It ended with MemoryError at about 33,000 frames when the table had 256
# slots: every frame chunk was a mapping of 1 MiB.
sys.setrecursionlimit(10**7)


def down(n):
    return 0 if n == 0 else 1 + down(n - 1)


assert down(200_000) == 200_000
print("recursion to 200000 frames", flush=True)

# The plan's "done when".
x = [bytearray(100) for _ in range(5_000_000)]
print("bytearrays", len(x), flush=True)
del x
print("ok mappings")
