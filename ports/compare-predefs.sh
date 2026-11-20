#!/bin/sh
# The two compilers of the Zoraya toolchain, zcc and tcc, must predefine the
# same identity (ADR-0143, INV-0143-1): the OS macros (__zoraya__, and none of
# Unix or Linux), the object format, the architecture and the ABI's types.
# Compiler-specific macros (__clang__, __TINYC__, feature macros) are not
# compared. Usage: compare-predefs.sh ZCC TCC WORKDIR
set -eu
zcc=$1 tcc=$2 dir=$3
mkdir -p "$dir"
names='__zoraya__ __unix__ __unix unix __linux__ __linux linux __gnu_linux__ __ELF__
__aarch64__ __AARCH64EL__ __LP64__ _LP64 __CHAR_UNSIGNED__ __BYTE_ORDER__
__SIZEOF_POINTER__ __SIZEOF_LONG__ __SIZE_TYPE__ __PTRDIFF_TYPE__ __WCHAR_TYPE__
__WINT_TYPE__ __INT64_TYPE__ __APPLE__ _WIN32 __FreeBSD__ __ANDROID__'

# One line per name: the name and its value, or "undefined". "long int" and
# "long unsigned int" are clang's spellings of tcc's "long" and "unsigned long".
pick() {
    for n in $names; do
        v=$(awk -v n="$n" '$1 == "#define" && $2 == n { $1 = ""; $2 = ""; sub(/^  /, ""); print; exit }' "$1")
        [ -n "$v" ] || v=undefined
        echo "$n $v"
    done | sed -e 's/long unsigned int/unsigned long/' -e 's/long int/long/'
}

"$zcc" -dM -E -x c /dev/null > "$dir/zcc.h"
"$tcc" -dM -E - < /dev/null > "$dir/tcc.h"
pick "$dir/zcc.h" > "$dir/zcc.txt"
pick "$dir/tcc.h" > "$dir/tcc.txt"
if ! diff "$dir/zcc.txt" "$dir/tcc.txt"; then
    echo "compare-predefs: zcc (<) and tcc (>) predefine different identities"
    exit 1
fi
grep -qx '__zoraya__ 1' "$dir/zcc.txt" || { echo "compare-predefs: no __zoraya__"; exit 1; }
for n in __unix__ __unix unix __linux__ __linux linux __gnu_linux__; do
    grep -qx "$n undefined" "$dir/zcc.txt" || { echo "compare-predefs: $n is defined"; exit 1; }
done
# INV-0452-2: for a Rust build (ZORAYA_RUST=1) zcc accepts exactly the cc
# crate's --target=aarch64-unknown-linux-musl, drops it, and predefines the
# same identity (no Linux); with no ZORAYA_RUST, and for any other target, it
# refuses.
ZORAYA_RUST=1 "$zcc" --target=aarch64-unknown-linux-musl -dM -E -x c /dev/null > "$dir/zcc-rust.h"
pick "$dir/zcc-rust.h" > "$dir/zcc-rust.txt"
diff "$dir/zcc.txt" "$dir/zcc-rust.txt" || { echo "compare-predefs: zcc differs under ZORAYA_RUST=1"; exit 1; }
if "$zcc" --target=aarch64-unknown-linux-musl -dM -E -x c /dev/null > /dev/null 2>&1; then
    echo "compare-predefs: zcc accepts --target without ZORAYA_RUST=1"; exit 1
fi
for t in --target=aarch64-unknown-linux-gnu --target=x86_64-unknown-linux-musl "--target=aarch64-unknown-linux-musl " -target; do
    if ZORAYA_RUST=1 "$zcc" "$t" -dM -E -x c /dev/null > /dev/null 2>&1; then
        echo "compare-predefs: zcc accepts $t under ZORAYA_RUST=1"; exit 1
    fi
done
echo "compare-predefs: zcc and tcc predefine the same identity: $(grep -cv undefined "$dir/zcc.txt") macros, no Unix or Linux"
