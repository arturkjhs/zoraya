#!/bin/sh
# The git part of `make test-apps` (TASK-31 10h, ADR-0339, ADR-0461): a bare repository on the host, served over
# smart HTTP by the host's `git http-backend` through the test server's TLS listeners (tools/test-httpd,
# tests/net/tls-git: a valid, an expired and a wrong-name certificate), a disk of the apps' root, the script
# tests/shell/apps-git.script, and then what the host sees: the pushed commit, and what the server logged.
# The Makefile gives the environment: RUNNER, RUN_QEMU, HTTPD, KERNEL, NET_HOST, ROOT_FROM (the apps' test
# root, with the test roots in /system/tls/roots and `net.hosts`), TIMEOUT (seconds), BUILD, and ACCEL.
set -eu
: "${RUNNER:?}" "${RUN_QEMU:?}" "${HTTPD:?}" "${KERNEL:?}" "${NET_HOST:?}" "${ROOT_FROM:?}" "${TIMEOUT:?}" "${BUILD:?}"
ACCEL=${ACCEL:-auto}
template=tests/shell/apps-git.script
dir=$PWD/$BUILD/apps-git
root=$BUILD/apps-git-root
root_hooks=$BUILD/apps-git-hooks-root
disk_hooks=$BUILD/apps-git-hooks-disk.img
disk=$BUILD/apps-git-disk.img
script=$BUILD/apps-git.script
log=target/test-logs/test-httpd-git.log
fail() { echo "[apps-git] FAIL: $*"; exit 1; }

# The host's git, and its smart-HTTP backend, serve the repository; without them the step is skipped (CI's image has both).
if ! command -v git >/dev/null 2>&1 || [ ! -x "$(git --exec-path)/git-http-backend" ]; then
	[ -z "${CI:-}" ] || fail "the host has no git http-backend on a CI runner"
	echo "[apps-git] skipped: the host has no git http-backend"
	exit 0
fi

rm -rf "$dir" "$root" && mkdir -p "$dir" target/test-logs
# The repository the guest clones: one commit on `main`. A second, empty-of-history clone target for the push is the
# guest's own clone of it. GIT_CONFIG_GLOBAL and NOSYSTEM keep the host's configuration out of it.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=Host GIT_AUTHOR_EMAIL=host@localhost \
	GIT_COMMITTER_NAME=Host GIT_COMMITTER_EMAIL=host@localhost
git init -q --bare -b main "$dir/repo.git"
git init -q -b main "$dir/seed"
printf 'seed line\n' > "$dir/seed/README"
git -C "$dir/seed" add README
git -C "$dir/seed" commit -q -m 'the seed commit'
git -C "$dir/seed" push -q "$dir/repo.git" main
git -C "$dir/repo.git" update-server-info

http_ready=$dir/httpd.port
tls_ready=$dir/tls.ports
: > "$http_ready" && : > "$tls_ready"
$HTTPD --port 0 --lifetime $((TIMEOUT * 2 + 120)) --ready-file "$http_ready" --tls-dir tests/net/tls-git --tls-ready-file "$tls_ready" \
	--git-root "$dir" --git-auth ada:s3cret > "$log" 2>&1 &
httpd=$!
trap 'status=$?; kill $httpd 2>/dev/null; wait $httpd 2>/dev/null; exit $status' EXIT
# A second server whose valid certificate is made by the OTHER test root (tests/net/tls), which the git disk does not trust
# (INV-0461-2: a certificate not made by a root of /system/tls/roots is refused, whatever GIT_SSL_CAINFO says).
tls2_ready=$dir/tls2.ports
log2=target/test-logs/test-httpd-git-untrusted.log
: > "$tls2_ready"
$HTTPD --port 0 --lifetime $((TIMEOUT * 2 + 120)) --ready-file "$dir/httpd2.port" --tls-dir tests/net/tls --tls-ready-file "$tls2_ready" \
	--git-root "$dir" > "$log2" 2>&1 &
httpd2=$!
trap 'status=$?; kill $httpd $httpd2 2>/dev/null; wait $httpd 2>/dev/null || true; wait $httpd2 2>/dev/null || true; exit $status' EXIT
for _ in $(seq 1 40); do [ -s "$tls2_ready" ] && break; perl -e 'select(undef,undef,undef,0.25)'; done
tlsother=$(cut -d' ' -f1 "$tls2_ready")
[ -n "$tlsother" ] || { cat "$log2"; fail "the second test server did not start"; }
for _ in $(seq 1 40); do [ -s "$http_ready" ] && [ -s "$tls_ready" ] && break; perl -e 'select(undef,undef,undef,0.25)'; done
port=$(cat "$http_ready")
set -- $(cat "$tls_ready")
tlsvalid=${1:-} tlsexpired=${2:-} tlswrong=${3:-}
if [ -z "$port" ] || [ -z "$tlswrong" ] || ! kill -0 $httpd 2>/dev/null; then cat "$log"; fail "the test server did not start"; fi

