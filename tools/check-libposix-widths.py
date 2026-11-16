#!/usr/bin/env python3
"""ADR-0451 item 1, INV-0451-1: every libposix definition returns the width its
C prototype declares.

libposix (crates/zoraya-posix) exports C symbols from Rust. A definition whose
Rust return type is narrower than the prototype in the sysroot's musl headers
loses the upper half of the result on AArch64 (`syscall()` returned `int` where
the prototype says `long`: -1 read as 4294967295). This checks every exported
definition against tools/libposix-widths.txt, the table of the prototypes'
return types (NAME, C return type, width class), offline:

  check-libposix-widths.py                 definitions against the table
  check-libposix-widths.py --headers DIR [--archive A]
                                           also the table against the headers
                                           in DIR (the sysroot's usr/include),
                                           parsed by clang's AST dump, and
                                           the parsed names against the
                                           archive's exports (nm); no fallback
  check-libposix-widths.py --regen DIR     rewrite the table from the headers
  check-libposix-widths.py --selftest      show that a narrowed definition fails

Width classes: v (void), 1, 2, 4, 8 (integers and pointers), f32, f64, S
(a struct returned by value; not checked), '-' (not in any header: libposix's
own zoraya_posix_* names, which take and return `long` for syscall.h).
Sign is not checked, width is.
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "crates", "zoraya-posix", "src")
TABLE = os.path.join(ROOT, "tools", "libposix-widths.txt")

RUST = {
    "i8": "1", "u8": "1", "bool": "1", "c_char": "1",
    "i16": "2", "u16": "2",
    "i32": "4", "u32": "4", "c_int": "4", "c_uint": "4",
    "i64": "8", "u64": "8", "isize": "8", "usize": "8", "c_long": "8", "c_ulong": "8",
    "f32": "f32", "f64": "f64",
    "()": "v", "!": "v",
}


# Names the sysroot's public headers do not declare, with the prototype they
# have where the C code that calls them declares it: musl's hidden internals
# (src/internal/*.h of musl 1.2.5) and the zoraya_posix_* routes
# (ports/musl/zoraya/syscall.h: `long`, as the Linux call returned).
INTERNAL = {
    "__block_all_sigs": ("void", "v"), "__block_app_sigs": ("void", "v"), "__restore_sigs": ("void", "v"),
    "__clock_gettime": ("int", "4"), "__clock_nanosleep": ("int", "4"), "__clone": ("int", "4"),
    "__dup3": ("int", "4"), "__lseek": ("off_t", "8"), "__mmap": ("void *", "8"),
    "__mprotect": ("int", "4"), "__munmap": ("int", "4"), "__libc_calloc": ("void *", "8"),
    "__libc_free": ("void", "v"), "__libc_malloc": ("void *", "8"), "__map_file": ("const unsigned char *", "8"),
    "__unmapself": ("void", "v"), "zoraya_fork_group": ("int", "4"),
    "zoraya_child_memory": ("int", "4"),
    "zoraya_posix_main_stack": ("void", "v"), "zoraya_posix_start": ("void", "v"),
    "zoraya_posix_exit_group": ("void", "v"), "zoraya_posix_thread_exit": ("void", "v"),
}
for _n in ("close", "fcntl", "futex", "interrupt", "ioctl", "mmap", "read", "readv", "rt_sigprocmask",
           "sched_yield", "set_tid_address", "thread_name", "tkill", "wait4", "writev"):
    INTERNAL["zoraya_posix_" + _n] = ("long", "8")


def rust_class(ty):
    ty = ty.strip()
    if ty in RUST:
        return RUST[ty]
    if ty.startswith("*") or ty.startswith("Option<") or ty.startswith("extern"):
        return "8"
    return None


def balanced(s, i):
    """Index just past the parenthesis group that opens at s[i]."""
    depth = 0
    while i < len(s):
        if s[i] == "(":
            depth += 1
        elif s[i] == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    raise ValueError("unbalanced parenthesis")


def strip_comments(s):
    s = re.sub(r"/\*.*?\*/", "", s, flags=re.S)
    return re.sub(r"//[^\n]*", "", s)


def definitions(text):
    """(name, rust return type) of every exported extern "C" definition.

    Fails (ValueError) on anything that exports or looks like it exports and
    that this parser does not consume: another attribute form, a macro that
    emits definitions, a `not_yet!` of another shape or kind. A definition the
    parser cannot read must not escape the check silently."""
    text = strip_comments(text)
    out = []
    # The stubs' macro first: its arms give each kind's type, and its body's
    # own attribute is a template, not a definition.
    kinds = {}
    mac = re.search(r"macro_rules!\s+not_yet\s*\{", text)
    if mac:
        body_end = balanced_brace(text, mac.end() - 1)
        body = text[mac.end():body_end]
        kinds = dict(re.findall(r"\((\w+)\s+\$\(\$name:ident\)\*\)\s*=>\s*\{\$\(.*?->\s*([^{]+?)\s*\{", body, re.S))
        text = text[:mac.start()] + text[body_end:]
    # `#[unsafe(no_mangle)]` or `#[unsafe(export_name = "x")]`, then the fn.
    for m in re.finditer(r'#\[unsafe\((no_mangle|export_name\s*=\s*"(\w+)")\)\]', text):
        f = re.compile(r'\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?'
                       r'extern\s+"C(?:-unwind)?"\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(').match(text, m.end())
        if not f:
            raise ValueError("an exported attribute not followed by `extern \"C\" fn NAME(`: " + text[m.start():m.start() + 80])
        end = balanced(text, f.end() - 1)
        r = re.compile(r"\s*(?:->\s*([^{;]+?))?\s*(?:where[^{]*)?\{").match(text, end)
        if not r:
            raise ValueError("cannot read the return type of " + f.group(1))
        out.append((m.group(2) or f.group(1), (r.group(1) or "()").strip()))
    rest = re.sub(r'#\[unsafe\((?:no_mangle|export_name\s*=\s*"\w+")\)\]', "", text)
    # not_yet!(KIND a b c): the stubs' macro, whose arms give each kind's type.
    for m in re.finditer(r"\bnot_yet\s*!\s*([(\[{])\s*(\w+)\s+([^)\]}]*)[)\]}]\s*;?", rest):
        if m.group(2) not in kinds:
            raise ValueError("not_yet! kind `%s` has no arm in the macro" % m.group(2))
        for n in m.group(3).split():
            out.append((n, kinds[m.group(2)]))
    rest = re.sub(r"\bnot_yet\s*!\s*([(\[{])\s*(\w+)\s+([^)\]}]*)[)\]}]\s*;?", "", rest)
    # Whatever is left must hold no export attribute, no stub invocation and no
    # macro that emits `extern "C"` functions.
    for pat, what in ((r"no_mangle", "no_mangle"), (r"export_name", "export_name"), (r"\bnot_yet\b", "not_yet"),
                      (r"link_name", "link_name")):
        m = re.search(pat, rest)
        if m:
            raise ValueError("`%s` in a form this checker does not read: %s" % (what, rest[max(0, m.start() - 40):m.start() + 60].strip()))
    for mm in re.finditer(r"macro_rules!\s+(\w+)\s*\{", rest):
        e = balanced_brace(rest, mm.end() - 1)
        if re.search(r'extern\s+"C', rest[mm.end():e]):
            raise ValueError("macro `%s` emits extern \"C\" items: this checker does not read macros" % mm.group(1))
    return out


def balanced_brace(s, i):
    """Index just past the brace group that opens at s[i]."""
    depth = 0
    while i < len(s):
        if s[i] == "{":
            depth += 1
        elif s[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    raise ValueError("unbalanced brace")


def all_definitions(src=SRC):
    defs = {}
    for dp, _, fs in os.walk(src):
        for f in sorted(fs):
            if f.endswith(".rs"):
                with open(os.path.join(dp, f)) as fh:
                    try:
                        found = definitions(fh.read())
                    except ValueError as e:
                        print("check-libposix-widths: FAILED\n  %s: %s" % (os.path.relpath(os.path.join(dp, f), ROOT), e),
                              file=sys.stderr)
                        sys.exit(1)
                    for name, ty in found:
                        defs[name] = (ty, os.path.relpath(os.path.join(dp, f), ROOT))
    return defs


def read_table():
    t = {}
    with open(TABLE) as fh:
        for line in fh:
            line = line.split("#")[0].strip()
            if line:
                name, ctype, cls = line.split(None, 2)
                t[name] = (ctype.replace("_", " "), cls.strip())
    return t


def c_class(qt):
    """Width class of a desugared C return type."""
    qt = qt.strip()
    if qt == "void":
        return "v"
    if "*" in qt:
        return "8"
    if qt in ("float",):
        return "f32"
    if qt in ("double",):
        return "f64"
    if qt in ("char", "signed char", "unsigned char", "_Bool"):
        return "1"
    if qt in ("short", "unsigned short"):
        return "2"
    if qt in ("int", "unsigned int"):
        return "4"
    if qt in ("long", "unsigned long", "long long", "unsigned long long"):
        return "8"
    if qt.startswith(("struct ", "union ")) or qt == "long double":
        return "S"
    if qt.startswith("enum "):
        return "4"
    return "?"


# Headers that include a Linux kernel header the sysroot does not carry; they
# declare no function libposix defines.
SKIP_HEADERS = {"sys/kd.h", "sys/vt.h", "sys/soundcard.h"}
# Compiler-runtime headers the C++ runtime puts in the same directory (LLVM's
# libunwind, ports/Makefile): not a libc ABI, and two of them conflict.
SKIP_PATTERN = re.compile(r"^(unwind.*|libunwind.*|__.*)\.h$")


def header_prototypes(inc):
    """name -> (C return type as written, desugared class) for every function
    the headers in INC declare, from clang's AST dump."""
    tu = os.path.join(os.environ.get("TMPDIR", "/tmp"), "libposix-widths-%d.c" % os.getpid())
    heads = []
    for sub in ("", "sys", "arpa", "net", "netinet"):
        d = os.path.join(inc, sub)
        if os.path.isdir(d):
            heads += [os.path.join(sub, h) for h in sorted(os.listdir(d)) if h.endswith(".h") and os.path.join(sub, h) not in SKIP_HEADERS
                       and not (sub == "" and SKIP_PATTERN.match(h))]
    try:
        with open(tu, "w") as fh:
            fh.write("".join("#include <%s>\n" % h for h in heads))
        res = subprocess.run(
            ["clang", "--target=aarch64-linux-musl", "-nostdinc", "-isystem", inc,
             "-isystem", subprocess.check_output(["clang", "-print-resource-dir"], text=True).strip() + "/include",
             "-D_GNU_SOURCE", "-w", "-fsyntax-only", "-Xclang", "-ast-dump=json", tu],
            capture_output=True, text=True)
    finally:
        if os.path.exists(tu):
            os.unlink(tu)
    if res.returncode != 0 or not res.stdout:
        raise SystemExit("check-libposix-widths: clang failed (status %d): %s" % (res.returncode, res.stderr[:400]))
    protos = {}
    typedefs = {}

    def resolve(t):
        t = re.sub(r"\b(const|volatile|restrict|__restrict|extern|static|inline)\b", "", t).strip()
        for _ in range(20):
            if t not in typedefs:
                break
            t = typedefs[t].strip()
        return t

    def walk(node):
        if node.get("kind") == "TypedefDecl" and node.get("name"):
            ty = node.get("type", {})
            typedefs[node["name"]] = ty.get("desugaredQualType", ty.get("qualType", ""))
        if node.get("kind") == "FunctionDecl":
            q = node.get("type", {}).get("qualType", "")
            if "(" in q and node.get("name"):
                head, rest = q.split("(", 1)
                if rest.lstrip().startswith("*"):
                    # a function returning a function pointer (signal)
                    protos.setdefault(node["name"], (head.strip() + " (*)()", "8"))
                else:
                    protos.setdefault(node["name"], (head.strip(), head.strip()))
        for k in node.get("inner", []):
            walk(k)

    walk(json.loads(res.stdout))
    return dict((n, (w, d if d == "8" else c_class(resolve(d)))) for n, (w, d) in protos.items())


