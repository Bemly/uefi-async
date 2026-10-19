# uefi-async QEMU 黑盒冒烟

起真 UEFI（RELEASE OVMF，无 ASSERT 干扰），跑 `testy` 的 `selftest`，
断言串口出现 `SELFTEST PASS`。覆盖在 host 上编不过/跑不起来的部分：
全局分配器、nano/dynamic 执行器、oneshot 通道、MultiCoreSignal、
WaitTimer/Timeout/Pacer（纯逻辑部分另见 `cargo test`）。

```bash
# 全量（先编 testy x86_64-unknown-uefi，再起 QEMU，默认清掉重跑）
cargo run --manifest-path tools/qemu-test/Cargo.toml --target "$(rustc -vV | sed -n 's/^host: //p')"

# 留盘排查（runs/smoke/ 下 serial.log + usb.img 不删）
cargo run --manifest-path tools/qemu-test/Cargo.toml --target "$(rustc -vV | sed -n 's/^host: //p')" -- --keep
```

产物在 `tools/qemu-test/runs/smoke/`（默认清掉重跑；`--keep` 留盘排查）。
固件 `assets/ovmf-release-x86_64-code.fd`（1.9MB，
osdev0/edk2-ovmf-nightly `20261019T035340Z`，`build -b RELEASE`；
与 uefi-player 同一文件，来源见该仓 `tools/qemu-matrix/README.md`）。
