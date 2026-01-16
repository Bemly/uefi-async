Please run the following script in reverse order, because the RustRover README executable has a bug and will execute backwards.

The author wrote it in reverse order for convenience.

下面脚本请倒着运行，因为RustRover的README运行程序有Bug，会倒着执行

作者这里为了方便就倒着写了

Debug Run:
```shell
qemu-system-x86_64 -drive if=pflash,format=raw,file=qemu/OVMF.fd -drive format=raw,file=fat:rw:qemu -m 4G -device usb-ehci -device usb-tablet -device virtio-gpu-pci -smp 4
mv ..\target\x86_64-unknown-uefi\debug\my-testy.efi .\qemu\EFI\BOOT\BOOTX64.EFI
rm .\qemu\EFI\BOOT\BOOTX64.EFI
cargo build
```

Run:
```shell
qemu-system-x86_64 -drive if=pflash,format=raw,file=qemu/OVMF.fd -drive format=raw,file=fat:rw:qemu -m 4G -device usb-ehci -device usb-tablet -device virtio-gpu-pci -smp 4 -cpu max -monitor stdio
mv ..\target\x86_64-unknown-uefi\release\my-testy.efi .\qemu\EFI\BOOT\BOOTX64.EFI
rm .\qemu\EFI\BOOT\BOOTX64.EFI
cargo build --release
```