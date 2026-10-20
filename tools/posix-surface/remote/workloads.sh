# Sourced by dynamic.sh and strace.sh. Defines the measurement sets and workloads W1..W6.
# MODE=dynamic: LD_PRELOAD interposer, hits in $RAW/dynamic/<wl>.tsv
# MODE=strace : strace -f, trace in $RAW/strace/<wl>.txt
SETS=$W/sets
RUN=$W/run
LIB=$W/interposer/libpsurf.so

mkset() { # name make-binary
  d=$SETS/$1/bin; rm -rf "$SETS/$1"; mkdir -p "$d"
  ln -s "$PFX/bin/dash" "$d/sh"; ln -s "$PFX/bin/dash" "$d/dash"
  ln -s "$PFX/bin/$2" "$d/make"
  ln -s "$PFX/bin/tcc" "$d/tcc"; ln -s "$PFX/bin/tcc" "$d/cc"
  for a in $("$PFX/bin/toybox"); do [ -e "$d/$a" ] || ln -s "$PFX/bin/toybox" "$d/$a"; done
}
mkfallback() { # busybox applets that toybox does not provide; last on PATH, every use is reported
  d=$SETS/fallback/bin; rm -rf "$SETS/fallback"; mkdir -p "$d"
  have=" $("$PFX/bin/toybox" | tr '\n' ' ') "
  for a in $("$PFX/bin/busybox" --list); do
    case "$have" in *" $a "*) ;; *) ln -s "$PFX/bin/busybox" "$d/$a";; esac
  done
}
setup_sets() {
  mkset t1p pdpmake; mkset t1g gmake; mkfallback
  "$PFX/bin/toybox" | tr ' ' '\n' | grep . > "$RES/toybox-applets.txt"
  ls "$SETS/fallback/bin" > "$RES/fallback-applets.txt"
  record_overrides
}

