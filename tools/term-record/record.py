#!/usr/bin/env python3
"""Records a terminal session on the host (ADR-0332 item 10).

    record.py SESSION OUTDIR

SESSION is a script, one command a line:

    size COLUMNS ROWS          the window's size (default 80 24)
    env NAME=VALUE             an environment variable (TERM is xterm-256color)
    cwd DIR                    where the program starts (default: a fresh empty directory)
    file NAME TEXT             a file made in that directory first (TEXT has \\n and \\xNN escapes)
    run PROGRAM ARGS...        the program, started on a pseudo-terminal
    send TEXT                  bytes to type (escapes: \\r \\n \\t \\e \\xNN \\\\)
    wait SECONDS               read until the output has been idle for 0.4 s, at most SECONDS
    mark LABEL                 a point at which the grid is checked, at the offset so far

In `run` and `env`, @curses@ is build/ports/host/curses-tests/bin (tools/term-record/build-curses-tests.sh)
and @terminfo@ the directory `tools/term-terminfo/terminfo.sh compile` fills, both
made absolute from the repository's root, which the script must be run from: a
session file names no path of the host.

It writes OUTDIR/NAME.rec (the program's output, byte for byte, which the
replay test feeds to zoraya-term) and OUTDIR/NAME.marks (offset and label of each
mark). NAME is the script's file name without its extension. Nothing the
terminal would answer is answered: the recording is of a terminal that says nothing.
Host tools only; the recorded program's output is data, committed with the recording.
"""
import fcntl
import os
import pty
import select
import shlex
import struct
import sys
import tempfile
import termios
import time


def unescape(text):
    out = bytearray()
    i = 0
    while i < len(text):
        c = text[i]
        if c == "\\" and i + 1 < len(text):
            n = text[i + 1]
            if n == "x" and i + 3 < len(text) + 0 and i + 4 <= len(text):
                out.append(int(text[i + 2 : i + 4], 16))
                i += 4
                continue
            out.extend({"r": b"\r", "n": b"\n", "t": b"\t", "e": b"\x1b", "\\": b"\\"}.get(n, n.encode()))
            i += 2
            continue
        out.extend(c.encode())
        i += 1
    return bytes(out)


def main():
    root = os.getcwd()
    tokens = {
        "@curses@": os.path.join(root, "build/ports/host/curses-tests/bin"),
        "@terminfo@": os.path.join(root, "build/ports/host/terminfo"),
    }

    def expand(text):
        for k, v in tokens.items():
            text = text.replace(k, v)
        return text

    session, outdir = sys.argv[1], sys.argv[2]
    name = os.path.splitext(os.path.basename(session))[0]
    columns, rows = 80, 24
    env = {"TERM": "xterm-256color", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8", "HOME": "/nonexistent", "PATH": os.environ.get("PATH", "/usr/bin:/bin")}
    cwd = tempfile.mkdtemp(prefix="zoraya-record-")
    files = []
    steps = []
    program = None
    for line in open(session):
        line = line.rstrip("\n")
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        cmd, _, rest = line.partition(" ")
        if cmd == "size":
            columns, rows = (int(v) for v in rest.split())
        elif cmd == "env":
            k, _, v = expand(rest).partition("=")
            env[k] = v
        elif cmd == "file":
            fname, _, text = rest.partition(" ")
            files.append((fname, unescape(text)))
        elif cmd == "run":
            program = shlex.split(expand(rest))
        elif cmd in ("send", "wait", "mark"):
            steps.append((cmd, rest))
        else:
            sys.exit(f"{session}: unknown command {cmd}")
    for fname, data in files:
        with open(os.path.join(cwd, fname), "wb") as f:
            f.write(data)
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.execvpe(program[0], program, env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    out = bytearray()
    marks = []

    def drain(limit):
        deadline = time.time() + limit
        idle_since = time.time()
        while time.time() < deadline:
            r, _, _ = select.select([fd], [], [], 0.05)
            if r:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    return False
                if not data:
                    return False
                out.extend(data)
                idle_since = time.time()
            elif time.time() - idle_since > 0.4:
                break
        return True

    alive = drain(3.0)
    for cmd, rest in steps:
        if cmd == "send" and alive:
            os.write(fd, unescape(rest))
            time.sleep(0.05)
        elif cmd == "wait" and alive:
            alive = drain(float(rest))
        elif cmd == "mark":
            marks.append((len(out), rest))
    try:
        os.close(fd)
    except OSError:
        pass
    try:
        os.kill(pid, 9)
    except OSError:
        pass
    try:
        os.waitpid(pid, 0)
    except OSError:
        pass
    os.makedirs(outdir, exist_ok=True)
    with open(os.path.join(outdir, name + ".rec"), "wb") as f:
        f.write(out)
    with open(os.path.join(outdir, name + ".marks"), "w") as f:
        f.write(f"size {columns} {rows}\n")
        for offset, label in marks:
            f.write(f"{offset} {label}\n")
    print(f"{name}: {len(out)} bytes, {len(marks)} marks")


main()
