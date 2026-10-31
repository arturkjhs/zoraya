#!/bin/sh
# The one place that knows the QEMU command line. Make targets, the cargo
# runner, the in-QEMU test runner and the DTB fixture script all call this;
# nothing else invokes QEMU.
#
# Usage: tools/run-qemu.sh [options] [KERNEL.elf]
#
#   --accel auto|hvf|tcg  auto (default): hvf on an Apple Silicon Mac where the
#                         Hypervisor framework is available, tcg elsewhere.
#                         Also settable as ZORAYA_ACCEL.
#   --smp N               CPUs (default 1)
#   --mem SIZE            RAM, QEMU syntax (default 128M)
#   --machine-opt OPT     appended to -machine virt,...; repeatable
#                         (e.g. gic-version=3, virtualization=on)
#   --test                non-interactive: serial on stdout, no monitor,
#                         stdin not read
#   --debug               TCG, wait for a debugger on tcp::1234
#   --dumpdtb FILE        write the generated device tree to FILE and exit;
#                         no kernel needed
#   --initrd FILE         load FILE as the initial ramdisk; the device tree's
#                         /chosen says where (linux,initrd-start and -end)
#   --print-cmd           print the QEMU command line on stderr first
#
# Loading: the kernel ELF is passed with -kernel. QEMU's virt board treats an
# ELF as a bare-metal image: it loads the segments at their physical
# addresses, starts the first CPU at the ELF entry at EL1 (EL2 with
# virtualization=on), keeps the other CPUs powered off under PSCI, and places
# the device tree blob at the start of RAM (0x40000000). See
# docs/design.md, "Boot".

set -eu

QEMU=${QEMU:-qemu-system-aarch64}
accel=${ZORAYA_ACCEL:-auto}
smp=1
mem=128M
machine=virt
mode=interactive
dumpdtb=
print_cmd=0
kernel=
initrd=

die() { echo "run-qemu.sh: $*" >&2; exit 2; }

while [ $# -gt 0 ]; do
    case $1 in
        --accel) [ $# -ge 2 ] || die "--accel needs a value"; accel=$2; shift 2 ;;
        --smp) [ $# -ge 2 ] || die "--smp needs a value"; smp=$2; shift 2 ;;
        --mem) [ $# -ge 2 ] || die "--mem needs a value"; mem=$2; shift 2 ;;
        --machine-opt) [ $# -ge 2 ] || die "--machine-opt needs a value"; machine="$machine,$2"; shift 2 ;;
        --test) mode=test; shift ;;
        --debug) mode=debug; accel=tcg; shift ;;
        --dumpdtb) [ $# -ge 2 ] || die "--dumpdtb needs a file"; dumpdtb=$2; shift 2 ;;
        --initrd) [ $# -ge 2 ] || die "--initrd needs a file"; initrd=$2; shift 2 ;;
        --print-cmd) print_cmd=1; shift ;;
        -h|--help) sed -n '2,/^$/s/^# \{0,1\}//p' "$0"; exit 0 ;;
        -*) die "unknown option $1" ;;
        *) [ -z "$kernel" ] || die "more than one kernel given"; kernel=$1; shift ;;
    esac
done

command -v "$QEMU" >/dev/null 2>&1 || die "$QEMU not found; install QEMU (macOS: brew install qemu)"

if [ "$accel" = auto ]; then
    accel=tcg
    if [ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] &&
        [ "$(sysctl -n kern.hv_support 2>/dev/null || echo 0)" = 1 ]; then
        accel=hvf
    fi
fi
case $accel in
    hvf) cpu=host ;;
    tcg) cpu=cortex-a72 ;;
    *) die "unknown accelerator $accel (auto, hvf or tcg)" ;;
esac

if [ -n "$dumpdtb" ]; then
    machine="$machine,dumpdtb=$dumpdtb"
elif [ -z "$kernel" ]; then
    die "no kernel ELF given"
fi

set -- "$QEMU" -machine "$machine" -accel "$accel" -cpu "$cpu" \
    -smp "$smp" -m "$mem" -nodefaults -no-reboot
[ -n "$kernel" ] && set -- "$@" -kernel "$kernel"
[ -n "$initrd" ] && set -- "$@" -initrd "$initrd"

case $mode in
    interactive) set -- "$@" -display none -serial mon:stdio ;;
    test) set -- "$@" -display none -monitor none -serial stdio ;;
    debug) set -- "$@" -display none -serial mon:stdio -s -S ;;
esac

if [ "$print_cmd" = 1 ]; then
    echo "$*" >&2
fi

if [ "$mode" = test ]; then
    exec "$@" </dev/null
fi
exec "$@"
