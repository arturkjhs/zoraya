#!/bin/sh
# Regenerates the device tree fixtures used by zoraya-dtb's host tests and
# records the exact QEMU command for each in the fixtures' README.md.
# Run from the repository root. Not part of `make test`: the committed
# fixtures are the reference.
#
# The hvf fixture is written only on a host where hvf is available.

set -eu

out=crates/zoraya-dtb/tests/fixtures
run=tools/run-qemu.sh
readme=$out/README.md
qemu_version=$(${QEMU:-qemu-system-aarch64} --version | head -n 1)

{
    echo "# Device tree fixtures"
    echo
    echo "Device tree blobs dumped from QEMU's \`virt\` board with \`tools/dump-dtb-fixtures.sh\`."
    echo "Each was produced by the \`tools/run-qemu.sh\` invocation shown, which ran the QEMU command"
    echo "below it. Regenerate with the script; do not edit the blobs."
    echo
    echo "QEMU: \`$qemu_version\`, host: \`$(uname -sm)\`."
    echo
    echo "\`rng-seed\` and \`kaslr-seed\` in \`/chosen\` are random per dump."
} >"$readme"

dump() {
    name=$1
    shift
    cmd=$("$run" --print-cmd "$@" --dumpdtb "$out/$name.dtb" 2>&1 >/dev/null)
    {
        echo
        echo "## $name.dtb"
        echo
        echo '```'
        echo "$run $* --dumpdtb $out/$name.dtb"
        echo "$cmd"
        echo '```'
    } >>"$readme"
    echo "wrote $out/$name.dtb"
}

dump virt-default --accel tcg
dump virt-smp4-2g --accel tcg --smp 4 --mem 2G
dump virt-gicv3 --accel tcg --machine-opt gic-version=3
dump virt-el2 --accel tcg --machine-opt virtualization=on
if [ "$(uname -s)" = Darwin ] && [ "$(sysctl -n kern.hv_support 2>/dev/null || echo 0)" = 1 ]; then
    dump virt-hvf-default --accel hvf
fi
