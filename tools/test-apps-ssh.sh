#!/bin/sh
# The ssh part of `make test-apps` (TASK-31 10g, ADR-0340, ADR-0341): two
# servers of the host (tools/test-sshd.sh), a disk of the apps' root with the
# client's throwaway key and a known_hosts that holds a wrong key for server A,
# the script tests/shell/apps-ssh.script, and then what the servers' logs say.
# The Makefile gives the environment: RUNNER, RUN_QEMU, HTTPD, KERNEL, NET_HOST,
# ROOT_FROM (the apps' root), TIMEOUT (seconds), BUILD, and ACCEL.
set -eu
: "${RUNNER:?}" "${RUN_QEMU:?}" "${HTTPD:?}" "${KERNEL:?}" "${NET_HOST:?}" "${ROOT_FROM:?}" "${TIMEOUT:?}" "${BUILD:?}"
ACCEL=${ACCEL:-auto}
template=tests/shell/apps-ssh.script
sshd_dir=$BUILD/apps-sshd
root=$BUILD/apps-ssh-root
disk=$BUILD/apps-ssh-disk.img
script=$BUILD/apps-ssh.script
fail() { echo "[apps-ssh] FAIL: $*"; exit 1; }

set +e
line=$(tools/test-sshd.sh start "$sshd_dir" $((TIMEOUT * 2 + 120)))
rc=$?
set -e
if [ "$rc" = 75 ]; then
	[ -z "${CI:-}" ] || fail "the host has no sshd on a CI runner"
	echo "[apps-ssh] skipped: the host has no sshd (CI installs openssh-server)"
	exit 0
fi
[ "$rc" = 0 ] || fail "the test servers did not start"
trap 'tools/test-sshd.sh stop "$sshd_dir"' EXIT
set -- $line
pa=$1 pb=$2 user=$3 fpa=$4 fpb=$5

rm -rf "$root" && mkdir -p "$root" target/test-logs
cp -R "$ROOT_FROM"/. "$root"/
mkdir -p "$root/data/home/ssh"
chmod 700 "$root/data/home/ssh"
cp "$sshd_dir/client" "$root/data/home/ssh/id_ed25519"
cp "$sshd_dir/client.pub" "$root/data/home/ssh/id_ed25519.pub"
chmod 600 "$root/data/home/ssh/id_ed25519"
# A stored key for server A's address that is not A's: a changed host key.
wrong=$(awk '{ print $1, $2 }' "$sshd_dir/wrong.pub")
echo "[$NET_HOST]:$pa $wrong" > "$root/data/home/ssh/known_hosts"
chmod 600 "$root/data/home/ssh/known_hosts"
target/debug/zoraya-mkfs --size 64M --from "$root" "$disk"
sed -e "s/@HOST@/$NET_HOST/g" -e "s/@PA@/$pa/g" -e "s/@PB@/$pb/g" -e "s/@USER@/$user/g" -e "s|@FPB@|$fpb|g" \
	"$template" > "$script"

set +e
$RUNNER --timeout "$TIMEOUT" --script "$script" --label apps-ssh -- \
	$RUN_QEMU --test --keys --net --net-guestfwd "$NET_HOST:$pa=$PWD/$HTTPD --relay 127.0.0.1:$pa" \
	--net-guestfwd "$NET_HOST:$pb=$PWD/$HTTPD --relay 127.0.0.1:$pb" --disk "$disk" --mem 512M --accel "$ACCEL" "$KERNEL"
rc=$?
set -e
[ "$rc" = 0 ] || exit "$rc"

# INV-0340-3: A's log shows the connection and no attempt to authenticate.
if grep -q -e 'Accepted' -e 'Failed' -e 'Invalid user' -e 'userauth' "$sshd_dir/sshd-a.log"; then
	cat "$sshd_dir/sshd-a.log"
	fail "INV-0340-3: server A saw an authentication attempt after a changed host key"
fi
grep -q 'Connection' "$sshd_dir/sshd-a.log" || fail "server A saw no connection: the changed-key run did not reach it"
# B took the key and ran the commands: the guest's words after the host were
# the far side's, and a login is a public-key login with the throwaway key.
grep -q "Accepted publickey for $user" "$sshd_dir/sshd-b.log" || { cat "$sshd_dir/sshd-b.log"; fail "server B saw no public-key login"; }
if grep -q -e 'Accepted password' -e 'Accepted keyboard-interactive' "$sshd_dir/sshd-b.log"; then fail "B accepted another method"; fi
echo "[apps-ssh] ok: server A saw no authentication after the changed key; server B took the client's key"
