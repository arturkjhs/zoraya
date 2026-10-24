"""M1 static upper bound.

For each measured binary: undefined dynamic symbols (nm -D --undefined-only) that musl's libc.so defines.
Because musl ships libc, libm, libpthread, libdl and librt as one libc.so, "defined by libc.so" is the whole libc
API boundary. Calls musl makes to itself are not visible here by construction (they never go through the
program's PLT), which is what M1 asks for.

Writes results/static.tsv: program  symbol  kind(func|data)
       results/musl-exports.tsv: symbol  kind
       results/static-crosscheck.txt: object-file method vs import-table method for a reference program.
"""
import subprocess, sys, os, glob, collections

RES = sys.argv[1]
MUSL = "/lib/ld-musl-aarch64.so.1"

def nm(args, path):
    out = subprocess.run(["nm", *args, path], capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            yield parts[-2], parts[-1]

# musl exports environ under four names; one data object, counted once.
ALIAS = {"___environ": "environ", "__environ": "environ", "_environ": "environ"}
exports = {}
for t, s in nm(["-D", "--defined-only"], MUSL):
    kind = "func" if t in "TtWwi" else "data"
    exports[ALIAS.get(s, s)] = kind
with open(os.path.join(RES, "musl-exports.tsv"), "w") as f:
    for s in sorted(exports):
        f.write(f"{s}\t{exports[s]}\n")

# referenced by crt1.o / crti.o startup code linked into every executable, or inserted by the compiler (Alpine gcc
# defaults to -fstack-protector-strong), not called by program code
CRT = {"__libc_start_main", "__cxa_finalize", "__deregister_frame_info", "__register_frame_info",
       "_ITM_registerTMCloneTable", "_ITM_deregisterTMCloneTable", "__gmon_start__",
       "__stack_chk_fail", "__stack_chk_guard"}

def imports(path):
    return sorted({ALIAS.get(s, s) for t, s in nm(["-D", "--undefined-only"], path)})

def closure(path, seen=None):
    """path plus every non-musl shared library it needs (for T3 clang/lld)."""
    seen = seen if seen is not None else []
    real = os.path.realpath(path)
    if real in seen:
        return seen
    seen.append(real)
    out = subprocess.run(["ldd", real], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if "=>" in line:
            lib = line.split("=>")[1].split("(")[0].strip()
            if lib and "ld-musl" not in lib and "libc.musl" not in lib and os.path.exists(lib):
                closure(lib, seen)
    return seen

rows = []
progs = {}  # name -> list of ELF files
for b in sorted(glob.glob(os.path.expanduser("~/psurf/prefix/bin/*"))):
    progs[os.path.basename(b)] = [os.path.realpath(b)]
# minimal per-tier multi-call builds (toybox-T1 etc.), produced by minbuild.sh from the dynamic data
for b in sorted(glob.glob(os.path.expanduser("~/psurf/prefix/min/*"))):
    progs[os.path.basename(b)] = [os.path.realpath(b)]
for name, cmd in (("clang", "clang"), ("ld.lld", "ld.lld")):
    p = subprocess.run(["sh", "-c", f"command -v {cmd}"], capture_output=True, text=True).stdout.strip()
    if p:
        progs[name] = [os.path.realpath(p)]
        progs[name + "+libs"] = closure(p)

for name, files in progs.items():
    syms = set()
    for fpath in files:
        syms |= {s for s in imports(fpath) if s in exports}
    for s in sorted(syms):
        rows.append((name, s, "crt" if s in CRT else exports[s]))
with open(os.path.join(RES, "static.tsv"), "w") as f:
    f.write("program\tsymbol\tkind\n")
    for r in rows:
        f.write("\t".join(r) + "\n")
with open(os.path.join(RES, "static-files.tsv"), "w") as f:
    for name, files in progs.items():
        for p in files:
            f.write(f"{name}\t{p}\n")

# cross-check: object-file method (undefined across objects minus defined across objects) for dash
objs = glob.glob(os.path.expanduser("~/psurf/build/dash/src/**/*.o"), recursive=True)
undef, defd = set(), set()
for o in objs:
    for t, s in nm([], o):
        if t == "U":
            undef.add(s)
        elif t in "TtDdBbRrWwCV":
            defd.add(s)
objset = {ALIAS.get(s, s) for s in undef - defd if ALIAS.get(s, s) in exports}
impset = {s for (p, s, k) in rows if p == "dash" and k != "crt"}
with open(os.path.join(RES, "static-crosscheck.txt"), "w") as f:
    f.write(f"dash objects: {len(objs)}\n")
    f.write(f"object-file method: {len(objset)} musl symbols\n")
    f.write(f"import-table method: {len(impset)} musl symbols\n")
    f.write(f"only in objects: {sorted(objset - impset)}\n")
    f.write(f"only in imports: {sorted(impset - objset)}\n")
c = collections.Counter(r[0] for r in rows)
for k, v in c.items():
    print(f"{k}\t{v}")
print(open(os.path.join(RES, "static-crosscheck.txt")).read())
