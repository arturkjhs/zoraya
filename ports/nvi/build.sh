#!/bin/sh
# Builds nvi2 for Zoraya (TASK-31 ports-early; ADR-0335 item 1). nvi2's own
# build is CMake with Ninja, which no host tool of this repository is (ADR-0335
# item 1): this is its list of sources and its generated headers, compiled by
# zcc, linked with the ncurses archives and DB 1.
#
#   build.sh SRC OBJDIR OUT ZCC CURSES_PREFIX DB1_INCLUDE DB1_LIB CFLAGS TTYSHIM_DIR
#
# SRC is the unpacked, patched nvi2 tree, run from the repository's root.
set -eu
src=$1 obj=$2 out=$3 zcc=$4 curses=$5 dbinc=$6 dblib=$7 cflags=$8 ttydir=$9
here=$(pwd)/ports/nvi
mkdir -p "$obj"
cd "$src"

CL="cl/cl_funcs.c cl/cl_main.c cl/cl_read.c cl/cl_screen.c cl/cl_term.c"
COMMON="common/conv.c common/cut.c common/delete.c common/encoding.c common/exf.c common/key.c common/line.c common/log.c common/main.c common/mark.c common/msg.c common/options.c common/options_f.c common/put.c common/screen.c common/search.c common/seq.c common/util.c"
EX="ex/ex.c ex/ex_abbrev.c ex/ex_append.c ex/ex_args.c ex/ex_argv.c ex/ex_at.c ex/ex_bang.c ex/ex_cd.c ex/ex_cmd.c ex/ex_cscope.c ex/ex_delete.c ex/ex_display.c ex/ex_edit.c ex/ex_equal.c ex/ex_file.c ex/ex_filter.c ex/ex_global.c ex/ex_init.c ex/ex_join.c ex/ex_map.c ex/ex_mark.c ex/ex_mkexrc.c ex/ex_move.c ex/ex_open.c ex/ex_preserve.c ex/ex_print.c ex/ex_put.c ex/ex_quit.c ex/ex_read.c ex/ex_screen.c ex/ex_script.c ex/ex_set.c ex/ex_shell.c ex/ex_shift.c ex/ex_source.c ex/ex_stop.c ex/ex_subst.c ex/ex_tag.c ex/ex_txt.c ex/ex_undo.c ex/ex_usage.c ex/ex_util.c ex/ex_version.c ex/ex_visual.c ex/ex_write.c ex/ex_yank.c ex/ex_z.c"
VI="vi/getc.c vi/v_at.c vi/v_ch.c vi/v_cmd.c vi/v_delete.c vi/v_ex.c vi/v_increment.c vi/v_init.c vi/v_itxt.c vi/v_left.c vi/v_mark.c vi/v_match.c vi/v_paragraph.c vi/v_put.c vi/v_redraw.c vi/v_replace.c vi/v_right.c vi/v_screen.c vi/v_scroll.c vi/v_search.c vi/v_section.c vi/v_sentence.c vi/v_status.c vi/v_txt.c vi/v_ulcase.c vi/v_undo.c vi/v_util.c vi/v_word.c vi/v_xchar.c vi/v_yank.c vi/v_z.c vi/v_zexit.c vi/vi.c vi/vs_line.c vi/vs_msg.c vi/vs_refresh.c vi/vs_relative.c vi/vs_smap.c vi/vs_split.c"
REGEX="regex/regcomp.c regex/regerror.c regex/regexec.c regex/regfree.c"

# The generated headers, as CMakeLists.txt makes them.
protos() { sed -n 's/^ \* PUBLIC: \(.*\)/\1/p' "$@"; }
protos $CL > "$obj/cl_extern.h"
protos $COMMON "$here/recover.c" > "$obj/common_extern.h"
protos $EX > "$obj/ex_extern.h"
protos $VI > "$obj/vi_extern.h"
awk -f common/options.awk common/options.c > "$obj/options_def.h"
awk -f ex/ex.awk ex/ex_cmd.c > "$obj/ex_def.h"
sed -n 's/^.*version \([^)]*\)).*/#define VI_VERSION "\1"/p' README > "$obj/version.h"
cp "$here/config.h" "$here/pathnames.h" "$here/bitstring.h" "$obj/"; mkdir -p "$obj/sys"; cp "$here/sys/queue.h" "$obj/sys/"

# -D__REGEX_PRIVATE and regex/ in the include path: the bundled, wide-character
# regex library is used instead of musl's (CMakeLists.txt, USE_WIDECHAR).
DEFS="-include $here/compat.h -D_GNU_SOURCE -D_XOPEN_SOURCE_EXTENDED -D__REGEX_PRIVATE -Wno-error=int-conversion -Wno-error=incompatible-pointer-types"
INC="-I$obj -I$src/common -Iregex -I$curses/usr/include -I$dbinc -I."
n=0
for f in $CL $COMMON $EX $VI $REGEX "$here/compat.c" "$here/recover.c"; do
  o=$obj/$(echo "${f#$here/}" | tr '/' '_' | sed 's/\.c$/.o/')
  echo "$o"
  "$zcc" $cflags $DEFS $INC -c -o "$o" "$f" &
  n=$((n + 1)); [ $((n % 3)) = 0 ] && wait
done > "$obj/objects.txt"
wait
for o in $(cat "$obj/objects.txt"); do [ -f "$o" ] || { echo "nvi: $o was not made"; exit 1; }; done
"$zcc" $cflags -Wl,--error-limit=0 -o "$out" $(cat "$obj/objects.txt") -L"$dblib" -L"$curses/usr/lib" -L"$ttydir" -lncursesw -ltinfow -ldb1 -lttyshim
