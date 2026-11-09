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
#   --keys                with --test: stdin goes to the serial line, for the
#                         scripted shell tests
#   --test-disk           attach an empty virtio-blk device that reads as zeros,
#                         for tests of device objects
#   --disk FILE           attach FILE as the virtio-blk disk
#   --disk-write-error    with --disk: the first write to it fails with EIO
#   --gui                 attach a virtio GPU (1024x768), keyboard and tablet,
#                         each alone on its page (ADR-0091, ADR-0097). Without
#                         --test, open QEMU's default display window
#   --gui-scale N         with --gui: 1 (1024x768) or 2 (2048x1536, the
#                         compositor's scale 2, ADR-0164)
#   --qmp PATH            a QMP socket at PATH, for screendumps and keys
#   --display-opts OPTS   the interactive display and its suboptions (with
#                         --gui; ignored with --test); the default is below
#   --debug               TCG, wait for a debugger on tcp::1234
#   --dumpdtb FILE        write the generated device tree to FILE and exit;
#                         no kernel needed
#   --reboot              a reset restarts the machine instead of ending QEMU
#                         (without it, -no-reboot; ADR-0159, INV-0155-7)
#   --print-cmd           print the QEMU command line on stderr first
#   --probe-el2           no kernel needed: exit 0 if the accelerator can start
#                         a machine at EL2 (virtualization=on). Exit 1, with the
#                         reason on stdout, only if it is HVF and HVF starts the
#                         plain machine but not the one with EL2 (HVF nested
#                         virtualisation needs an M3 or later). TCG is not
#                         probed. The probe creates the VM through --dumpdtb, so
#                         it tests the host, never the kernel.
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
reboot=
probe_el2=0
keys=0
test_disk=0
write_error=0
disk=
gui=0
gui_scale=1
qmp=
display_opts=
kernel=

die() { echo "run-qemu.sh: $*" >&2; exit 2; }

while [ $# -gt 0 ]; do
    case $1 in
        --accel) [ $# -ge 2 ] || die "--accel needs a value"; accel=$2; shift 2 ;;
        --smp) [ $# -ge 2 ] || die "--smp needs a value"; smp=$2; shift 2 ;;
        --mem) [ $# -ge 2 ] || die "--mem needs a value"; mem=$2; shift 2 ;;
        --machine-opt) [ $# -ge 2 ] || die "--machine-opt needs a value"; machine="$machine,$2"; shift 2 ;;
        --test) mode=test; shift ;;
        --keys) keys=1; shift ;;
        --test-disk) test_disk=1; shift ;;
        --disk-write-error) write_error=1; shift ;;
        --disk) [ $# -ge 2 ] || die "--disk needs a file"; disk=$2; shift 2 ;;
        --gui) gui=1; shift ;;
        --gui-scale) [ $# -ge 2 ] || die "--gui-scale needs 1 or 2"; gui_scale=$2; shift 2 ;;
        --qmp) [ $# -ge 2 ] || die "--qmp needs a path"; qmp=$2; shift 2 ;;
        --display-opts) [ $# -ge 2 ] || die "--display-opts needs a value"; display_opts=$2; shift 2 ;;
        --debug) mode=debug; accel=tcg; shift ;;
        --dumpdtb) [ $# -ge 2 ] || die "--dumpdtb needs a file"; dumpdtb=$2; shift 2 ;;
        --print-cmd) print_cmd=1; shift ;;
        --reboot) reboot=1; shift ;;
        --probe-el2) probe_el2=1; shift ;;
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

# Each probe is a run of this script with --dumpdtb: QEMU creates the VM, which
# is where HVF refuses EL2, writes the device tree and exits.
if [ "$probe_el2" = 1 ]; then
    [ "$accel" = hvf ] || exit 0
    dir=$(mktemp -d "${TMPDIR:-/tmp}/zoraya-probe.XXXXXX")
    trap 'rm -rf "$dir"' EXIT
    # If HVF cannot start even the plain machine, report EL2 as available, so
    # that the configuration runs and fails where it shows.
    # Each probe runs in a subshell that waits for it (the "exit $?" keeps the
    # shell from exec'ing it), so the report of QEMU's abort goes to the
    # subshell's stderr, which is discarded, not to ours.
    ("$0" --accel hvf --dumpdtb "$dir/plain.dtb"; exit $?) </dev/null >/dev/null 2>&1 || exit 0
    ("$0" --accel hvf --machine-opt virtualization=on --dumpdtb "$dir/el2.dtb"; exit $?) </dev/null >/dev/null 2>&1 && exit 0
    echo "HVF on this Mac has no EL2 (needs M3 or later)"
    exit 1
