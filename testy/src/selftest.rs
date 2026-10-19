//! QEMU 自测：在真 UEFI 环境里跑完核心链路，打串口标记给外部 runner 断言。
//!
//! 覆盖：全局分配器（Box/Vec）、nano 执行器（spawn/join/try_join/join_all/select、
//! 超时、WaitTimer、Pacer/Skip/Yield）、dynamic 执行器（spawn/cancel/retune、
//! 跨执行器 steal）、oneshot 通道、MultiCoreSignal。全部有界、秒级完成。
//! 跑完即返回（调用方继续原有 demo；runner 见到标记即判分，不等退出）。

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};
use uefi::println;
use uefi_async::common::signal::multiple::MultiCoreSignal;
use uefi_async::dynamic::Executor as DynExec;
use uefi_async::nano_alloc::channel::single::oneshot::Oneshot;
use uefi_async::nano_alloc::control::single::join::{join, join_all, try_join};
use uefi_async::nano_alloc::control::single::select::select;
use uefi_async::nano_alloc::executor::Executor as NanoExec;
use uefi_async::nano_alloc::task::TaskNode as NanoNode;
use uefi_async::nano_alloc::time::{_Timeout, _WaitTimer, WaitTimer};
use uefi_async::{tick, FREQ, Pacer, Skip, Yield};

struct Ctx {
    ok: bool,
}

macro_rules! check {
    ($ctx:expr, $name:expr, $cond:expr) => {{
        if $cond {
            println!("SELFTEST {} ... ok", $name);
        } else {
            println!("SELFTEST {} ... FAILED", $name);
            $ctx.ok = false;
        }
    }};
}

async fn ready_val(v: u32) -> u32 {
    v
}

async fn unit() {}

async fn ready_ok() -> Result<(), ()> {
    Ok(())
}

