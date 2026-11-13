# Sourced by tools/test-changed.sh and tools/ci-plan.sh: what a list of
# changed paths needs. Needs $changed (one path a line), $map
# (tools/test-map.txt) and $cargo. Sets
#   all        every step of make test that runs a machine
#   steps      the steps the map gives the paths (sorted, without `check`)
#   everything 1 if a path is not in the map (steps is then $all)
#   packages   the host-test packages: changed crates and their dependents
# and prints a line to stderr for each path the map does not know. A path the
# map does not know runs every step.

all="test-qemu test-boot test-libposix test-posix-threads test-files test-crash test-posix test-posix-gui test-tour-gui test-gui test-net test-keeper test-hand-run test-fuzz test-fuzz-net"

plan_changed() {
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
            echo "  $path: not in $map: every step" >&2
            everything=1
        else
            steps="$steps $found"
        fi
    done
    if [ "$everything" = 1 ]; then
        steps=$all
    fi
    steps=$(for s in $steps; do [ "$s" = check ] || echo "$s"; done | sort -u | tr '\n' ' ')

    # The changed crates' packages, and every workspace package that depends
    # on one of them (cargo tree -i), host-tested together.
    packages=
    for dir in $(echo "$crates" | tr ' ' '\n' | sort -u); do
        name=$(sed -n 's/^name *= *"\(.*\)"/\1/p' "$dir/Cargo.toml" | head -1)
        [ -n "$name" ] || continue
        deps=$("$cargo" tree -q --workspace -i "$name" -e normal,dev --prefix none --format '{p}' 2>/dev/null |
            awk '{ print $1 }' | sort -u)
        packages="$packages $name $deps"
    done
    packages=$(echo "$packages" | tr ' ' '\n' | grep -v -E '^$|^zoraya-kernel$|^zoraya-user$|^zoraya-rt$' | sort -u | tr '\n' ' ')
}
