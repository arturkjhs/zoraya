#!/bin/sh
# Stage (TASK-02b): parallel builds (W2-j, W3-j), recursive make with a shared jobserver (W8), shell-level
# parallelism (W10) and interrupted parallel builds (W9), in both measurement modes, into raw-par/.
# Reuses the TASK-02 sets, namespace and run_wl; TASK-02 raw data and results are not touched.
# analysis/parallel.py derives results/par/ and the docs/research/task-02b tables.
. "$(dirname "$0")/common.sh"
RAW=$W/raw-par; rm -rf "$RAW"; mkdir -p "$RAW"
RES=$RES/par; rm -rf "$RES"; mkdir -p "$RES"
. "$TOOLS/remote/workloads.sh"
setup_sets
WL=$TOOLS/workloads
NCPU=$(nproc)
JOBS="2 4"; case " $JOBS " in *" $NCPU "*) ;; *) JOBS="$JOBS $NCPU";; esac   # -jN, N = VM cores
STYLES="pipe fifo"                      # GNU make 4.4.1 --jobserver-style
REPS=${PSURF_PAR_REPS:-2}               # repetitions of the non-interrupt workloads (run-to-run variation)
W9_DELAYS=${PSURF_W9_DELAYS:-"0.3 1.5 3.0"}   # W8 -j4 takes about 4 s: early, middle, late
W9_REPS=${PSURF_W9_REPS:-3}
STRACE_OPTS="-f -q -ttt -v -s 256"      # -q keeps exit lines, -ttt/-v for timelines and jobserver MAKEFLAGS
printf 'ncpu\t%s\njobs\t%s\nstyles\t%s\nreps\t%s\nw9_delays\t%s\nw9_reps\t%s\n' \
  "$NCPU" "$JOBS" "$STYLES" "$REPS" "$W9_DELAYS" "$W9_REPS" > "$RES/params.tsv"

tree() { # id kind -> fresh, writable build tree in $RUN/id/src (src/ trees are read-only)
  [ -d "$RUN/$1" ] && chmod -R u+w "$RUN/$1"
  rm -rf "$RUN/$1"; mkdir -p "$RUN/$1"
  case $2 in
    lua)    cp -Rp "$SRC/lua" "$RUN/$1/src";;
    sqlite) cp -Rp "$SRC/sqlite" "$RUN/$1/src";;
    w8)     cp -Rp "$WL/w8" "$RUN/$1/src";;
    w10)    mkdir -p "$RUN/$1/src"; cp "$WL/w10/w10.sh" "$WL/w8/mktab.c" "$RUN/$1/src/";;
  esac
  chmod -R u+w "$RUN/$1/src"
  [ "$2" = sqlite ] && cp "$WL/w3/Makefile" "$RUN/$1/src/Makefile"
  return 0
}
W8CHECK='test "$(ls */*/libleaf.a | wc -l)" -eq 4 && for f in */*/t?.h; do tail -n 1 "$f" | grep -q "0 };" || exit 1; done && echo W8-OK'

# build <W> <id-prefix> <set> <make args...>: one build plus its check
build() {
  w=$1 id=$2 set=$3; shift 3
  case $w in W2) k=lua;; W3) k=sqlite;; W8) k=w8;; esac
  tree "$id" $k; d=$RUN/$id/src
  case $w in
    W2) run_wl "$id" "$set" "$d" make "$@" CC=tcc AR="tcc -ar rcu" RANLIB=true posix
        run_wl "$id-check" "$set" "$d" sh -c './src/lua -e "print(string.format(\"lua ok %d\", 6*7))"';;
    W3) run_wl "$id" "$set" "$d" make "$@"
        run_wl "$id-check" "$set" "$d" sh -c 'echo "create table t(x); insert into t values(40),(2); select sum(x) from t;" | ./sqlite3';;
    W8) run_wl "$id" "$set" "$d" make "$@"
        run_wl "$id-check" "$set" "$d" sh -c "$W8CHECK";;
  esac
}

Wpar() { # W2-j, W3-j, W8
  for r in $(seq 1 "$REPS"); do
    for w in W2 W3 W8; do
      [ $w = W2 ] || { build $w ${w}j-g-j1-r$r t1g -j1; build $w ${w}j-p-j1-r$r t1p -j1; }   # W2 serial: TASK-02 W2-make-*
      for j in $JOBS; do
        for st in $STYLES; do build $w ${w}j-g-$st-j$j-r$r t1g -j$j --jobserver-style=$st; done
        build $w ${w}j-p-j$j-r$r t1p -j$j           # pdpmake: -j accepted, documented as not implemented
      done
    done
  done
}
W10() {
  for r in $(seq 1 "$REPS"); do
    for v in serial parallel; do
      tree W10-$v-r$r w10
      run_wl W10-$v-r$r t1g "$RUN/W10-$v-r$r/src" sh w10.sh $v
    done
  done
}
W9() {
  for c in int term; do
    for dl in $W9_DELAYS; do
      for r in $(seq 1 "$W9_REPS"); do
        id=W9-$c-d$dl-r$r; tree "$id" w8
        run_wl "$id" t1g "$RUN/$id/src" /usr/bin/python3 -I "$TOOLS/remote/w9_session.py" \
          $c "$dl" "$SETS/t1g/bin" "$MODE" "$RAW" "$id" "$RUN/$id/src" 4
      done
    done
  done
}

for MODE in dynamic strace; do
  mkdir -p "$RAW/$MODE"; : > "$RES/$MODE-status.tsv"
  for w in ${PSURF_PAR:-Wpar W10 W9}; do $w; done
done
python3 -I "$TOOLS/analysis/callsites_par.py" "$W"
python3 -I "$TOOLS/analysis/parallel.py" "$W"
