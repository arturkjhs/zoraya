#!/bin/sh
# Stage: tight static upper bound for multi-call binaries. From the dynamic run, take the toybox and busybox
# applets each tier's core runs actually executed, and build toybox/busybox containing only those applets
# (toybox-T0, toybox-T1, toybox-T2, busybox-T0, ...). Their import tables are the static upper bound for
# "exactly the utilities this tier needs", next to the full-binary bound. Not used to run workloads.
. "$(dirname "$0")/common.sh"
export CC=gcc CFLAGS="-O2 -g -U_FORTIFY_SOURCE"
J=$(nproc)
python3 -I - "$W" "$TOOLS/analysis" > "$RES/applets-by-tier.tsv" <<'EOF'
import sys; sys.path.insert(0, sys.argv[2])
import runs
per = runs.applets_by_tier(sys.argv[1])
for t, d in per.items():
    for box, apps in d.items():
        print(f"{t}\t{box}\t{' '.join(sorted(apps))}")
EOF
cat "$RES/applets-by-tier.tsv"
rm -rf "$PFX/min"; mkdir -p "$PFX/min"; : > "$RES/minbuild-missing.tsv"
while IFS='	' read -r tier box apps; do
  [ -n "$apps" ] || { log "$box-$tier: no applets"; continue; }
  d=$BLD/$box-$tier; rm -rf "$d"; cp -Rp "$SRC/$box" "$d"; chmod -R u+w "$d"; cd "$d"
  if [ "$box" = toybox ]; then
    make -s allnoconfig >/dev/null
    for a in $apps; do
      case $a in '['|'[[') u=TEST;; *) u=$(echo "$a" | tr 'a-z-' 'A-Z_');; esac
      sed -i "s/^# CONFIG_$u is not set/CONFIG_$u=y/" .config
    done
    yes '' | make -s oldconfig >/dev/null 2>&1 || true
    make -s -j$J >/dev/null 2>&1
    got=$(./toybox | tr ' ' '\n' | grep . | sort | tr '\n' ' ')
  else
    make -s allnoconfig >/dev/null
    for a in $apps; do
      case $a in '['|'[[') u=TEST;; *) u=$(echo "$a" | tr 'a-z-' 'A-Z_');; esac
      sed -i "s/^# CONFIG_$u is not set/CONFIG_$u=y/" .config
    done
    yes '' | make -s oldconfig >/dev/null 2>&1 || true
    make -s -j$J >/dev/null 2>&1
    # `busybox --list` needs the "busybox" applet itself, which would add code; read the config instead
    got=""
    for a in $apps; do
      case $a in '['|'[[') u=TEST;; *) u=$(echo "$a" | tr 'a-z-' 'A-Z_');; esac
      grep -q "^CONFIG_$u=y" .config && got="$got $a"
    done
  fi
  install -m755 "$box" "$PFX/min/$box-$tier"
  for a in $apps; do
    case " $got " in *" $a "*) ;; *) log "WARNING $box-$tier lacks applet $a"; echo "$tier	$box	$a" >> "$RES/minbuild-missing.tsv";; esac
  done
  log "$box-$tier: requested [$apps] built [$got]"
done < "$RES/applets-by-tier.tsv"
python3 -I "$TOOLS/analysis/static.py" "$RES"
