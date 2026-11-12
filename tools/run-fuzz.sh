#!/bin/sh
# Runs fuzz targets of fuzz/ for a bounded time each (ADR-0268 point 3's
# "fuzzing" and point 7's "net-fuzz", ADR-0298), without cargo-fuzz: the
# targets are built with the toolchain `nightly` and LLVM's coverage
# instrumentation, as cargo-fuzz does, and libFuzzer's own driver runs them.
# Several run at once. A finding (a crash, a timeout or an out-of-memory) fails
# the run and is kept in target/fuzz-run/TARGET/artifacts/; every target's
# output goes to target/test-logs/fuzz-TARGET.log (read it with tail or grep,
# never stream it). The corpus is target/fuzz-run/TARGET/corpus and is not
# committed.
#
#   tools/run-fuzz.sh SECONDS JOBS TARGET...
#
# Needs `rustup toolchain install nightly` and `cargo +nightly fetch --locked`
# in fuzz/ (both outside `make test`, as the pinned toolchain's install is):
# the build itself is offline.
set -eu
cd "$(dirname "$0")/.."
seconds=$1
jobs=$2
shift 2
[ $# -gt 0 ] || { echo "run-fuzz: no target" >&2; exit 2; }
cargo=${CARGO_BIN:-cargo}
host=$(rustc -vV | sed -n 's/^host: //p')
dir=$PWD/target/fuzz
logs=target/test-logs
mkdir -p "$logs"

flags="-Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 -Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table -Cllvm-args=-sanitizer-coverage-trace-compares --cfg fuzzing -Cdebug-assertions -Coverflow-checks -Cdebuginfo=0"
if ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
	echo "run-fuzz: no nightly toolchain (rustup toolchain install nightly)" >&2
	exit 1
fi
bins=
for t in "$@"; do bins="$bins --bin $t"; done
# --target keeps the flags off the build scripts and proc macros.
# shellcheck disable=SC2086
( cd fuzz && RUSTFLAGS="$flags" "$cargo" +nightly build --offline --locked --release --target "$host" $bins --target-dir "$dir" ) >"$logs/fuzz-build.log" 2>&1 ||
	{ echo "run-fuzz: the build failed:" >&2; tail -n 30 "$logs/fuzz-build.log" >&2; exit 1; }

run_one() { # target
	t=$1
	work=target/fuzz-run/$t
	mkdir -p "$work/corpus" "$work/artifacts"
	# MEMORY: libFuzzer's own limit, 2 GiB; the time is the run's.
	perl -e 'alarm shift; exec @ARGV' $((seconds + 120)) "$dir/$host/release/$t" \
		-max_total_time="$seconds" -timeout=10 -rss_limit_mb=2048 -max_len=4096 \
		-artifact_prefix="$work/artifacts/" "$work/corpus" >"$logs/fuzz-$t.log" 2>&1
}

pids=
running=0
failed=
for t in "$@"; do
	run_one "$t" &
	pids="$pids $!:$t"
	running=$((running + 1))
	if [ "$running" -ge "$jobs" ]; then
		wait "${pids##* }" 2>/dev/null || true
		running=$((running - 1))
	fi
done
for p in $pids; do
	wait "${p%%:*}" || failed="$failed ${p##*:}"
done
for t in "$@"; do
	runs=$(sed -n 's/^Done \([0-9]*\) runs in \([0-9]*\) second.*/\1 runs in \2 s/p' "$logs/fuzz-$t.log" | tail -1)
	cov=$(grep -a '^#[0-9]*.*cov: ' "$logs/fuzz-$t.log" | tail -1 | sed -n 's/.*cov: \([0-9]*\) ft: \([0-9]*\).*/cov \1 ft \2/p')
	echo "[fuzz] $t: ${runs:-no result} ${cov:+($cov)}"
done
if [ -n "$failed" ]; then
	echo "[fuzz] FAIL: a finding in:$failed (target/fuzz-run/TARGET/artifacts/, target/test-logs/fuzz-TARGET.log)" >&2
	exit 1
fi
echo "[fuzz] $# target(s), $seconds s each, no finding"