def archive_exports(archive):
    """The global text symbols of the `zoraya_posix` member of libzoraya_posix.a
    (Rust-mangled names left out): what libc.a gets from this crate."""
    if not os.path.isfile(archive):
        raise SystemExit("check-libposix-widths: no archive " + archive)
    res = subprocess.run(["nm", "-g", "--defined-only", "-A", archive], capture_output=True, text=True)
    if res.returncode != 0:
        raise SystemExit("check-libposix-widths: nm failed: " + res.stderr[:300])
    names = set()
    for line in res.stdout.splitlines():
        m = re.match(r"^.*?:(zoraya_posix-[^:]*):\s*[0-9a-f]+ [Tt] (\S+)$", line)
        if m and not m.group(2).startswith(("_R", "_ZN")):
            names.add(m.group(2))
    if not names:
        raise SystemExit("check-libposix-widths: no zoraya_posix member exports in " + archive)
    return names


def compare(defs, table):
    errors = []
    for name in sorted(defs):
        rty, where = defs[name]
        rc = rust_class(rty)
        if rc is None:
            errors.append("%s (%s): Rust return type `%s` is not one this checker knows" % (name, where, rty))
            continue
        if name not in table:
            errors.append("%s (%s): no row in tools/libposix-widths.txt "
                          "(run: tools/check-libposix-widths.py --regen build/sysroot/usr/include)" % (name, where))
            continue
        ctype, cc = table[name]
        if cc == "?":
            errors.append("%s: the table row has class ? (a return type the checker could not classify)" % name)
            continue
        if cc in ("-", "S"):
            continue
        if rc != cc:
            errors.append("%s (%s): returns `%s` (%s) but the C prototype returns `%s` (%s)"
                          % (name, where, rty, rc, ctype, cc))
    return errors


