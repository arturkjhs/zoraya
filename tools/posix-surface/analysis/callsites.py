"""M3 candidate call sites for the conflict-critical features.

A site is a source line matching a feature pattern AND present in the DWARF line table of the measured binary
(objdump --dwarf=decodedline), i.e. code that was actually compiled and linked (inlined code included, dead
code and unconfigured applets excluded). For toybox/busybox the binary is the per-tier minimal build for T2
(only applets the workloads executed), so sites are limited to code those applets can reach.

Each site gets a rule-based first classification (rule_class/rule_effort/rule_reason). These are estimates by
rule, refined by review in docs/research/task-02/callsite-analysis.md.

Usage: callsites.py <work>   -> <work>/results/callsites.tsv, callsites-summary.tsv
"""
import os, re, sys, glob, subprocess, collections

W = sys.argv[1]
BLD, RES = os.path.join(W, "build"), os.path.join(W, "results")

# program -> (source root, binary with debug info, tier, kind)
PROGS = {
    "dash":    ("dash",          "dash/src/dash", "T0", "core"),
    "oksh":    ("oksh",          "oksh/oksh", "T0", "comparison"),
    "mrsh":    ("mrsh",          "mrsh/mrsh", "T0", "comparison"),
    "toybox":  ("toybox-T2",     "toybox-T2/generated/unstripped/toybox", "T0-T2", "core"),
    "busybox": ("busybox-T2",    "busybox-T2/busybox_unstripped", "T0-T2", "core"),
    "tcc":     ("tcc",           "tcc/tcc", "T1", "core"),
    "gmake":   ("gmake",         "gmake/make", "T1", "core"),
    "pdpmake": ("pdpmake",       "pdpmake/make", "T1", "comparison"),
}

F_PROC = r"fork|vfork|execl|execlp|execle|execv|execvp|execve|execvpe|fexecve|posix_spawnp?|wait|waitpid|wait3|wait4|waitid|pipe2?|popen|pclose|system"
F_SIG = r"signal|sigaction|sigprocmask|pthread_sigmask|kill|killpg|raise|sigsuspend|sigwait|sigtimedwait|alarm|setitimer|sigsetjmp|siglongjmp|sigemptyset|sigfillset|sigaddset|sigdelset|sigismember|bsd_signal|xsignal|signal_no_SA_RESTART_empty_mask|signal_SA_RESTART_empty_mask|bb_signals"
F_TTY = r"tcgetattr|tcsetattr|tcgetpgrp|tcsetpgrp|isatty|ttyname|setpgid|setsid|getpgrp|setpgrp|getsid|tcdrain|tcflush|cfmakeraw|cfsetispeed|cfsetospeed|ctermid"
F_ID = r"getuid|geteuid|getgid|getegid|setuid|seteuid|setgid|setegid|setreuid|setregid|setresuid|setresgid|setgroups|getgroups|initgroups|chmod|fchmod|fchmodat|lchmod|chown|fchown|lchown|fchownat|umask|getpwnam|getpwuid|getgrnam|getgrgid|getlogin|access|faccessat|euidaccess|eaccess"
F_PATH = r"getcwd|chdir|fchdir|realpath|tmpfile|mkstemps?|mkdtemp|mktemp|tmpnam|tempnam|chroot"
F_ENV = r"getenv|setenv|unsetenv|putenv|clearenv|secure_getenv"
F_OTHER = {  # DM-flagged APIs (TASK-01 catalog)
    "DM-053 ioctl": r"ioctl",
    "DM-023 select/poll": r"select|pselect|poll|ppoll|epoll_\w+",
    "DM-037 fsync": r"fsync|fdatasync|sync|syncfs",
    "DM-038 rename": r"rename|renameat2?",
    "DM-039 symlink/lstat": r"symlink|symlinkat|readlink|readlinkat|lstat|lchown",
    "DM-036 record locks": r"flock|lockf",
    "DM-031 mmap": r"mmap|munmap|mremap",
    "DM-097 locale": r"setlocale|nl_langinfo|newlocale|uselocale",
    "DM-060 non-reentrant": r"strtok|localtime|gmtime|ctime|asctime|getpwent|getgrent|strerror|readdir|ttyname|getlogin",
    "DM-065 wall clock": r"time|gettimeofday|clock_gettime",
}
FEATURES = [("1 process", F_PROC), ("2 signals", F_SIG), ("4 tty/job control", F_TTY),
            ("5 identity/permissions", F_ID), ("3 global path/cwd", F_PATH), ("6 environment (DM-085)", F_ENV)] + \
           [("6 " + k, v) for k, v in F_OTHER.items()]
