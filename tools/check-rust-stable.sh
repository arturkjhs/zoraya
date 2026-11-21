#!/bin/sh
# INV-0450-1 (ADR-0450 item 2): no build file of a Rust POSIX program uses a
# nightly feature: no -Z option, no +nightly, no RUSTC_BOOTSTRAP, and
# rust-toolchain.toml's channel is a stable version. The files that build Rust
# POSIX programs are tools/zcargo and its helpers, ports/ (outside the patches
# of other projects' sources), the Makefiles and tests/rust-std/. tools/zcargo
# itself names what it refuses; those lines (they say "refusing" or call die)
# are not uses. The fuzz targets of the Makefile use the pinned nightly for the
# repository's own fuzzers (tools/run-fuzz.sh), not for a Rust POSIX program.
set -eu
cd "$(dirname "$0")/.."
bad=0
channel=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -1)
case "$channel" in
[0-9]*.[0-9]*.[0-9]*) ;;
*) echo "check-rust-stable: rust-toolchain.toml's channel is '$channel', not a stable version x.y.z"; bad=1 ;;
esac
files="tools/zcargo tools/zcargo-link tools/rust-musl-target.sh tools/test-zcargo.sh Makefile ports/Makefile rust-toolchain.toml"
files="$files $(find ports -type f ! -path 'ports/patches/*' ! -path 'ports/musl/zoraya/*' ! -name sources.lock | sort)"
files="$files $(find tests/rust-std -type f ! -name Cargo.lock | sort)"
# A use: RUSTC_BOOTSTRAP, a +nightly or beta toolchain argument, a nightly or
# beta channel or toolchain, a -Z option passed to cargo or rustc.
pattern='RUSTC_BOOTSTRAP|[+ =]nightly|\+beta|channel *= *"(nightly|beta)|RUSTUP_TOOLCHAIN=(nightly|beta)|(cargo|rustc|RUSTFLAGS[^ ]*) .*-Z[a-z]'
for f in $files; do
    [ -f "$f" ] || continue
    # Not comments, not lines that refuse or test the refusal.
    hits=$(grep -nE "$pattern" "$f" | grep -vE '^[0-9]+:[[:space:]]*#' | grep -vE 'refusing|die |refused|check-rust-stable|test-zcargo|nightly (run|and)|nightly,|the nightly|nightly\)|fuzz' || true)
    if [ -n "$hits" ]; then
        echo "check-rust-stable: $f:"
        echo "$hits" | sed 's/^/  /'
        bad=1
    fi
done
# zcargo itself refuses what it must (INV-0450-1's second half).
for need in 'RUSTC_BOOTSTRAP' '-Z*)' '+*)'; do
    grep -qF -e "$need" tools/zcargo || { echo "check-rust-stable: tools/zcargo does not refuse $need"; bad=1; }
done
[ "$bad" = 0 ] || exit 1
echo "check-rust-stable: ok (channel $channel; no nightly feature in the build files)"