# /bin as seen by a workload of a given set: measured binaries where the set has the name, original otherwise
ORIGBIN=/tmp/psurf-origbin
mknewbin() { # set -> prints dir
  nb=$W/newbin/$1; rm -rf "$nb"; mkdir -p "$nb" "$ORIGBIN"
  for f in /bin/*; do ln -s "$ORIGBIN/$(basename "$f")" "$nb/$(basename "$f")"; done
  if [ "$1" != t3 ]; then
    for f in "$SETS/$1/bin"/*; do ln -sfn "$f" "$nb/$(basename "$f")"; done
  fi
  echo "$nb"
}
record_overrides() { # which /bin/<name> map to which measured binary, for strace attribution
  : > "$RES/bin-overrides.tsv"
  for s in t1p t1g; do
    for f in "$SETS/$s/bin"/*; do printf '%s\t/bin/%s\t%s\n' "$s" "$(basename "$f")" "$(readlink -f "$f")"; done
  done >> "$RES/bin-overrides.tsv"
}
NS() { # set cmd... : run cmd inside the namespace for this set
  s=$1; shift
  nb=$(mknewbin "$s")
  /usr/bin/unshare --map-root-user --mount /bin/sh "$TOOLS/remote/ns_exec.sh" "$nb" "$ORIGBIN" "$(id -u)" "$(id -g)" "$@"
}

# run_wl <workload-id> <set> <dir> <cmd...>: run cmd in dir with a clean environment
run_wl() {
  id=$1 set=$2 dir=$3; shift 3
  P=$SETS/$set/bin:$SETS/fallback/bin
  [ "$set" = t3 ] && P=/usr/bin:/bin
  mkdir -p "$RAW/$MODE"
  out=$RAW/$MODE/$id.log
  log "$MODE $id ($set)"
  set +e
  harness=no; case $id in W5-*) harness=yes;; esac   # W5: the pty harness applies preload/strace to the shell only
  if [ "$MODE" = dynamic ]; then
    rm -rf /tmp/psurf-hits; mkdir -p /tmp/psurf-hits
    if [ $harness = yes ]; then pre=PSURF_PRELOAD; else pre=LD_PRELOAD; fi
    (cd "$dir" && NS "$set" /usr/bin/env -i PATH="$P" HOME="$dir" TERM=dumb SHELL="$SETS/t1g/bin/sh" $pre="$LIB" "$@") > "$out" 2>&1
    rc=$?
    cat /tmp/psurf-hits/* 2>/dev/null | sort -u > "$RAW/dynamic/$id.tsv"
  elif [ $harness = yes ]; then
    (cd "$dir" && NS "$set" /usr/bin/env -i PATH="$P" HOME="$dir" TERM=dumb SHELL="$SETS/t1g/bin/sh" "$@") > "$out" 2>&1
    rc=$?
  else
    (cd "$dir" && NS "$set" /usr/bin/env -i PATH="$P" HOME="$dir" TERM=dumb SHELL="$SETS/t1g/bin/sh" \
        /usr/bin/strace -f -qq -s 256 -o "$RAW/strace/$id.txt" "$@") > "$out" 2>&1
    rc=$?
  fi
  set -e
  echo "$id	$set	$rc" >> "$RES/$MODE-status.tsv"
  log "  exit $rc"
}

fresh_run() { rm -rf "$RUN/$1"; mkdir -p "$RUN/$1"; cp -Rp "$SRC/$2" "$RUN/$1/src"; chmod -R u+w "$RUN/$1/src"; }

# W1: tcc compiles itself (configure run by dash, build driven by GNU make; pdpmake attempted and recorded)
W1() {
  fresh_run W1 tcc; ( cd "$RUN/W1/src" && patch -s -p1 < "$TOOLS/patches/build/tcc-aarch64-wchar.patch" )
  run_wl W1-configure t1g "$RUN/W1/src" sh ./configure --cc=tcc --config-musl --prefix="$RUN/W1/inst"
  run_wl W1-make-pdpmake t1p "$RUN/W1/src" make
  run_wl W1-make t1g "$RUN/W1/src" make
  run_wl W1-check t1g "$RUN/W1/src" sh -c './tcc -v && printf "int main(){return 0;}\n" > t.c && ./tcc -B. -o t t.c && ./t && echo W1-OK'
}
# W2: Lua with its plain Makefile, tcc as compiler, tcc -ar as archiver; once with each make
W2() {
  for m in p g; do
    fresh_run W2$m lua
    run_wl W2-make-$m t1$m "$RUN/W2$m/src" make CC=tcc AR="tcc -ar rcu" RANLIB=true posix
    run_wl W2-check-$m t1$m "$RUN/W2$m/src" sh -c './src/lua -e "print(string.format(\"lua ok %d\", 6*7))"'
  done
}
# W3: SQLite amalgamation compiled and linked by tcc from a shell script
W3() {
  fresh_run W3 sqlite
  run_wl W3-build t1p "$RUN/W3/src" sh -c 'tcc -O2 -DSQLITE_THREADSAFE=0 -c sqlite3.c && tcc -c shell.c && tcc -o sqlite3 shell.o sqlite3.o -lm'
  run_wl W3-check t1p "$RUN/W3/src" sh -c 'echo "create table t(x); insert into t values(40),(2); select sum(x) from t;" | ./sqlite3'
}
# W4: GNU hello: ./configure && make with tcc, dash as CONFIG_SHELL; gcc baseline configure (unmeasured) for diffing
W4() {
  fresh_run W4 hello
  # first as-is (records whether a toolchain without a separate ld passes), then with LD=tcc to continue
  run_wl W4-configure-noLD t1g "$RUN/W4/src" sh ./configure CC=tcc CONFIG_SHELL="$SETS/t1g/bin/sh" --disable-nls
  cp "$RUN/W4/src/config.log" "$RAW/$MODE/W4-noLD-config.log" 2>/dev/null || true
  run_wl W4-configure-noAR t1g "$RUN/W4/src" sh ./configure CC=tcc LD=tcc CONFIG_SHELL="$SETS/t1g/bin/sh" --disable-nls
  run_wl W4-make-noAR t1g "$RUN/W4/src" make
  # T1/T2 sets have no `ar` (toybox has none; busybox defconfig has none): use tcc's built-in archiver
  run_wl W4-configure t1g "$RUN/W4/src" sh ./configure CC=tcc LD=tcc AR="tcc -ar" CONFIG_SHELL="$SETS/t1g/bin/sh" --disable-nls
  run_wl W4-make t1g "$RUN/W4/src" make
  run_wl W4-check t1g "$RUN/W4/src" ./hello
  fresh_run W4p hello
  run_wl W4p-configure t1p "$RUN/W4p/src" sh ./configure CC=tcc LD=tcc AR="tcc -ar" CONFIG_SHELL="$SETS/t1p/bin/sh" --disable-nls
  run_wl W4p-make t1p "$RUN/W4p/src" make
  if [ "$MODE" = dynamic ]; then   # baseline with Alpine's normal toolchain, not measured
    rm -rf "$RUN/W4base"; mkdir -p "$RUN/W4base"; cp -Rp "$SRC/hello" "$RUN/W4base/src"; chmod -R u+w "$RUN/W4base/src"
    (cd "$RUN/W4base/src" && ./configure CC=gcc --disable-nls > "$RAW/W4-baseline-configure.log" 2>&1) || true
    cp "$RUN/W4base/src/config.log" "$RAW/W4-baseline-config.log" 2>/dev/null || true
    cp "$RUN/W4/src/config.log" "$RAW/W4-tcc-config.log" 2>/dev/null || true
  fi
}
# W5: scripted interactive session on a pty for each shell
W5() {
  for sh in dash oksh mrsh busybox; do
    mkdir -p "$RUN/W5-$sh"
    run_wl W5-$sh t1g "$RUN/W5-$sh" /usr/bin/python3 -I "$TOOLS/remote/w5_session.py" "$sh" "$PFX/bin" "$MODE" "$RAW"
  done
}
# W6 (T3): clang -c one C file, link with lld
W6() {
  rm -rf "$RUN/W6"; mkdir -p "$RUN/W6"
  printf '#include <stdio.h>\nint main(void){puts("w6 ok");return 0;}\n' > "$RUN/W6/hello.c"
  run_wl W6-compile t3 "$RUN/W6" clang -O2 -c hello.c -o hello.o
  run_wl W6-link t3 "$RUN/W6" clang -fuse-ld=lld -o hello hello.o
  run_wl W6-check t3 "$RUN/W6" ./hello
}
WORKLOADS=${PSURF_WORKLOADS:-W1 W2 W3 W4 W5 W6}
