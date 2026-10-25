# Device tree fixtures

Device tree blobs dumped from QEMU's `virt` board with `tools/dump-dtb-fixtures.sh`.
Each was produced by the `tools/run-qemu.sh` invocation shown, which ran the QEMU command
below it. Regenerate with the script; do not edit the blobs.

QEMU: `QEMU emulator version 11.1.2`, host: `Darwin arm64`.

`rng-seed` and `kaslr-seed` in `/chosen` are random per dump.

## virt-default.dtb

```
tools/run-qemu.sh --accel tcg --dumpdtb crates/zoraya-dtb/tests/fixtures/virt-default.dtb
qemu-system-aarch64 -machine virt,dumpdtb=crates/zoraya-dtb/tests/fixtures/virt-default.dtb -accel tcg -cpu cortex-a72 -smp 1 -m 128M -nodefaults -no-reboot -display none -serial mon:stdio
```

## virt-smp4-2g.dtb

```
tools/run-qemu.sh --accel tcg --smp 4 --mem 2G --dumpdtb crates/zoraya-dtb/tests/fixtures/virt-smp4-2g.dtb
qemu-system-aarch64 -machine virt,dumpdtb=crates/zoraya-dtb/tests/fixtures/virt-smp4-2g.dtb -accel tcg -cpu cortex-a72 -smp 4 -m 2G -nodefaults -no-reboot -display none -serial mon:stdio
```

## virt-gicv3.dtb

```
tools/run-qemu.sh --accel tcg --machine-opt gic-version=3 --dumpdtb crates/zoraya-dtb/tests/fixtures/virt-gicv3.dtb
qemu-system-aarch64 -machine virt,gic-version=3,dumpdtb=crates/zoraya-dtb/tests/fixtures/virt-gicv3.dtb -accel tcg -cpu cortex-a72 -smp 1 -m 128M -nodefaults -no-reboot -display none -serial mon:stdio
```

## virt-el2.dtb

```
tools/run-qemu.sh --accel tcg --machine-opt virtualization=on --dumpdtb crates/zoraya-dtb/tests/fixtures/virt-el2.dtb
qemu-system-aarch64 -machine virt,virtualization=on,dumpdtb=crates/zoraya-dtb/tests/fixtures/virt-el2.dtb -accel tcg -cpu cortex-a72 -smp 1 -m 128M -nodefaults -no-reboot -display none -serial mon:stdio
```

## virt-hvf-default.dtb

```
tools/run-qemu.sh --accel hvf --dumpdtb crates/zoraya-dtb/tests/fixtures/virt-hvf-default.dtb
qemu-system-aarch64 -machine virt,dumpdtb=crates/zoraya-dtb/tests/fixtures/virt-hvf-default.dtb -accel hvf -cpu host -smp 1 -m 128M -nodefaults -no-reboot -display none -serial mon:stdio
```
