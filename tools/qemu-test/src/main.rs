//! uefi-async QEMU 黑盒冒烟（host 工具，100% Rust，测试逻辑全在这里）。
//!
//! 流程：编 testy（x86_64-unknown-uefi）→ 组 FAT 镜像（EFI/BOOT/BOOTX64.EFI）→
//! 起 QEMU（RELEASE OVMF，无 ASSERT 干扰；-accel hvf/kvm，退回 tcg 只警告）→
//! 抓串口等 `SELFTEST PASS` / `SELFTEST FAIL`（超时即 FAIL）。
//! testy 的 `selftest` 跑完核心链路（alloc/nano/dynamic/channel/signal），
//! 这些在 host 上要么编不过（no_std），要么跑不起来（要 UEFI 引导服务）。

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TOOL_DIR: &str = env!("CARGO_MANIFEST_DIR");
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const OVMF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/ovmf-release-x86_64-code.fd"
);

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("spawn {cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn build_testy() -> Result<String, String> {
    println!("[build] cargo build -p my-testy --target x86_64-unknown-uefi");
    let out = Command::new("cargo")
        .args(["build", "-p", "my-testy", "--target", "x86_64-unknown-uefi"])
        .current_dir(ROOT)
        .output()
        .map_err(|e| format!("spawn cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    for p in [
        format!("{ROOT}/target/x86_64-unknown-uefi/debug/my-testy.efi"),
        format!("{ROOT}/target/x86_64-unknown-uefi/release/my-testy.efi"),
    ] {
        if std::path::Path::new(&p).exists() {
            return Ok(p);
        }
    }
    Err("testy efi missing after build".into())
}

/// 建空 FAT 镜像。macOS 走 hdiutil+newfs_msdos（已验证），Linux 走 mtools。
fn make_fat_image(path: &str) -> Result<&'static str, String> {
    if cfg!(target_os = "macos") {
        run("mkfile", &["-n", "64m", path])?;
        let out = run("hdiutil", &["attach", "-nomount", path])?;
        let dev = out
            .split_whitespace()
            .find(|t| t.starts_with("/dev/"))
            .ok_or_else(|| format!("hdiutil attach gave no device: {out}"))?
            .to_string();
        let r = run("newfs_msdos", &["-F", "32", "-v", "UEFIASYNC", &dev]);
        let _ = run("hdiutil", &["detach", &dev]);
        r?;
        return Ok("hdiutil");
    }
    if run("which", &["mformat"]).is_ok() {
        run("mformat", &["-i", path, "-C", "-F", "-v", "UEFIASYNC", "::"])?;
        return Ok("mtools");
    }
    Err("no FAT tool: macOS needs hdiutil/newfs_msdos, linux needs mtools".into())
}

fn populate(fstype: &str, image: &str, efi: &[u8]) -> Result<(), String> {
    if fstype == "hdiutil" {
        let mnt = format!("{image}.mnt");
        let _ = std::fs::create_dir_all(&mnt);
        run("hdiutil", &["attach", "-mountpoint", &mnt, image])?;
        let mut ok: Result<(), String> = Ok(());
        for (arc, data) in [("EFI/BOOT/BOOTX64.EFI", efi)] {
            let dst = format!("{mnt}/{arc}");
            if let Some(p) = std::path::Path::new(&dst).parent() {
                if std::fs::create_dir_all(p).is_err() {
                    ok = Err(format!("mkdir {p:?}"));
                    break;
                }
            }
            if std::fs::write(&dst, data).is_err() {
                ok = Err(format!("write {dst}"));
                break;
            }
        }
        let _ = run("hdiutil", &["detach", &mnt]);
        return ok;
    }
    let stage = format!("{image}.stage");
    std::fs::write(&stage, efi).map_err(|e| format!("stage: {e}"))?;
    for dir in ["::/EFI", "::/EFI/BOOT"] {
        let _ = run("mmd", &["-i", image, dir]);
    }
    run("mcopy", &["-i", image, "-o", &stage, "::/EFI/BOOT/BOOTX64.EFI"])?;
    let _ = std::fs::remove_file(&stage);
    Ok(())
}

