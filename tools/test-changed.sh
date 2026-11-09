#!/bin/sh
# What a change needs (make test-changed): the paths changed since
# origin/main (committed, staged, unstaged and untracked), each mapped by
# tools/test-map.txt to the steps of make test that cover it; make check;
# the host tests of the changed crates and of the crates that depend on
# them; and those steps, in parallel lanes under HVF (tools/test-par.sh with
# FULL=0). A path the map does not know runs every step.
#
#   tools/test-changed.sh [BASE]     (BASE: origin/main)
#
# DRY=1 prints what would run and runs nothing.

set -u

base=${1:-origin/main}
map=tools/test-map.txt
cargo=${CARGO:-cargo}

changed=$(
    {
        git diff --name-only "$(git merge-base "$base" HEAD)" HEAD
        git diff --name-only HEAD
        git ls-files --others --exclude-standard
    } | sort -u
)
if [ -z "$changed" ]; then
    echo "make test-changed: nothing changed since $base; make check only"
    exec "${MAKE:-make}" --no-print-directory check
fi

all="test-qemu test-boot test-libposix test-files test-crash test-posix test-posix-gui test-tour-gui test-gui"
steps=
everything=0
crates=
for path in $changed; do
    # The longest prefix in the map.
    found=$(awk -v p="$path" '
        /^#/ || NF < 2 { next }
        index(p, $1) == 1 && length($1) > best { best = length($1); line = $0 }
        END { if (best) { n = split(line, w, " "); s = ""; for (i = 2; i <= n; i++) s = s " " w[i]; print s } }
    ' "$map")
    case "$path" in
        crates/*/* | tools/*/*)
            dir=$(echo "$path" | cut -d/ -f1-2)
            [ -f "$dir/Cargo.toml" ] && crates="$crates $dir"
            ;;
    esac
    if [ -z "$found" ]; then
        echo "  $path: not in $map: every step"
        everything=1
    else
        steps="$steps $found"
    fi
done
if [ "$everything" = 1 ]; then
    steps=$all
fi
steps=$(for s in $steps; do [ "$s" = check ] || echo "$s"; done | sort -u | tr '\n' ' ')

# The changed crates' packages, and every workspace package that depends on
# one of them (cargo tree -i), host-tested together.
packages=
for dir in $(echo "$crates" | tr ' ' '\n' | sort -u); do
    name=$(sed -n 's/^name *= *"\(.*\)"/\1/p' "$dir/Cargo.toml" | head -1)
    [ -n "$name" ] || continue
    deps=$("$cargo" tree -q --workspace -i "$name" -e normal,dev --prefix none --format '{p}' 2>/dev/null |
        awk '{ print $1 }' | sort -u)
    packages="$packages $name $deps"
done
packages=$(echo "$packages" | tr ' ' '\n' | grep -v -E '^$|^zoraya-kernel$|^zoraya-user$|^zoraya-rt$' | sort -u | tr '\n' ' ')

echo "make test-changed: since $base"
echo "  host tests: ${packages:-none}"
echo "  steps: ${steps:-none} (under HVF)"
[ "${DRY:-0}" = 1 ] && exit 0

status=0
"${MAKE:-make}" --no-print-directory check || status=1
if [ -n "$packages" ]; then
    args=
    for p in $packages; do args="$args -p $p"; done
    log=build/logs-par/host-tests.log
    mkdir -p build/logs-par
    t0=$(date +%s)
    # shellcheck disable=SC2086 # the -p words
    if tools/mutants/watchdog.sh "$cargo" test -q $args >"$log" 2>&1; then
        echo "  ok    host tests $(($(date +%s) - t0)) s: $(grep -c 'test result: ok' "$log") test binaries"
    else
        echo "  FAIL  host tests: $log"
        grep -E -m 6 'panicked|FAILED|^error' "$log" | sed 's/^/          /'
        status=1
    fi
fi
if [ -n "$steps" ]; then
    # shellcheck disable=SC2086 # the steps
    FULL=0 tools/test-par.sh $steps || status=1
fi
exit "$status"
