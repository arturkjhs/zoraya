#!/bin/sh
# Tests of tools/zcargo (ADR-0452, INV-0452-1, INV-0450-1, INV-0450-2, INV-0454-1).
#
#   tools/test-zcargo.sh refusals        what zcargo refuses, each with its message; what it does
#                                        not refuse (offline: needs no sysroot; make check-zcargo)
#   tools/test-zcargo.sh build WORKDIR   a small crate built by zcargo: the link line is static,
#                                        non-PIE, eh-frame-hdr, the sysroot's libunwind and libc
#                                        and libzoraya-rust and no library of the toolchain's
#                                        self-contained directory; its program passes
#                                        zoraya-elf-check; a crate that issues a raw svc is
#                                        refused and leaves no program (needs the sysroot and the
#                                        archive; make test-rust-std)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
zcargo=$here/zcargo
fail() { echo "test-zcargo: FAIL: $*" >&2; exit 1; }

refused() {
    # refused WHY CMD...: the command exits 1 and zcargo says "refusing".
    why=$1; shift
    out=$("$@" 2>&1) && fail "$why: not refused"
    case "$out" in
    *"zcargo: refusing"*) ;;
    *) fail "$why: failed, but not by a refusal: $out" ;;
    esac
}

refusals() {
    cd "$root"
    refused "-Z argument" "$zcargo" build -Zbuild-std
    refused "-Z argument, separate" "$zcargo" build -Z build-std
    refused "+toolchain" "$zcargo" +nightly build
    refused "RUSTC_BOOTSTRAP" env RUSTC_BOOTSTRAP=1 "$zcargo" build
    refused "another --target" "$zcargo" build --target x86_64-unknown-linux-gnu
    refused "another --target=" "$zcargo" build --target=aarch64-unknown-none
    refused "a linker in --config" "$zcargo" build --config target.aarch64-unknown-linux-musl.linker=cc
    refused "rustflags in --config" "$zcargo" build --config build.rustflags=[]
    refused "link-self-contained=yes" env RUSTFLAGS="-C link-self-contained=yes" "$zcargo" build
    refused "link-self-contained, joined" env RUSTFLAGS="-Clink-self-contained=yes" "$zcargo" build
    refused "relocation-model=pic" env RUSTFLAGS="-C relocation-model=pic" "$zcargo" build
    refused "-crt-static" env RUSTFLAGS="-C target-feature=-crt-static" "$zcargo" build
    refused "panic=immediate-abort" env RUSTFLAGS="-C panic=immediate-abort" "$zcargo" build
    refused "a linker" env RUSTFLAGS="-C linker=cc" "$zcargo" build
    refused "-Z in the flags" env RUSTFLAGS="-Zunstable-options" "$zcargo" build
    refused "--sysroot in the flags" env RUSTFLAGS="--sysroot /tmp" "$zcargo" build
    refused "the encoded flags" env CARGO_ENCODED_RUSTFLAGS="$(printf -- '-C\037link-self-contained=yes')" "$zcargo" build
    refused "a --config file" "$zcargo" build --config my.toml
    refused "a rustc in --config" "$zcargo" build --config build.rustc-wrapper=x
    refused "--codegen" "$zcargo" build --codegen linker=cc
    refused "args after --" "$zcargo" rustc -- -C relocation-model=pic
    refused "--codegen in the flags" env RUSTFLAGS="--codegen link-self-contained=yes" "$zcargo" build
    refused "RUSTC_WRAPPER" env RUSTC_WRAPPER=/bin/echo "$zcargo" build
    refused "RUSTC" env RUSTC=/bin/echo "$zcargo" build
    # What it lets through: the refusals are not about these, so whatever stops
    # the command later (no sysroot, say) does not say "refusing".
    for flags in "-C panic=abort" "-C panic=unwind" "-C relocation-model=static" "-C link-self-contained=no" \
        "-C target-feature=+crt-static" "-C opt-level=3"; do
        out=$(RUSTFLAGS="$flags" "$zcargo" --version 2>&1) || true
        case "$out" in *"refusing"*) fail "refused $flags: $out" ;; esac
    done
    echo "test-zcargo: refusals ok"
}

build() {
    rm -rf "$1" && mkdir -p "$1"
    work=$(cd "$1" && pwd)
    mkdir -p "$work/hello/src" "$work/rawsvc/src"
    cat > "$work/hello/Cargo.toml" <<'T'
[package]
name = "hello"
version = "0.1.0"
edition = "2024"
[workspace]
T
    echo 'fn main() { println!("hello"); }' > "$work/hello/src/main.rs"
    cat > "$work/rawsvc/Cargo.toml" <<'T'
[package]
name = "rawsvc"
version = "0.1.0"
edition = "2024"
[workspace]
T
    cat > "$work/rawsvc/src/main.rs" <<'T'
fn main() {
    // A raw system call: Linux's number, reaching an unrelated Zoraya call.
    // SAFETY: this program is never run; the build refuses it.
    unsafe { core::arch::asm!("mov x8, 172", "svc #0", out("x0") _, out("x8") _) };
}
T
    (cd "$work/hello" && CARGO_TARGET_DIR="$work/target-hello" "$zcargo" -v build --release > "$work/hello.log" 2>&1) \
        || { tail -20 "$work/hello.log"; fail "the small crate did not build"; }
    exe=$work/target-hello/aarch64-unknown-linux-musl/release/hello
    [ -x "$exe" ] || fail "no program at $exe"
    link=$(ls "$work"/target-hello/aarch64-unknown-linux-musl/release/deps/hello-*.link | head -1)
    [ -f "$link" ] || fail "no link line kept"
    for want in -static -no-pie -Wl,--eh-frame-hdr -lunwind -lc -lzoraya-rust; do
        grep -qx -e "$want" "$link" || fail "the link line has no $want"
    done
    for not in -pie -static-pie -Wl,-pie; do
        ! grep -qx -e "$not" "$link" || fail "the link line has $not"
    done
    # INV-0454-1: nothing of the toolchain's own libc or unwinder.
    ! grep -q 'self-contained' "$link" || fail "the link line names the toolchain's self-contained directory"
    ! grep -q 'libc\.a' "$link" || fail "the link line names a libc.a"
    "$root/target/debug/zoraya-elf-check" --no-foreign-svc "$(ls "$work"/target-hello/aarch64-unknown-linux-musl/release/deps/hello-*.map | head -1)" "$exe" > /dev/null \
        || fail "zoraya-elf-check refuses the small crate's program"
    # INV-0450-2: a program with an svc of its own is refused, and not left behind.
    if (cd "$work/rawsvc" && CARGO_TARGET_DIR="$work/target-rawsvc" "$zcargo" build --release > "$work/rawsvc.log" 2>&1); then
        fail "the crate with a raw svc was built"
    fi
    grep -q 'svc instruction(s) outside libc.a' "$work/rawsvc.log" || { tail -20 "$work/rawsvc.log"; fail "the raw svc was refused for another reason"; }
    [ ! -e "$work/target-rawsvc/aarch64-unknown-linux-musl/release/rawsvc" ] || fail "the refused program was left in the target directory"
    echo "test-zcargo: build ok"
}

case "${1:-}" in
refusals) refusals ;;
build) build "${2:?work directory}" ;;
*) echo "usage: test-zcargo.sh refusals | build WORKDIR" >&2; exit 2 ;;
esac
