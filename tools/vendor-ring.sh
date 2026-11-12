#!/bin/sh
# Makes vendor/ring: ring 0.17.14 from the cargo registry cache, without its tests, with one change: `none` is added to ring's list of
# operating systems that have the Linux ABI, so that its pregenerated aarch64
# assembly (the "linux64" files, plain ELF) is built for aarch64-unknown-none.
# Without it the C objects build but the link fails on ring_core_*_bn_mul4x_mont
# and its kin (ADR-0289). Run it only to make the directory again.
set -eu
cd "$(dirname "$0")/.."
src=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/ring-0.17.14 | head -1)
[ -d "$src" ] || { echo "ring-0.17.14 is not in the cargo cache: cargo fetch first" >&2; exit 1; }
rm -rf vendor/ring
mkdir -p vendor/ring
for f in Cargo.toml Cargo.toml.orig LICENSE LICENSE-BoringSSL LICENSE-other-bits README.md build.rs; do
  cp "$src/$f" vendor/ring/
done
cp -R "$src/src" "$src/crypto" "$src/include" "$src/third_party" vendor/ring/
# All of pregenerated/: the host tests build ring too (macOS and Linux files).
cp -R "$src/pregenerated" vendor/ring/
# The only change to ring's own files.
perl -0pi -e 's/const LINUX_ABI: &\[&str\] = &\[\n/const LINUX_ABI: &[&str] = &[\n    "none", \/\/ Zoraya: aarch64-unknown-none (ADR-0289)\n/' vendor/ring/build.rs
grep -q 'Zoraya: aarch64-unknown-none' vendor/ring/build.rs
# And its warnings are upstream's, not ours: a path dependency would show them.
printf '\n[lints.rust]\nwarnings = "allow"\n' >> vendor/ring/Cargo.toml
# The manifest `make check` compares the directory with (INV-0289-2). It lists
# tracked files, so run `git add vendor/ring` before this last step if the
# directory changed: tools/check-vendor-ring.sh --write
