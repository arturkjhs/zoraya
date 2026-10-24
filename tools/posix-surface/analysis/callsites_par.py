"""TASK-02b M7 candidate call sites: jobserver, readiness waiting and child reaping, signal handling and masks,
cleanup on interruption, in GNU make, pdpmake and dash.

Same filter as M3 (callsites.py): a site is a matching source line present in the DWARF line table of the measured
binary at an address inside an executable section. Feature assignment:
  1 jobserver      - any call inside a jobserver_* function (GNU make posixos.c), and every call of a jobserver_*
                     function from elsewhere (the client side in job.c/main.c)
  2 wait/reap      - readiness waits and child reaping (pselect/select/poll, wait*, sigsuspend, system)
  3 signals        - signal dispositions and masks
  4 cleanup        - deleting incomplete targets and terminating children on interruption
A line can carry several features. Classes, effort and the event-model mapping come from review
(review/m7-*.tsv), merged by callsite_par_report.py; there is no rule-based class here.
Usage: callsites_par.py <work> -> <work>/results/par/callsites-par.tsv
"""
import os, re, sys, glob, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dwarfsites import line_table, functions, enclosing

W = sys.argv[1]
BLD, RES = os.path.join(W, "build"), os.path.join(W, "results", "par")
os.makedirs(RES, exist_ok=True)
PROGS = {"gmake": ("gmake", "gmake/make"), "pdpmake": ("pdpmake", "pdpmake/make"), "dash": ("dash", "dash/src/dash")}

API_WAIT = r"pselect|select|poll|ppoll|epoll_\w+|wait|waitpid|wait3|wait4|waitid|sigsuspend|system|reap_children|block_sigs|unblock_sigs"
API_SIG = r"FATAL_SIG|signal|bsd_signal|sigaction|sigprocmask|pthread_sigmask|sigemptyset|sigfillset|sigaddset|sigdelset|sigismember|raise|kill|killpg|setsignal|ignoresig|sigblockall|sigclearmask|alarm|setitimer"
API_CLEAN = r"unlink|unlinkat|remove|delete_target|delete_child_targets|kill|killpg|remove_intermediates"
API_IO = r"read|write|open|close|pipe|pipe2|mkfifo|mknod|fcntl|dup|dup2|fd_noinherit|fd_inherit|set_blocking|unlink"
CALL = lambda pat: re.compile(r"(?<![\w.>])(" + pat + r")\s*\(")
C_WAIT, C_SIG, C_CLEAN, C_IO = CALL(API_WAIT), CALL(API_SIG), CALL(API_CLEAN), CALL(API_IO)
C_JS = re.compile(r"(?<![\w.>])(jobserver_\w+)\s*\(")
CLEANUP_FN = re.compile(r"fatal_error_signal|delete_child_targets|delete_target|die|make_handler|remove_target|onint|exitshell|exraise")
SIGNAME = re.compile(r"\bSIG(CHLD|INT|QUIT|TERM|HUP|PIPE|TSTP|TTIN|TTOU|CONT|STOP|ALRM|USR1|USR2|KILL|XCPU|XFSZ)\b")
SKIPDIR = ("tests", "testsuite", "scripts", "win32", "w32", "vms", "amiga", "doc")

rows = []
for prog, (src, binp) in PROGS.items():
    root, binary = os.path.join(BLD, src), os.path.join(BLD, binp)
    if not os.path.exists(binary):
        print(f"missing binary {binary}", file=sys.stderr)
        continue
    lt = line_table(binary)
    for path in sorted(glob.glob(os.path.join(root, "**", "*.[ch]"), recursive=True)):
        rel = os.path.relpath(path, root)
        if rel.split("/")[0] in SKIPDIR:
            continue
        ok = lt.get(os.path.basename(path), set())
        if not ok:
            continue
        text = open(path, errors="replace").read().split("\n")
        fs = None
        for ln, code in enumerate(text, 1):
            if ln not in ok:
                continue
            s = code.strip()
            if s.startswith(("//", "*", "/*", "#")):
                continue
            found = set()
            if fs is None and (C_WAIT.search(code) or C_SIG.search(code) or C_CLEAN.search(code) or
                               C_IO.search(code) or C_JS.search(code)):
                fs = functions(path)
            if fs is None:
                continue
            fn = enclosing(fs, ln)
            fstart = {n: st for st, n in fs}
            injs = fn.startswith("jobserver_") or fn in ("free_job_token",)
            for m in C_JS.finditer(code):
                found.add(("1 jobserver", m.group(1)))
            if injs:
                for rx in (C_IO, C_WAIT, C_SIG):
                    for m in rx.finditer(code):
                        found.add(("1 jobserver", m.group(1)))
            for m in C_WAIT.finditer(code):
                found.add(("2 wait/reap", m.group(1)))
            for m in C_SIG.finditer(code):
                found.add(("3 signals", m.group(1)))
            if CLEANUP_FN.search(fn):
                for m in C_CLEAN.finditer(code):
                    found.add(("4 cleanup", m.group(1)))
            for feat, api in sorted(found):
                if fstart.get(api) == ln:
                    continue   # definition, not a call
                if feat == "2 wait/reap" and api in ("signal",):
                    continue
                sig = ",".join(sorted(set(SIGNAME.findall(code)))) if feat in ("3 signals", "4 cleanup") else ""
                rows.append((prog, feat, api, rel, str(ln), fn, s[:160].replace("\t", " "), sig))

with open(os.path.join(RES, "callsites-par.tsv"), "w") as f:
    f.write("program\tfeature\tapi\tfile\tline\tfunction\tcode\tsignals\n")
    for r in rows:
        f.write("\t".join(r) + "\n")
c = collections.Counter((r[0], r[1]) for r in rows)
for k, v in sorted(c.items()):
    print(*k, v, sep="\t")
print("total", len(rows))
