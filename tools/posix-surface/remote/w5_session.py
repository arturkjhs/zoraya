"""W5: drive an interactive shell on a pseudo-terminal.

Usage: w5_session.py <shell> <bindir> <mode> <rawdir>
The shell runs as a session leader with the pty as controlling terminal (pty.fork), interactive (-i), so
job control, prompt handling and SIGINT delivery from the terminal driver are real. In strace mode the shell
is started under strace -f. The harness (this Python process) is not measured: LD_PRELOAD is only in the
child's environment.
"""
import os, pty, sys, time, select, signal

shell, bindir, mode, rawdir = sys.argv[1:5]
# mrsh has no -i option; it is interactive when stdin is a terminal (which it is here)
argv = [os.path.join(bindir, shell)] + (["sh"] if shell == "busybox" else []) + ([] if shell == "mrsh" else ["-i"])
if mode == "strace":
    argv = ["/usr/bin/strace", "-f", "-qq", "-s", "256", "-o",
            os.path.join(rawdir, "strace", f"W5-{shell}.txt")] + argv

PROMPT = "PSURF> "
env = dict(os.environ)
env["PS1"] = PROMPT
env["ENV"] = "/dev/null"
if "PSURF_PRELOAD" in env:
    env["LD_PRELOAD"] = env.pop("PSURF_PRELOAD")

steps = [
    "cd \"$HOME\"",
    "echo hello world | tr a-z A-Z > up.txt",
    "cat < up.txt",
    "touch a.txt b.txt; ls *.txt",
    "FOO=bar; export FOO; sh -c 'echo child sees $FOO'",
    "sleep 2 &",
    "jobs",
    ("sleep 30", "INTR"),
    "echo status=$?",
    "cd /tmp; pwd; cd -",
    "wait; echo waited",
    "exit",
]

pid, fd = pty.fork()
if pid == 0:
    os.execve(argv[0], argv, env)

transcript = []
def read_until(marker, timeout=20.0):
    buf, end = "", time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.2)
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

ok = read_until(PROMPT)
for st in steps:
    cmd, action = (st if isinstance(st, tuple) else (st, None))
    os.write(fd, (cmd + "\n").encode())
    if action == "INTR":
        time.sleep(1.0)
        os.write(fd, b"\x03")  # VINTR from the terminal: the tty driver sends SIGINT to the foreground group
    if cmd == "exit":
        break
    ok = read_until(PROMPT) and ok
try:
    _, status = os.waitpid(pid, 0)
except ChildProcessError:
    status = 0
sys.stdout.write("".join(transcript).replace("\r", ""))
print(f"\n[w5] shell={shell} all_prompts_seen={ok} exit_status={os.waitstatus_to_exitcode(status)}")
