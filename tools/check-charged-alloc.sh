#!/bin/sh
# INV-0261-5 (and INV-0260-2): kernel system call code reaches the heap only
# through the charging forms of kernel/src/charge.rs.
#
#  1. `uncharged_arc(` and `uncharged_box(` appear only in memory/heap.rs and
#     charge.rs, and where a line (or the one above it) says
#     `// uncharged: REASON`, which is for objects with no paying job: boot
#     code, devices, kernel threads, the root job.
#  2. The files a system call reaches allocate no `Arc` or `Box` directly:
#     no `Arc::new(`, `Box::new(`, `Arc::from(` or `Box::from(`.
set -eu
cd "$(dirname "$0")/.."
fail=0

hits=$(grep -rn -E '\buncharged_(arc|box)\(' kernel/src --include='*.rs' |
	grep -v -E '^kernel/src/(memory/heap|charge|testing)\.rs:' || true)
echo "$hits" | while IFS= read -r hit; do
	[ -n "$hit" ] || continue
	file=${hit%%:*}; rest=${hit#*:}; line=${rest%%:*}
	same=$(sed -n "${line}p" "$file")
	above=$(sed -n "$((line - 1))p" "$file")
	case "$same$above" in
	*"// uncharged:"*) ;;
	*) echo "check-charged-alloc: $file:$line calls an uncharged form without '// uncharged: REASON'" >&2; exit 1 ;;
	esac
done || fail=1

for f in syscall channel stream port object process job sched thread memory/object memory/user; do
	found=$(grep -n -E '\b(Arc|Box)::(new|from)\(' "kernel/src/$f.rs" | grep -v -E '^[0-9]+:[[:space:]]*//' || true)
	# A line with `// uncharged:` above it is a boot or kernel object.
	found=$(echo "$found" | while IFS= read -r l; do
		[ -n "$l" ] || continue
		n=${l%%:*}
		prev=$(sed -n "$((n - 1))p" "kernel/src/$f.rs")
		if ! echo "$prev" | grep -q '// uncharged:'; then echo "$l"; fi
	done)
	if [ -n "$found" ]; then
		echo "$found" >&2
		echo "check-charged-alloc: kernel/src/$f.rs allocates an Arc or Box directly" >&2
		fail=1
	fi
done

[ "$fail" -eq 0 ] || exit 1
echo "check-charged-alloc: ok"