def selftest():
    good = '#[unsafe(no_mangle)]\nextern "C" fn syscall() -> i64 { 0 }\n' \
           '#[unsafe(no_mangle)]\nextern "C" fn tee(f: extern "C" fn(i32) -> !) -> isize { 0 }\n'
    bad = good.replace("-> i64", "-> i32")
    table = {"syscall": ("long", "8"), "tee": ("ssize t", "8")}
    d = dict((n, (t, "selftest")) for n, t in definitions(good))
    assert sorted(d) == ["syscall", "tee"], d
    assert compare(d, table) == [], compare(d, table)
    d = dict((n, (t, "selftest")) for n, t in definitions(bad))
    e = compare(d, table)
    assert len(e) == 1 and e[0].startswith("syscall"), e
    stub = 'macro_rules! not_yet {\n    (int $($name:ident)*) => {$(\n        extern "C" fn $name() -> i32 { 0 }\n    )*};\n' \
           '    (ptr $($name:ident)*) => {$(\n        extern "C" fn $name() -> *mut u8 { 0 }\n    )*};\n' \
           '    (long $($name:ident)*) => {$(\n        extern "C" fn $name() -> i64 { 0 }\n    )*};\n}\n' \
           'not_yet!(int tee);\nnot_yet!(ptr __map_file);\nnot_yet!(long syscall);\n'
    assert sorted(definitions(stub)) == [("__map_file", "*mut u8"), ("syscall", "i64"), ("tee", "i32")], definitions(stub)
    assert len(compare(dict((n, (t, "selftest")) for n, t in definitions(stub)), dict(table, __map_file=("void *", "8")))) == 1
    # What the parser does not read must fail, not pass unseen.
    macro = 'macro_rules! not_yet {\n    (int $($name:ident)*) => {$(\n        #[unsafe(no_mangle)]\n' \
            '        extern "C" fn $name() -> i32 { 0 }\n    )*};\n}\n'
    unread = {
        "not_yet! kind with no arm": macro + "not_yet!(wide a);\n",
        "another emitting macro": macro + 'macro_rules! stub {\n    ($n:ident) => { #[unsafe(no_mangle)] extern "C" fn $n() -> i32 { 0 } };\n}\n',
        "cfg_attr": '#[cfg_attr(all(), unsafe(no_mangle))]\nextern "C" fn a() -> i32 { 0 }\n',
        "export_name = concat!": '#[unsafe(export_name = concat!("a", "b"))]\nextern "C" fn a() -> i32 { 0 }\n',
        "attribute without a fn": '#[unsafe(no_mangle)]\npub static X: i32 = 0;\n',
        "link_name": '#[link_name = "x"]\nextern "C" fn a() -> i32 { 0 }\n',
    }
    for what, src in unread.items():
        try:
            definitions(src)
        except ValueError:
            continue
        raise AssertionError("the parser read over " + what)
    # The indented, brace and bracket forms are read, not skipped.
    assert definitions(macro + "mod m {\n    not_yet!(int a);\n}\n") == [("a", "i32")]
    assert definitions(macro + "not_yet! { int a b }\n") == [("a", "i32"), ("b", "i32")]
    assert definitions(macro + "not_yet![int a];\n") == [("a", "i32")]
    print("check-libposix-widths: selftest ok")


