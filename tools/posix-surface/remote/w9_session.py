"""W9 (TASK-02b): interrupt a parallel build the way a human does.

Usage: w9_session.py <case> <delay> <bindir> <mode> <rawdir> <id> <builddir> <jobs>
  case  int  : write VINTR (Ctrl-C) to the pty -> the tty driver sends SIGINT to the foreground process group
        term : send SIGTERM to the top-level make only (found as the shell's child named `make`)
  delay: seconds after the command line is sent
dash runs interactive (-i) as session leader with the pty as its controlling terminal, so job control is on and
make runs as a foreground job in its own process group. In strace mode strace runs as a detached grandchild in its
own session (-DDD): the shell stays the direct child of the harness and the controlling process, and Ctrl-C does
not reach strace. The harness is not measured. Harness actions are logged with epoch timestamps (`HARNESS t=`)
on the same clock as strace -ttt, for the timelines.
"""
import os, pty, sys, time, select, signal

case, delay, bindir, mode, rawdir, wid, builddir, jobs = sys.argv[1:9]
delay = float(delay)
argv = [os.path.join(bindir, "dash"), "-i"]
trace = os.path.join(rawdir, "strace", f"{wid}.txt")
if mode == "strace":
    argv = ["/usr/bin/strace", "-DDD", "-f", "-q", "-ttt", "-v", "-s", "256", "-o", trace] + argv

PROMPT = "PSURF> "
env = dict(os.environ)
env["PS1"] = PROMPT
env["ENV"] = "/dev/null"
if "PSURF_PRELOAD" in env:
    env["LD_PRELOAD"] = env.pop("PSURF_PRELOAD")

def hlog(msg):
    print(f"HARNESS t={time.time():.6f} {msg}", flush=True)

pid, fd = pty.fork()
if pid == 0:
    os.execve(argv[0], argv, env)

transcript = []
def read_until(marker, timeout=120.0):
    buf, end = "", time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                data = os.read(fd, 4096).decode(errors="replace")
            except OSError:
                break
            buf += data
            if marker in buf:
                break
    transcript.append(buf)
    return marker in buf

def children(ppid):
    out = []
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                st = open(f"/proc/{p}/stat").read()
            except OSError:
                continue
            fields = st[st.rindex(")") + 2:].split()
            if int(fields[1]) == ppid:
                out.append((int(p), st[st.index("(") + 1:st.rindex(")")]))
    return out

def procs_in(d, skip):
    """processes whose cwd is inside the build tree (left behind after make returned), except harness/shell/strace"""
    out = []
    for p in os.listdir("/proc"):
        if p.isdigit() and int(p) not in skip:
            try:
                comm = open(f"/proc/{p}/comm").read().strip()
                if comm != "strace" and os.readlink(f"/proc/{p}/cwd").startswith(d):
                    out.append(f"{p}:{comm}")
            except OSError:
                pass
    return out

ok = read_until(PROMPT)
os.write(fd, f"cd {builddir}\n".encode()); ok = read_until(PROMPT) and ok
hlog(f"send command make -j{jobs}")
os.write(fd, f"make -j{jobs}\n".encode())
time.sleep(delay)
if case == "int":
    hlog("write VINTR (Ctrl-C) to pty")
    os.write(fd, b"\x03")
else:
    mk = [p for p, comm in children(pid) if comm == "make"]
    if mk:
        hlog(f"kill SIGTERM pid={mk[0]} (top-level make)")
        os.kill(mk[0], signal.SIGTERM)
    else:
        hlog("no top-level make found (build already finished?)")
# the pty echo of the command line contains no prompt, so the next prompt means the foreground job is over
# (after Ctrl-C an interactive shell abandons the rest of the line, so the status is asked for separately)
ok = read_until(PROMPT) and ok
hlog("prompt back (make returned)")
os.write(fd, b"echo MAKE_STATUS=$?\n"); ok = read_until(PROMPT) and ok
done, partial = [], []
for leaf in ("a/1", "a/2", "b/1", "b/2"):
    for i in range(1, 9):
        f = os.path.join(builddir, leaf, f"t{i}.h")
        if os.path.exists(f):
            (done if open(f).read().rstrip().endswith("0 };") else partial).append(f"{leaf}/t{i}.h")
hlog(f"tables complete: {len(done)}/32; partial targets left: {' '.join(partial) or 'none'}")
time.sleep(0.5)
left = procs_in(builddir, {os.getpid(), pid})
hlog(f"processes left in build tree after 0.5s: {' '.join(left) or 'none'}")
os.write(fd, b"exit\n")
try:
    _, status = os.waitpid(pid, 0)
except ChildProcessError:
    status = 0
if mode == "strace":   # the detached strace finishes writing after the shell is gone
    end = time.time() + 30
    def tracing():
        for p in os.listdir("/proc"):
            try:
                if p.isdigit() and trace in open(f"/proc/{p}/cmdline").read().replace("\0", " "):
                    return True
            except OSError:
                pass
        return False
    while time.time() < end and tracing():
        time.sleep(0.1)
sys.stdout.write("".join(transcript).replace("\r", ""))
print(f"\n[w9] case={case} delay={delay} all_prompts_seen={ok} exit_status={os.waitstatus_to_exitcode(status)}")
