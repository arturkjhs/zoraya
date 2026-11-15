#!/bin/sh
# TASK-35: runs the probe inside Zoraya, one boot per script, through
# tools/test-runner (the only way QEMU is run). From the repository root,
# after build.sh and `make build/zoraya-kernel.elf target/debug/zoraya-test-runner
# tools-storage` and `cargo build -p zoraya-test-httpd`.
#   [VARIANT=today|fixed] docs/research/rust-std/probes/run.sh SCRIPT [ACCEL]
# The probe goes on the disk three times: /bin/rprobe (for posixrun),
# /bin/cxxtest (run through the broker under cxxtest's description, so that
# +r and +w grant directories) and /bin/zfetch (under zfetch's description,
# which grants the connection to the URL on the line).
set -eu
script=$1
accel=${2:-auto}
variant=${VARIANT:-today}
name=$(basename "$script" .script)-${VARIANT:-today}
out=build/rust-std-probe/run-$name
rm -rf "$out" && mkdir -p "$out/root"
cp -R rootfs/. "$out/root/"
mkdir -p "$out/root/bin" "$out/root/work/granted" "$out/root/work/secret"
for n in rprobe cxxtest zfetch; do cp "build/rust-std-probe/rprobe-$variant" "$out/root/bin/$n"; done
printf 'one\n' > "$out/root/work/granted/one.txt"
printf 'net.hosts = test.zoraya=10.0.2.2\n' > "$out/root/config/settings"
target/debug/zoraya-elf-check "$out/root/bin"
target/debug/zoraya-mkfs --size 32M --from "$out/root" "$out/disk.img"
net=
httpd=
case "$name" in
*net*)
    : > "$out/ready"
    target/debug/zoraya-test-httpd --port 0 --lifetime 300 --ready-file "$out/ready" > "$out/httpd.log" 2>&1 &
    httpd=$!
    i=0
    while [ ! -s "$out/ready" ] && [ $i -lt 40 ]; do perl -e 'select(undef,undef,undef,0.25)'; i=$((i+1)); done
    port=$(cat "$out/ready")
    sed "s/@PORT@/$port/g" "$script" > "$out/script"
    script=$out/script
    net="--net --net-open"
    ;;
esac
rc=0
target/debug/zoraya-test-runner --timeout 300 --script "$script" --label "rust-std-$name-$accel" -- \
    tools/run-qemu.sh --test --keys $net --disk "$out/disk.img" --accel "$accel" build/zoraya-kernel.elf || rc=$?
[ -n "$httpd" ] && kill "$httpd" 2>/dev/null
echo "run.sh: $name exit $rc; log in target/test-logs/"
exit $rc