cp -R "$ROOT_FROM"/. "$root"/
# The git test's own root (tests/net/tls-git/README.md: mbedTLS cannot read the other one's critical name constraint).
cp tests/net/tls-git/root.pem "$root/system/tls/roots"
target/debug/zoraya-mkfs --size 64M --from "$root" "$disk"
sed -e "s/@HOST@/$NET_HOST/g" -e "s/@TLSVALID@/$tlsvalid/g" -e "s/@TLSEXPIRED@/$tlsexpired/g" -e "s/@TLSWRONG@/$tlswrong/g" -e "s/@TLSOTHER@/$tlsother/g" \
	"$template" > "$script"

set --
for p in $port $tlsvalid $tlsexpired $tlswrong $tlsother; do
	set -- "$@" --net-guestfwd "$NET_HOST:$p=$PWD/$HTTPD --relay 127.0.0.1:$p"
done
set +e
$RUNNER --timeout "$TIMEOUT" --script "$script" --label apps-git -- \
	$RUN_QEMU --test --keys --net "$@" --disk "$disk" --mem 512M --accel "$ACCEL" "$KERNEL"
rc=$?
set -e
[ "$rc" = 0 ] || { tail -30 "$log"; exit "$rc"; }

# The hooks' script needs the root file system of `make ports` (/bin/sh to run a hook, toybox's chmod and ln); without it
# the step is skipped, as the ssh step is without sshd.
posix_root=${ROOTFS_POSIX:-build/rootfs-posix}
if [ -x "$posix_root/bin/sh" ]; then
	rm -rf "$root_hooks" && mkdir -p "$root_hooks" && cp -R "$posix_root"/. "$root_hooks"/ && cp -R "$ROOT_FROM"/. "$root_hooks"/
	target/debug/zoraya-mkfs --size 96M --from "$root_hooks" "$disk_hooks"
	set +e
	$RUNNER --timeout "$TIMEOUT" --script tests/shell/apps-git-hooks.script --label apps-git-hooks -- \
		$RUN_QEMU --test --keys --net --disk "$disk_hooks" --mem 512M --accel "$ACCEL" "$KERNEL"
	rc=$?
	set -e
	[ "$rc" = 0 ] || exit "$rc"
else
	echo "[apps-git] skipped the hooks' script: no $posix_root/bin/sh (run make ports)"
fi

# What the host sees. The push arrived, as the commit the guest made, by the user the guest typed.
git -C "$dir/repo.git" log --format=%s main | grep -q 'a change typed in the guest' || { tail -30 "$log"; fail "the pushed commit is not in the repository"; }
git -C "$dir/repo.git" log --format=%an main | grep -q '^Ada Test$' || fail "the pushed commit's author is not the guest's"
grep -q 'git: POST /git/repo.git/git-receive-pack auth=yes' "$log" || fail "the server saw no authenticated push"
# A push without the credentials never got past the 401: the server never saw git-receive-pack unauthenticated succeed.
if grep -q 'git: POST /git/repo.git/git-receive-pack auth=no' "$log"; then fail "a push body reached the server without credentials"; fi
# INV-0339-2: the push with no `+net` word never reached the server: of the requests for the push's refs the log holds the one
# challenge (no credentials, answered 401) of the push that had the word, and the one with them.
n=$(grep -c 'git: GET /git/repo.git/info/refs?service=git-receive-pack auth=no' "$log" || true)
[ "$n" = 1 ] || { tail -30 "$log"; fail "INV-0339-2: $n unauthenticated requests for the push's refs, wanted 1 (the challenge)"; }
# INV-0339-5, INV-0461-2: the expired and the wrong-name servers saw a handshake and no request, and the host
# saw no request on them for the repository.
for name in expired wrong; do
	grep -q "^tls $name: ended after 0 request bytes" "$log" || { tail -30 "$log"; fail "tls $name: no refused handshake logged"; }
	if grep "^tls $name: request of" "$log"; then fail "INV-0461-2: a request reached the $name server"; fi
done
# INV-0461-2: the server of the other root saw a handshake and no request.
grep -q "^tls valid: ended after 0 request bytes" "$log2" || { tail -20 "$log2"; fail "INV-0461-2: no refused handshake on the untrusted-root server"; }
if grep "^tls valid: request of" "$log2"; then fail "INV-0461-2: a request reached the untrusted-root server"; fi
echo "[apps-git] ok: clone, commit and push over https; the expired and wrong-name servers saw no request byte"
