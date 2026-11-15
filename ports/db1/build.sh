#!/bin/sh
# Builds libdb1.a, Berkeley DB 1.85, for Zoraya (TASK-31 ports-early; nvi's
# buffer, ADR-0335 item 2). The archive's own PORT/ makefiles are for other
# systems; this is the object list of PORT/linux/Makefile (the hash, btree and
# recno access methods, the memory pool and dbopen) compiled by zcc with the
# headers of PORT/include.
#
#   build.sh SRC OBJDIR PREFIX ZCC AR CFLAGS
#
# PREFIX gets include/db.h (with mpool.h, ndbm.h, compat.h and sys/) and lib/libdb1.a.
set -eu
src=$1 obj=$2 prefix=$3 zcc=$4 ar=$5 cflags=$6
mkdir -p "$obj" "$prefix/include/sys" "$prefix/lib"
cp "$src"/PORT/include/*.h "$prefix/include/"
cp "$src/PORT/include/cdefs.h" "$src/PORT/include/queue.h" "$prefix/include/sys/"
cd "$src"
files="hash/hash.c hash/hash_bigkey.c hash/hash_buf.c hash/hash_func.c hash/hash_log2.c hash/hash_page.c hash/hsearch.c hash/ndbm.c
 btree/bt_close.c btree/bt_conv.c btree/bt_delete.c btree/bt_get.c btree/bt_open.c btree/bt_overflow.c btree/bt_page.c btree/bt_put.c
 btree/bt_search.c btree/bt_seq.c btree/bt_split.c btree/bt_utils.c db/db.c mpool/mpool.c
 recno/rec_close.c recno/rec_delete.c recno/rec_get.c recno/rec_open.c recno/rec_put.c recno/rec_search.c recno/rec_seq.c recno/rec_utils.c"
objs=""
for f in $files; do
  o=$obj/$(basename "$f" .c).o
  # -w: this is 1994 code; -Wno-error=int-conversion: btree/bt_seq.c returns NULL from an int function (the value 0 it always had).
  "$zcc" $cflags -w -Wno-error=int-conversion -D__DBINTERFACE_PRIVATE -D_GNU_SOURCE -I"$prefix/include" -I"$(dirname "$f")" -c -o "$o" "$f"
  objs="$objs $o"
done
rm -f "$prefix/lib/libdb1.a"
"$ar" rcs "$prefix/lib/libdb1.a" $objs