fn pick_accel() -> Vec<String> {
    if cfg!(target_os = "macos") {
        return vec!["-accel".into(), "hvf".into()];
    }
    if std::path::Path::new("/dev/kvm").exists() {
        return vec!["-accel".into(), "kvm".into()];
    }
    eprintln!("[warn] no hvf/kvm, falling back to tcg (slow)");
    vec!["-accel".into(), "tcg".into()]
}

fn wait_marker(path: &str, timeout: Duration, proc: &mut Child) -> (bool, String) {
    let start = Instant::now();
    loop {
        if let Ok(Some(status)) = proc.try_wait() {
            return (
                false,
                format!("<qemu exited rc={}>", status.code().unwrap_or(-1)),
            );
        }
        let data = std::fs::read_to_string(path).unwrap_or_default();
        if data.contains("SELFTEST PASS") {
            return (true, String::new());
        }
        if data.contains("SELFTEST FAIL") {
            return (false, "serial has SELFTEST FAIL".into());
        }
        if data.contains("panicked at") {
            return (false, "firmware panicked".into());
        }
        if start.elapsed() >= timeout {
            return (false, "timeout waiting for SELFTEST PASS".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn main() {
    let keep = std::env::args().any(|a| a == "--keep");
    let rundir = format!("{TOOL_DIR}/runs/smoke");
    if std::path::Path::new(&rundir).exists() && !keep {
        let _ = std::fs::remove_dir_all(&rundir);
    }
    let _ = std::fs::create_dir_all(&rundir);
    let serial = format!("{rundir}/serial.log");
    let image = format!("{rundir}/usb.img");
    let _ = std::fs::remove_file(&image);

    let efi = match build_testy() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let efi_data = std::fs::read(&efi).unwrap_or_default();
    if efi_data.is_empty() {
        eprintln!("efi unreadable");
        std::process::exit(2);
    }
    let fstype = match make_fat_image(&image) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = populate(fstype, &image, &efi_data) {
        eprintln!("{e}");
        std::process::exit(2);
    }

    let port: u16 = 44600;
    let mut cmd = vec![
        "qemu-system-x86_64".to_string(),
        "-M".into(),
        "q35".into(),
        "-device".into(),
        "virtio-vga,xres=1280,yres=720".into(),
        "-drive".into(),
        format!("if=pflash,format=raw,unit=0,file={OVMF},readonly=on"),
        "-drive".into(),
        format!("file={image},format=raw,if=none,id=hd"),
        "-device".into(),
        "virtio-blk-pci,drive=hd".into(),
        "-smp".into(),
        "2".into(),
        "-m".into(),
        "512".into(),
        "-display".into(),
        "none".into(),
        "-no-reboot".into(),
        "-serial".into(),
        format!("file:{serial}"),
        "-monitor".into(),
        format!("telnet:127.0.0.1:{port},server,nowait"),
    ];
    cmd.extend(pick_accel());
    println!("  {}", cmd.join(" "));
    let mut proc = match Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("spawn qemu: {e}");
            std::process::exit(2);
        }
    };
    let (ok, detail) = wait_marker(&serial, Duration::from_secs(180), &mut proc);
    let _ = proc.kill();
    let _ = proc.wait();
    // 落盘串口节选：只打 SELFTEST 行， verdict 一眼可见。
    if let Ok(data) = std::fs::read_to_string(&serial) {
        for line in data.lines().filter(|l| l.contains("SELFTEST")) {
            println!("  {line}");
        }
    }
    if ok {
        println!("[PASS] qemu smoke (SELFTEST PASS)");
    } else {
        println!("[FAIL] qemu smoke -- {detail}");
        std::process::exit(1);
    }
}
