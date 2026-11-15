#!/bin/sh
# TASK-35 probe build (Route A): Rust's prebuilt std for
# aarch64-unknown-linux-musl, stable toolchain only, linked by tools/zcc
# against a copy of Zoraya's libc.a. Run from the repository root after
# `make -f ports/Makefile sysroot cxx`. Writes build/rust-std-probe/.
set -eu
root=$(pwd)
out=$root/build/rust-std-probe
sys=$out/sysroot
nm=${NM:-llvm-nm}
rm -rf "$sys" && mkdir -p "$sys/usr/lib"
ln -s ../../../sysroot/usr/include "$sys/usr/include"
for f in "$root"/build/sysroot/usr/lib/*; do
    b=$(basename "$f")
    [ "$b" = libc.a ] || ln -s "../../../../sysroot/usr/lib/$b" "$sys/usr/lib/$b"
done
# libposix's Rust runtime inside libc.a defines the allocator shims and the
# panic handler under the __rustc pseudo-crate, whose v0 hash is the same for
# every target of one compiler, so they collide with the program's own std.
# Rename them in a copy, keeping the length (the archive index stays valid).
hash=$("$nm" "$root/build/sysroot/usr/lib/libc.a" 2>/dev/null |
    sed -n 's/.* T \(_RNvCs[A-Za-z0-9]*_7___\)rustc17rust_begin_unwind$/\1/p' | head -1)
[ -n "$hash" ] || { echo "build.sh: no __rustc names in libc.a" >&2; exit 1; }
perl -0777 -pe "s/\Q${hash}rustc\E/${hash}zrstc/g" \
    "$root/build/sysroot/usr/lib/libc.a" > "$sys/usr/lib/libc.a"
for c in zshim zshim-fixed; do
    ZORAYA_SYSROOT=$sys "$root/tools/zcc" -O2 -Wall -Werror -c \
        -o "$out/$c.o" "docs/research/rust-std/probes/$c.c"
done
cd docs/research/rust-std/probes
# today: the probe-local zshim.o only (the names libc.a lacks).
# fixed: also zshim-fixed.o, --wrap=syscall and --wrap=ioctl (what libposix would answer).
for v in today fixed; do
    flags="-C relocation-model=static -C link-self-contained=no -C target-feature=+crt-static"
    flags="$flags -C link-arg=$out/zshim.o"
    if [ $v = fixed ]; then
        flags="$flags -C link-arg=$out/zshim-fixed.o -C link-arg=-Wl,--wrap=ioctl -C link-arg=-Wl,--wrap=syscall"
    fi
    ZORAYA_SYSROOT=$sys CARGO_TARGET_DIR=$out/target-$v CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2} \
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS=$flags cargo build --release
    cp "$out/target-$v/aarch64-unknown-linux-musl/release/rprobe" "$out/rprobe-$v"
    echo "build.sh: $out/rprobe-$v"
done