fi

if [ -n "$dumpdtb" ]; then
    machine="$machine,dumpdtb=$dumpdtb"
elif [ -z "$kernel" ]; then
    die "no kernel ELF given"
fi

set -- "$QEMU" -machine "$machine" -accel "$accel" -cpu "$cpu" \
    -smp "$smp" -m "$mem" -nodefaults
[ -n "$reboot" ] || set -- "$@" -no-reboot
[ -n "$kernel" ] && [ -z "$dumpdtb" ] && set -- "$@" -kernel "$kernel"

# Interactive with --gui: QEMU's display opens a window. On macOS the
# cocoa display keeps the Command keys for the host unless told otherwise;
# left-command-key=on sends both Command keys to the guest as Meta and keeps
# Meta on a chord (measured on 2026-11-03 with QEMU 11.1.2, whose default
# already does so; docs/research/desktop/command-key.md), which
# the stage 7 desktop needs for its system key (ADR-0160). Cmd-Tab stays
# the system's. --display-opts replaces the whole value.
display=none
if [ "$gui" = 1 ] && [ "$mode" = interactive ]; then
    display=default
    [ "$(uname -s)" = Darwin ] && display=cocoa,left-command-key=on
fi
[ -n "$display_opts" ] && display=$display_opts
case $mode in
    interactive) set -- "$@" -display "$display" -serial mon:stdio ;;
    test) set -- "$@" -display none -monitor none -serial stdio ;;
    debug) set -- "$@" -display none -serial mon:stdio -s -S ;;
esac

# Version 2 of virtio-mmio: QEMU defaults to the legacy transport (ADR-0080).
if [ "$test_disk" = 1 ] || [ -n "$disk" ] || [ "$gui" = 1 ]; then
    set -- "$@" -global virtio-mmio.force-legacy=false
fi
if [ "$test_disk" = 1 ]; then
    set -- "$@" -drive driver=null-co,if=none,id=testdisk -device virtio-blk-device,drive=testdisk
fi
if [ -n "$disk" ]; then
    source="file=$disk"
    # One write to the disk fails with EIO: QEMU's blkdebug, under the raw format.
    if [ "$write_error" = 1 ]; then
        source="file.driver=blkdebug,file.image.filename=$disk,file.inject-error.0.event=write_aio"
        source="$source,file.inject-error.0.iotype=write,file.inject-error.0.once=on"
    fi
    set -- "$@" -drive "$source,format=raw,if=none,id=disk,werror=report,rerror=report" \
        -device virtio-blk-device,drive=disk
fi

# Each GUI device alone on a page of eight transports (ADR-0091): QEMU puts
# the first -device without bus= on transport 31, the disk's, page 3.
if [ "$gui" = 1 ]; then
    # ADR-0164: --gui-scale 2 asks for twice the pixels; the compositor
    # takes scale 2 from a display at least 1920 wide.
    case $gui_scale in
        1) xres=1024 yres=768 ;;
        2) xres=2048 yres=1536 ;;
        *) die "--gui-scale is 1 or 2" ;;
    esac
    set -- "$@" -device virtio-gpu-device,bus=virtio-mmio-bus.23,xres=$xres,yres=$yres \
        -device virtio-keyboard-device,bus=virtio-mmio-bus.15 \
        -device virtio-tablet-device,bus=virtio-mmio-bus.7
fi
if [ -n "$qmp" ]; then
    set -- "$@" -qmp "unix:$qmp,server=on,wait=off"
fi

if [ "$print_cmd" = 1 ]; then
    echo "$*" >&2
fi

# A test run or a device-tree dump never reads the terminal: QEMU would make
# an inherited terminal non-blocking, and the runner's and make's writes to it
# would fail with EAGAIN (docs/backlog.md). With --keys stdin is the
# runner's pipe.
if { [ "$mode" = test ] && [ "$keys" = 0 ]; } || [ -n "$dumpdtb" ]; then
    exec "$@" </dev/null
fi
exec "$@"
