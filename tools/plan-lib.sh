# Sourced by tools/test-changed.sh and tools/ci-plan.sh: what a list of
# changed paths needs. Needs $changed (one path a line), $map
# (tools/test-map.txt) and $cargo. Sets
#   all        every step of make test that runs a machine
#   steps      the steps the map gives the paths (sorted, without `check`)
#   everything 1 if a path is not in the map (steps is then $all)
#   packages   the host-test packages: changed crates and their dependents
# and prints a line to stderr for each path the map does not know. A path the
# map does not know runs every step.

all="test-qemu test-boot test-libposix test-posix-threads test-posix-suites test-posix-threadtest3 test-files test-crash test-posix test-posix-gui test-tour-gui test-gui test-net test-keeper test-hand-run test-apps test-python test-fuzz test-fuzz-net"

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

# Docs-only pushes to main (CLAUDE.md, "Work discipline"). Sourced by
# tools/ci-plan.sh. `docs_only BEFORE AFTER` succeeds if every path changed
# between the two commits is under docs/ and no build input of libc.a changed:
# docs/adr/posix-surface-*.txt (the tier lists) or a ```posix-surface-added
# block of an ADR (ports/Makefile's ADDED_ADRS). It fails, with the reason on
# stderr, for anything else, so that the caller runs the full set: a missing,
# all-zero or unknown BEFORE (first push), a BEFORE that is not an ancestor of
# AFTER (force push) and an empty change. A merge commit is the diff against
# BEFORE, which on main is its first parent.
# Files under docs/ that tests or builds read count as code: docs/tour.md (the
# source of the tour and conventions scripts), docs/dependencies.md
# (zoraya-fonts' test), docs/adr/0211-* (posix-stats' test),
# docs/research/task-02/ (posix-surface-check, ports/Makefile) and
# docs/research/desktop/mockups/ (zoraya-tokens, the mockups tool).
# The document checks a docs-only run keeps (the CI `checks` job's commands
# that read docs/; the others read code only):
doc_checks="make adr-check|make check-host-paths|make check-untrusted-gate|make check-quota-tests|make check-tls-roots"

surface_blocks() { # REV PATH: the posix-surface-added blocks of PATH at REV
    git show "$1:$2" 2>/dev/null | awk '
        /^```posix-surface-added[[:space:]]*$/ { inb = 1; next }
        /^```/ { inb = 0; next }
        inb { print }'
}

docs_only() {
    d_before=$1
    d_after=$2
    case "$d_before" in
        '' | *[!0-9a-f]* | 0000000*) echo "ci-plan: no before commit: full set" >&2; return 1 ;;
    esac
    if ! git rev-parse -q --verify "$d_before^{commit}" >/dev/null 2>&1 ||
        ! git rev-parse -q --verify "$d_after^{commit}" >/dev/null 2>&1; then
        echo "ci-plan: before or after commit unknown: full set" >&2
        return 1
    fi
    if ! git merge-base --is-ancestor "$d_before" "$d_after" 2>/dev/null; then
        echo "ci-plan: $d_before is not an ancestor (force push?): full set" >&2
        return 1
    fi
    d_paths=$(git diff --name-only "$d_before" "$d_after" | sort -u)
    if [ -z "$d_paths" ]; then
        echo "ci-plan: no changed paths: full set" >&2
        return 1
    fi
    for p in $d_paths; do
        case "$p" in
            docs/adr/posix-surface-*.txt)
                echo "ci-plan: $p is a build input of libc.a: full set" >&2
                return 1
                ;;
            docs/tour.md | docs/dependencies.md | docs/adr/0211-* | docs/research/task-02/* | docs/research/desktop/mockups/*)
                # Read by a test or a build (include_str!, a path in a tool):
                # tour.md is the source of the tour scripts.
                echo "ci-plan: $p is read by a test or a build: full set" >&2
                return 1
                ;;
            docs/adr/*.md)
                if [ "$(surface_blocks "$d_before" "$p")" != "$(surface_blocks "$d_after" "$p")" ]; then
                    echo "ci-plan: $p changes a posix-surface-added block: full set" >&2
                    return 1
                fi
                ;;
            docs/*) ;;
            *)
                echo "ci-plan: $p is not under docs/: full set" >&2
                return 1
                ;;
        esac
    done
    echo "ci-plan: $(echo "$d_paths" | grep -c .) changed paths, all under docs/: document checks only" >&2
    return 0
}