CALL = {name: re.compile(r"(?<![\w.>])(" + pat + r")\s*\(") for name, pat in FEATURES}
ABS = re.compile(r'"(/(?:bin|usr|tmp|dev|proc|etc|var|sys|lib)\b[^"]*)"')
FCNTL_LOCK = re.compile(r"F_SETLKW?|F_GETLK|F_OFD_")
SIGNAME = re.compile(r"\bSIG(CHLD|INT|QUIT|TERM|HUP|PIPE|TSTP|TTIN|TTOU|CONT|STOP|ALRM|WINCH|USR1|USR2|KILL|XCPU|XFSZ)\b")

def line_table(binary):
    out = subprocess.run(["objdump", "--dwarf=decodedline", binary], capture_output=True, text=True).stdout
    have = collections.defaultdict(set)
    cur = None
    for line in out.splitlines():
        m = re.match(r"^(\S+\.[ch]):$", line) or re.match(r"^CU: (\S+\.[ch]):?$", line)
        if m:
            cur = os.path.basename(m.group(1))
            continue
        parts = line.split()
        if len(parts) >= 3 and parts[1].isdigit() and cur:
            have[os.path.basename(parts[0])].add(int(parts[1]))
    return have

def functions(path):
    out = subprocess.run(["ctags", "-x", "--c-kinds=f", "--language-force=C", path],
                         capture_output=True, text=True).stdout
    fs = []
    for line in out.splitlines():
        p = line.split()
        if len(p) >= 3 and p[2].isdigit():
            fs.append((int(p[2]), p[0]))
    return sorted(fs)

def enclosing(fs, ln):
    name = "?"
    for start, n in fs:
        if start <= ln:
            name = n
        else:
            break
    return name

def rule(feature, api, code, fn, prog):
    """first-pass replaceability by rule; every value is an estimate"""
    c = code
    shell = prog in ("dash", "oksh", "mrsh") or (prog == "busybox" and "ash" in fn)
    if feature == "1 process":
        if api in ("posix_spawn", "posix_spawnp"):
            return "direct", 0, "already posix_spawn"
        if api == "vfork":
            return "direct", 15, "vfork+exec child: posix_spawn with file actions"
        if api == "fork":
            if shell:
                return "emulate", 0, "shell fork (subshell/pipeline/background job): POSIX semantics of the language"
            return "restructure", 40, "fork: check whether exec follows; else needs separate process design"
        if api.startswith("exec"):
            return ("emulate" if shell else "direct"), (0 if shell else 5), \
                   "shell exec of a command" if shell else "exec in child: fold into spawn"
        if api in ("wait", "waitpid", "wait3", "wait4", "waitid"):
            return ("emulate" if shell else "direct"), (0 if shell else 5), \
                   "shell job status (incl. WUNTRACED)" if shell else "wait for own child: wait on process handle"
        if api in ("pipe", "pipe2"):
            return "direct", 3, "pipe between parent and spawned child: pass handle at spawn"
        if api in ("popen", "pclose", "system"):
            return "direct", 10, "popen/system: spawn of /bin/sh -c with a pipe"
    if feature == "2 signals":
        if "SIGPIPE" in c and ("SIG_IGN" in c or "SIG_DFL" in c):
            return "direct", 1, "SIGPIPE disposition: native write returns EPIPE"
        if "SIGCHLD" in c:
            return ("emulate" if shell else "restructure"), (0 if shell else 30), "child-exit notification"
        if re.search(r"SIG(TSTP|TTIN|TTOU|CONT)", c):
            return "emulate", 0, "job-control signal"
        if re.search(r"SIG(INT|QUIT|HUP|TERM)", c):
            return ("emulate" if shell else "restructure"), (0 if shell else 20), "interrupt/termination handling"
        if api in ("kill", "killpg", "raise"):
            return ("emulate" if shell else "direct"), (0 if shell else 3), "send signal"
        if api in ("alarm", "setitimer"):
            return "restructure", 20, "timeout via signal: native timer/deadline"
        if api.startswith("sig") and ("set" in api or "mask" in api or api == "sigprocmask"):
            return ("emulate" if shell else "direct"), (0 if shell else 2), "signal mask bookkeeping"
        return ("emulate" if shell else "restructure"), (0 if shell else 15), "signal handler/disposition"
    if feature == "4 tty/job control":
        if api == "isatty":
            return "direct", 1, "terminal check: handle type query"
        if api in ("setpgid", "setsid", "tcsetpgrp", "tcgetpgrp", "getpgrp", "setpgrp", "getsid"):
            return "emulate", 0, "process group / session / foreground: job-control semantics"
        return ("emulate" if shell else "restructure"), (0 if shell else 15), "termios mode change"
    if feature == "5 identity/permissions":
        if api in ("chmod", "fchmod", "fchmodat", "umask", "chown", "fchown", "lchown", "fchownat"):
            return "emulate", 0, "file mode/ownership semantics expected by POSIX tools (install, tar, cp -p)"
        if api in ("access", "faccessat", "eaccess", "euidaccess"):
            return "direct", 3, "permission probe: try the operation / capability check"
        if api.startswith(("getpw", "getgr", "getlogin")):
            return "emulate", 0, "user/group database"
        if api.startswith("set"):
            return "restructure", 20, "privilege change"
        return "direct", 2, "identity query"
    if feature == "3 global path/cwd":
        if api in ("chdir", "fchdir", "getcwd"):
            return ("emulate" if shell else "restructure"), (0 if shell else 10), "process-global cwd"
        if api in ("tmpfile", "mkstemp", "mkstemps", "mkdtemp", "mktemp", "tmpnam", "tempnam"):
            return "direct", 3, "temporary file in global /tmp: temp-dir capability"
        if api == "realpath":
            return "restructure", 10, "canonical global path"
        return "restructure", 10, "global path"
    if feature.startswith("6 environment"):
        if "PATH" in c:
            return "emulate", 0, "$PATH search semantics"
        return ("emulate" if api in ("setenv", "putenv", "unsetenv") else "direct"), 2, "environment variable"
    if feature.startswith("6 "):
        return "direct", 5, "DM-flagged API; see catalog entry"
    return "restructure", 10, "unclassified"

