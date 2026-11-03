#!/bin/sh
# The export check must fail on broken copies of the real libc.a (TASK-17):
# an extra export, a missing tier name, a stray svc, a name of the exclusion
# table, and a name defined strongly twice. Each case is a copy of libc.a with one change.
#
# Usage: check-broken-archives.sh CHECK AR ZCC LIST LIBC WORKDIR [SVC_MEMBER]
# SVC_MEMBER is the prefix of the one member that may hold svc (zoraya-rt's,
# INV-0100-2).
set -eu
check=$1 ar=$2 zcc=$3 list=$4 libc=$5 dir=$6
allow=${7:+--allow-svc $7}
rm -rf "$dir"
mkdir -p "$dir"
cd "$dir"

# An archive that passes, so that each failure below is the change's.
"$check" check "$list" "$libc" $allow > good.log || { cat good.log; echo "check-broken: the real libc.a fails"; exit 1; }

expect() {
    case=$1 want=$2
    if "$check" check "$list" "$case.a" $allow > "$case.log" 2>&1; then
        cat "$case.log"
        echo "check-broken: $case: the check passed a broken archive"
        exit 1
    fi
    if ! grep -q -- "$want" "$case.log"; then
        cat "$case.log"
        echo "check-broken: $case: the check failed without reporting '$want'"
        exit 1
    fi
    echo "check-broken: $case: fails as it should ($want)"
}

printf 'int strlcpy(char *d, const char *s, unsigned long n) { return 0; }\n' > extra.c
"$zcc" -w -c -o extra.o extra.c
cp "$libc" extra.a && "$ar" r extra.a extra.o
expect extra "export outside the tier list (INV-0100-1): strlcpy"

cp "$libc" missing.a && "$ar" d missing.a puts.o
expect missing "tier list name not defined: puts"

printf '\t.text\n\t.globl __stray\n__stray:\n\tsvc #0\n\tret\n' > stray.s
"$zcc" -c -o stray.o stray.s
cp "$libc" stray.a && "$ar" r stray.a stray.o
expect stray "svc in stray.o"

printf 'unsigned alarm(unsigned s) { return 0; }\n' > denied.c
"$zcc" -c -o denied.o denied.c
cp "$libc" denied.a && "$ar" r denied.a denied.o
expect denied "export on the exclusion table (INV-0005-2): alarm"

printf 'unsigned long strlen(const char *s) { return 0; }\n' > twice.c
"$zcc" -w -c -o twice.o twice.c
cp "$libc" twice.a && "$ar" r twice.a twice.o
expect twice "defined strongly in more than one member: strlen"
