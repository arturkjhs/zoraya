"""Generate the aarch64 trampolines and name table for the LD_PRELOAD interposer.

Input: results/static.tsv (union of every measured program's libc function imports).
Output: psurf_tramp.S, psurf_names.c in the given build directory, plus interposed.txt / not-interposed.txt.
"""
import sys, os

RES, OUT = sys.argv[1], sys.argv[2]
# never interposed: the runtime itself calls dlsym; the rest run before or outside normal call paths
EXCLUDE = {"dlsym", "__libc_start_main", "__cxa_finalize", "_init", "_fini"}

funcs, skipped = set(), {}
for line in open(os.path.join(RES, "static.tsv")).read().splitlines()[1:]:
    prog, sym, kind = line.split("\t")
    if kind != "func":
        skipped.setdefault(sym, kind)
    elif sym in EXCLUDE:
        skipped.setdefault(sym, "excluded")
    else:
        funcs.add(sym)
# musl disables its memalign family when malloc is interposed but aligned_alloc is not
# (src/malloc/mallocng/glue.h DISABLE_ALIGNED_ALLOC); interposing it as a pass-through keeps behaviour intact.
if "malloc" in funcs:
    funcs.add("aligned_alloc")
funcs = sorted(funcs)

S = [".text"]
for i, s in enumerate(funcs):
    S.append(f"""
.globl {s}
.type {s},%function
.p2align 4
{s}:
    adrp x16, psurf_slots+{8*i}
    ldr x16, [x16, #:lo12:psurf_slots+{8*i}]
    cbz x16, 1f
    br x16
1:  stp x29, x30, [sp, #-224]!
    mov x29, sp
    stp x0, x1, [sp, #16]
    stp x2, x3, [sp, #32]
    stp x4, x5, [sp, #48]
    stp x6, x7, [sp, #64]
    str x8, [sp, #80]
    stp q0, q1, [sp, #96]
    stp q2, q3, [sp, #128]
    stp q4, q5, [sp, #160]
    stp q6, q7, [sp, #192]
    mov w0, #{i}
    mov x1, x30
    bl psurf_resolve
    mov x16, x0
    ldp x0, x1, [sp, #16]
    ldp x2, x3, [sp, #32]
    ldp x4, x5, [sp, #48]
    ldp x6, x7, [sp, #64]
    ldr x8, [sp, #80]
    ldp q0, q1, [sp, #96]
    ldp q2, q3, [sp, #128]
    ldp q4, q5, [sp, #160]
    ldp q6, q7, [sp, #192]
    ldp x29, x30, [sp], #224
    br x16
.size {s}, .-{s}""")
S.append(f"""
.bss
.p2align 3
.globl psurf_slots
.hidden psurf_slots
psurf_slots: .zero {8*len(funcs)}
.section .note.GNU-stack,"",%progbits
""")
open(os.path.join(OUT, "psurf_tramp.S"), "w").write("\n".join(S))

with open(os.path.join(OUT, "psurf_names.c"), "w") as f:
    f.write("const char *const psurf_names[] = {\n")
    for s in funcs:
        f.write(f'  "{s}",\n')
    f.write("};\nconst unsigned psurf_count = %d;\n" % len(funcs))
with open(os.path.join(RES, "interposed.txt"), "w") as f:
    f.write("\n".join(funcs) + "\n")
with open(os.path.join(RES, "not-interposed.txt"), "w") as f:
    for s, why in sorted(skipped.items()):
        f.write(f"{s}\t{why}\n")
print(f"{len(funcs)} trampolines; {len(skipped)} symbols not interposable (data/crt/excluded)")