rows = []
for prog, (src, binp, tier, kind) in PROGS.items():
    root = os.path.join(BLD, src)
    binary = os.path.join(BLD, binp)
    if not os.path.exists(binary):
        print(f"missing binary {binary}", file=sys.stderr)
        continue
    lt = line_table(binary)
    for path in sorted(glob.glob(os.path.join(root, "**", "*.[ch]"), recursive=True)):
        rel = os.path.relpath(path, root)
        if rel.startswith(("tests", "testsuite", "scripts", "kconfig", "generated", "win32", "tests/")) or "/tests/" in rel:
            continue
        lines_ok = lt.get(os.path.basename(path), set())
        if not lines_ok:
            continue
        try:
            text = open(path, errors="replace").read().split("\n")
        except OSError:
            continue
        fs = None
        for ln, code in enumerate(text, 1):
            if ln not in lines_ok:
                continue
            s = code.strip()
            if s.startswith(("//", "*", "/*", "#")):
                continue
            found = []
            for feat, rx in CALL.items():
                for m in rx.finditer(code):
                    found.append((feat, m.group(1)))
            if "fcntl" in code and FCNTL_LOCK.search(code):
                found.append(("6 DM-036 record locks", "fcntl-lock"))
            for m in ABS.finditer(code):
                found.append(("3 global path/cwd", "literal:" + m.group(1)))
            if re.search(r'getenv\s*\(\s*"PATH"', code):
                found.append(("3 global path/cwd", "PATH-lookup"))
            if not found:
                continue
            if fs is None:
                fs = functions(path)
            fn = enclosing(fs, ln)
            for feat, api in sorted(set(found)):
                if api.startswith("literal:"):
                    lit = api[8:]
                    if lit.startswith("/dev/null"):
                        cls, eff, why = "direct", 2, "null device: pass a null handle"
                    elif lit.startswith("/dev/tty"):
                        cls, eff, why = "emulate", 0, "controlling terminal by name"
                    elif lit.startswith(("/bin/sh", "/bin/", "/usr/bin")):
                        cls, eff, why = "direct", 3, "hard-coded program path: configurable/handle"
                    elif lit.startswith("/tmp"):
                        cls, eff, why = "direct", 3, "global /tmp: temp-dir capability"
                    elif lit.startswith("/proc"):
                        cls, eff, why = "restructure", 15, "procfs text interface (DM-054)"
                    elif lit.startswith("/etc"):
                        cls, eff, why = "restructure", 10, "global configuration file"
                    else:
                        cls, eff, why = "restructure", 10, "global path"
                else:
                    cls, eff, why = rule(feat, api, code, fn, prog)
                sig = ",".join(sorted(set(SIGNAME.findall(code)))) if feat == "2 signals" else ""
                rows.append((prog, kind, tier, feat, api, rel, str(ln), fn, s[:160].replace("\t", " "),
                             cls, str(eff), why, sig))

with open(os.path.join(RES, "callsites.tsv"), "w") as f:
    f.write("program\tkind\ttier\tfeature\tapi\tfile\tline\tfunction\tcode\trule_class\trule_effort_lines\trule_reason\tsignals\n")
    for r in rows:
        f.write("\t".join(r) + "\n")
cnt = collections.Counter((r[0], r[3], r[9]) for r in rows)
with open(os.path.join(RES, "callsites-summary.tsv"), "w") as f:
    f.write("program\tfeature\tclass\tsites\n")
    for (p, ft, c), n in sorted(cnt.items()):
        f.write(f"{p}\t{ft}\t{c}\t{n}\n")
by = collections.Counter((r[0], r[3]) for r in rows)
for (p, ft), n in sorted(by.items()):
    print(f"{p}\t{ft}\t{n}")
print("total", len(rows))
