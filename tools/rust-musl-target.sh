#!/bin/sh
# Puts the toolchain's prebuilt std for aarch64-unknown-linux-musl (ADR-0450)
# in build/rust-musl-sysroot, for a machine whose toolchain lacks the target
# and where nothing may be installed outside build/ (`rustup target add` is the
# usual way, and what rust-toolchain.toml and CI's image do). The component is
# the one the toolchain's own channel manifest names, fetched from the URL it
# gives and checked against the sha256 it gives; tools/zcargo then passes
# --sysroot. Usage (from anywhere): tools/rust-musl-target.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
target=aarch64-unknown-linux-musl
channel=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' "$root/rust-toolchain.toml" | head -1)
export RUSTUP_TOOLCHAIN=$channel
sysroot=$(rustc --print sysroot)
manifest=$sysroot/lib/rustlib/multirust-channel-manifest.toml
[ -f "$manifest" ] || { echo "rust-musl-target: no $manifest" >&2; exit 1; }
field() {
    awk -v want="[pkg.rust-std.target.$target]" -v key="$1" '
        $0 == want { on = 1; next }
        /^\[/ { on = 0 }
        on && $1 == key { gsub(/"/, "", $3); print $3; exit }' "$manifest"
}
url=$(field url)
hash=$(field hash)
[ -n "$url" ] && [ -n "$hash" ] || { echo "rust-musl-target: the manifest has no std for $target" >&2; exit 1; }
out=$root/build/rust-musl-sysroot
if ls "$out/lib/rustlib/$target/lib"/libstd-*.rlib >/dev/null 2>&1; then
    echo "rust-musl-target: $out is there"
    exit 0
fi
dl=$root/build/rust-musl-dl
rm -rf "$dl" && mkdir -p "$dl"
curl -sSf --max-time 900 -o "$dl/std.tar.gz" "$url"
got=$(shasum -a 256 "$dl/std.tar.gz" | awk '{print $1}')
[ "$got" = "$hash" ] || { echo "rust-musl-target: $url has sha256 $got, the manifest says $hash" >&2; exit 1; }
mkdir -p "$dl/x"
tar xzf "$dl/std.tar.gz" -C "$dl/x"
rm -rf "$out" && mkdir -p "$out/lib"
cp -R "$dl"/x/rust-std-*/rust-std-$target/lib/rustlib "$out/lib/"
rm -rf "$dl"
echo "rust-musl-target: $out holds std for $target (sha256 $hash)"