pub fn run() -> bool {
    println!("SELFTEST begin");
    let mut ctx = Ctx { ok: true };

    // ---- 全局分配器：Box/Vec 可用即算过（后面所有 async 都依赖它） ----
    {
        let mut v = Vec::new();
        v.push(1u32);
        v.push(2u32);
        let b = Box::new(7u32);
        check!(ctx, "alloc", v.iter().sum::<u32>() == 3 && *b == 7);
    }

    // ---- nano 执行器：多任务 + join/timeout/select/pacer 全跑完 ----
    {
        let flag_join = Rc::new(RefCell::new(0u32));
        let flag_to = Rc::new(RefCell::new(false));
        let flag_sel = Rc::new(RefCell::new(0u32));
        let flag_pace = Rc::new(RefCell::new(0u32));
        let f0 = flag_join.clone();
        let f1 = flag_to.clone();
        let f2 = flag_sel.clone();
        let f3 = flag_pace.clone();
        let mut exec = NanoExec::new();
        let mut cx = NanoExec::init_step();
        let mut n0 = NanoNode::new(
            Box::pin(async move {
                join!(unit(), unit()).await;
                let (a, b) = join_all!(ready_val(3), ready_val(4)).await;
                let t = try_join!(ready_ok(), ready_ok()).await;
                if t.is_ok() && a + b == 7 {
                    *f0.borrow_mut() = 1;
                }
            }),
            0,
        );
        let mut n1 = NanoNode::new(
            Box::pin(async move {
                // 就绪 future + 大 deadline：必走 Ok 分支。
                if ready_val(9).timeout(tick().wrapping_add(1_000_000_000)).await == Ok(9) {
                    *f1.borrow_mut() = true;
                }
            }),
            0,
        );
        let mut n2 = NanoNode::new(
            Box::pin(async move {
                select! {
                    ready_val(1) => { *f2.borrow_mut() = 1; },
                    WaitTimer::from_ms(60_000) => { *f2.borrow_mut() = 2; },
                }
            }),
            0,
        );
        let mut n3 = NanoNode::new(
            Box::pin(async move {
                let mut pacer = Pacer::new(2);
                pacer.throttle().await;
                pacer.burst(2).await;
                pacer.step(1, false).await;
                Yield.await;
                Skip(2).await;
                5.ms().await;
                *f3.borrow_mut() = 1;
            }),
            0,
        );
        exec.add(&mut n0).add(&mut n1).add(&mut n2).add(&mut n3);
        // 墙钟兜底（5s）：5.ms() 这类真时间等待不能只靠固定步数，
        // 步进多快取决于机器，墙钟说了算。
        let t0 = tick();
        let budget = FREQ.hz().max(1).saturating_mul(5);
        loop {
            exec.run_step(tick(), &mut cx);
            if *flag_join.borrow() == 1
                && *flag_to.borrow()
                && *flag_sel.borrow() == 1
                && *flag_pace.borrow() == 1
            {
                break;
            }
            if tick().wrapping_sub(t0) > budget {
                break;
            }
        }
        check!(ctx, "nano-join", *flag_join.borrow() == 1);
        check!(ctx, "nano-timeout", *flag_to.borrow());
        check!(ctx, "nano-select", *flag_sel.borrow() == 1);
        check!(ctx, "nano-pacer", *flag_pace.borrow() == 1);
    }

    // ---- dynamic 执行器：spawn/cancel/retune/steal ----
    // Box 里的 future 必须 Send：跨任务状态走静态原子量，不用 Rc。
    {
        static DRAN: AtomicU32 = AtomicU32::new(0);
        let mut a = DynExec::new();
        let mut b = DynExec::new();
        b.add_stealer(a.stealer());
        let mut cx = DynExec::init_step();
        let id_run = a
            .spawn(
                Box::pin(async move {
                    DRAN.fetch_add(1, Ordering::Release);
                }),
                0,
            )
            .expect("spawn run");
        let id_gone = a.spawn(Box::pin(async {}), 0).expect("spawn gone");
        check!(ctx, "dyn-spawn", a.is_alive(id_run) && a.is_alive(id_gone));
        // cancel 先于首次调度：被收割，不执行。
        check!(ctx, "dyn-cancel", a.cancel(id_gone));
        check!(ctx, "dyn-retune", a.set_freq(id_run, 100));
        check!(ctx, "dyn-cancel-unknown", !a.cancel(0xFFFF_FFFF_FFFF_FFFF));
        // B 只从 A 偷：验证跨执行器 stealing 通路。
        for _ in 0..500 {
            b.run_step(tick(), &mut cx);
            if DRAN.load(Ordering::Acquire) == 1 {
                break;
            }
        }
        check!(ctx, "dyn-steal-run", DRAN.load(Ordering::Acquire) == 1);
        // 跑完即回收，cancel 已完成任务返回 false。
        for _ in 0..500 {
            a.run_step(tick(), &mut cx);
            if !a.is_alive(id_run) {
                break;
            }
        }
        check!(ctx, "dyn-reap", !a.is_alive(id_run) && !a.cancel(id_run));
    }

    // ---- oneshot + MultiCoreSignal（同核直驱） ----
    {
        let (tx, rx) = Oneshot::new();
        tx.send(42u32);
        let mut exec = NanoExec::new();
        let mut cx = NanoExec::init_step();
        let got = Rc::new(RefCell::new(0u32));
        let g = got.clone();
        let mut n = NanoNode::new(
            Box::pin(async move {
                *g.borrow_mut() = rx.await;
            }),
            0,
        );
        exec.add(&mut n);
        for _ in 0..100 {
            exec.run_step(tick(), &mut cx);
            if *got.borrow() == 42 {
                break;
            }
        }
        check!(ctx, "oneshot", *got.borrow() == 42);

        static SIG: MultiCoreSignal<u64> = MultiCoreSignal::new();
        SIG.signal(7);
        let sgot = Rc::new(RefCell::new(0u64));
        let sg = sgot.clone();
        let mut nsig = NanoNode::new(
            Box::pin(async move {
                *sg.borrow_mut() = SIG.wait().await;
            }),
            0,
        );
        exec.add(&mut nsig);
        for _ in 0..100 {
            exec.run_step(tick(), &mut cx);
            if *sgot.borrow() == 7 {
                break;
            }
        }
        check!(ctx, "signal", *sgot.borrow() == 7);
    }

    if ctx.ok {
        println!("SELFTEST PASS");
    } else {
        println!("SELFTEST FAIL");
    }
    ctx.ok
}
