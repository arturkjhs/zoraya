#!/bin/sh
# INV-0280-1: the NIC driver reads no byte of any frame slot. The frames
# object is the client's; the driver only pins it for the device. This checks
# the three parts of that statement on the source:
#
#  1. `zoraya-virtio-net` has `#![forbid(unsafe_code)]`, has exactly one
#     `Shared` view (the type parameter `S` of `Net`, which `ethd` binds to
#     its queue memory), and implements `Shared` for nothing itself.
#  2. `user/src/bin/ethd.rs` calls `memory_map(` exactly once, on the queue
#     object it created (`queues`), and implements `Shared` exactly once, for
#     `Queues`, built from that mapping alone. The handle that comes with
#     `Attach` is only pinned (`Host::pin`) and closed.
#  3. `ethd.rs` opens its device handle exactly once, through `Window::open`.
#
# A source check, not a run: it fails on the edit that would give the driver
# a view of a frame slot, which no test in QEMU would notice.
set -eu
cd "$(dirname "$0")/.."

fail=0
bad() { echo "check-ethd-maps: $*" >&2; fail=1; }

net=crates/zoraya-virtio-net/src
ethd=user/src/bin/ethd.rs
for f in "$net/lib.rs" "$net/serve.rs" "$ethd"; do
	[ -r "$f" ] || { echo "check-ethd-maps: no $f" >&2; exit 1; }
done

grep -q '^#!\[forbid(unsafe_code)\]' "$net/lib.rs" || bad "$net/lib.rs lost #![forbid(unsafe_code)]"

# No Shared implementation in the crate's code (the test module is in tests.rs).
if grep -nE 'impl.*\bShared\b +for' "$net/lib.rs" "$net/serve.rs" >&2; then
	bad "zoraya-virtio-net implements Shared itself"
fi
grep -qE '^pub struct Net<R, S> \{' "$net/lib.rs" || bad "Net no longer has exactly the parameters <R, S>"
grep -qE '^pub struct Server<R, S, H> \{' "$net/serve.rs" || bad "Server no longer has exactly the parameters <R, S, H>"
count=$(grep -c ': Shared' "$net/lib.rs" "$net/serve.rs" | awk -F: '{ n += $2 } END { print n }')
# One bound on the impls of Net and one on the impls of Server.
[ "$count" -le 2 ] || bad "$count places bind a Shared view; Net and Server take one parameter S each"

maps=$(grep -c 'memory_map(' "$ethd" || true)
[ "$maps" -eq 1 ] || bad "$ethd calls memory_map( $maps times; exactly one (the queue object) is allowed"
grep -qE 'memory_map\(queues,' "$ethd" || bad "$ethd maps something other than 'queues'"
# The device registers are opened once, through `Window::open` (user/src/mmio.rs: a
# mapping, or register calls on a transport device, ADR-0295); `ethd` maps no
# device itself.
devs=$(grep -c 'Window::open(' "$ethd" || true)
[ "$devs" -eq 1 ] || bad "$ethd calls Window::open( $devs times; exactly one is allowed"
grep -qE 'Window::open\(device,' "$ethd" || bad "$ethd opens a device handle other than 'device'"
! grep -q 'device_map(' "$ethd" || bad "$ethd calls device_map( itself; the device goes through Window::open"
impls=$(grep -cE 'impl Shared for' "$ethd" || true)
[ "$impls" -eq 1 ] || bad "$ethd implements Shared $impls times; exactly one (Queues) is allowed"
grep -qE 'impl Shared for Queues' "$ethd" || bad "$ethd implements Shared for something other than Queues"
built=$(grep -cE 'Queues\(' "$ethd" || true)
# The definition `struct Queues(u64);` and the one construction `Queues(at)`.
[ "$built" -eq 2 ] && grep -q 'Queues(at)' "$ethd" || bad "Queues is built from something other than the queue mapping 'at'"
grep -qE 'let at = memory_map\(queues,' "$ethd" || bad "'at' is not the mapping of 'queues'"

[ "$fail" -eq 0 ] || exit 1
echo "check-ethd-maps: ok"