def main(argv):
    if argv[1:] == ["--selftest"]:
        return selftest()
    defs = all_definitions()
    if argv[1:2] == ["--regen"]:
        protos = header_prototypes(argv[2])
        lines = []
        for name in sorted(defs):
            if name in INTERNAL:
                w, c = INTERNAL[name]
                lines.append("%s %s %s" % (name, w.replace(" ", "_"), c))
            elif name in protos:
                w, c = protos[name]
                lines.append("%s %s %s" % (name, w.replace(" ", "_"), c))
            else:
                lines.append("%s - -" % name)
        with open(TABLE, "w") as fh:
            fh.write("# NAME C-RETURN-TYPE WIDTH-CLASS of every libposix definition (ADR-0451 item 1,\n"
                     "# INV-0451-1). Written by tools/check-libposix-widths.py --regen from the sysroot's\n"
                     "# headers; `-` is a name no header declares. Classes: see that script.\n")
            fh.write("\n".join(lines) + "\n")
        print("check-libposix-widths: wrote %d rows" % len(lines))
        return
    table = read_table()
    errors = compare(defs, table)
    stale = sorted(set(table) - set(defs))
    for n in stale:
        errors.append("%s: a row in tools/libposix-widths.txt with no definition (remove it)" % n)
    note = ""
    if argv[1:2] == ["--headers"]:
        protos = header_prototypes(argv[2])
        for name, (ctype, cls) in sorted(table.items()):
            if name in INTERNAL:
                if INTERNAL[name][1] != cls:
                    errors.append("%s: the table says %s, the script's INTERNAL says %s" % (name, cls, INTERNAL[name][1]))
            elif name in protos:
                if protos[name][1] != cls:
                    errors.append("%s: the table says %s, the headers say %s" % (name, cls, protos[name][1]))
            elif cls != "-":
                errors.append("%s: the table has a prototype, the headers declare none" % name)
        note = ", table checked against the headers"
        if "--archive" in argv:
            have = archive_exports(argv[argv.index("--archive") + 1])
            for n in sorted(have - set(defs)):
                errors.append("%s: exported by the archive, found by no definition the parser read" % n)
            for n in sorted(set(defs) - have):
                errors.append("%s: parsed as a definition, not exported by the archive" % n)
            note += ", names equal to the archive's exports"
    if errors:
        print("check-libposix-widths: FAILED", file=sys.stderr)
        for e in errors:
            print("  " + e, file=sys.stderr)
        sys.exit(1)
    print("check-libposix-widths: ok (%d definitions%s)" % (len(defs), note))


if __name__ == "__main__":
    main(sys.argv)
