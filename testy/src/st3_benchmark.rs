use core::fmt::Write;
use alloc::boxed::Box;
use alloc::string::String;
use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;
use spin::Barrier;
use st3::lifo::{Stealer, Worker};
use uefi::boot::{create_event, get_handle_for_protocol, get_image_file_system, image_handle, open_protocol_exclusive, stall, EventType, Tpl};
use uefi::fs::FileSystem;
use uefi::{cstr16, println};
use uefi::proto::pi::mp::MpServices;

#[repr(C)]
struct Context<'bemly_> {
    pub mp: &'bemly_ MpServices,
    pub num_cores: usize,
    // 存储每个核心 Stealer 的 Arc 原始指针
    pub stealers_raw: *mut [usize; 64],
    // 用于记录每个核心的局部成绩
    pub scores: *mut [u64; 64],
}

// 这里的转换非常关键，利用 repr(transparent) 特性
fn stealer_to_ptr<T>(s: Stealer<T>) -> usize {
    unsafe { transmute::<Stealer<T>, usize>(s) }
}

fn ptr_to_stealer_ref<'a, T>(ptr: usize) -> &'a Stealer<T> {
    unsafe { transmute::<&usize, &Stealer<T>>(&*(ptr as *const usize)) }
}

static ALL_CORE_READY: Barrier = Barrier::new(4);

pub unsafe fn benchmark() {

    calibrate_tsc();

    let mp = get_handle_for_protocol::<MpServices>()
        .expect("Failed to get MP services");
    let mp = open_protocol_exclusive::<MpServices>(mp)
        .expect("Failed to open MP services");
    let num_cores = mp.get_number_of_processors()
        .expect("Failed to get number of processors")
        .enabled;

    // 使用我们之前配好的 Talc 分配器分配共享内存
    let stealers_raw = Box::new([0usize; 64]);
    let scores = Box::new([0u64; 64]);

    let mut ctx = Context {
        mp: &mp,
        num_cores,
        stealers_raw: Box::into_raw(stealers_raw),
        scores: Box::into_raw(scores),
    };
    let arg_ptr = addr_of_mut!(ctx).cast();

    let event = unsafe {
        create_event(EventType::empty(), Tpl::CALLBACK, None, None)
            .expect("Failed to create event")
    };

    if num_cores > 1 {
        let _ = mp.startup_all_aps(false, process, arg_ptr, Some(event), None);
    }
    process(arg_ptr);
}

#[inline(always)]
fn time() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

static TICKS_PER_SECOND: AtomicU64 = AtomicU64::new(0);

fn calibrate_tsc() {
    let start = time();
    // Stall 使用微秒 (microseconds) 为单位
    stall(Duration::from_micros(100_000));
    let end = time();

    let ticks_per_100ms = end - start;
    // 换算成 1 秒的频率
    TICKS_PER_SECOND.store(ticks_per_100ms * 10, Ordering::Release);
}

extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &mut *arg.cast::<Context>() };
    let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");

    let worker = Worker::new(1024);
    let my_stealer = worker.stealer();

    // Stealer 指针存入共享数组
    unsafe {
        (*ctx.stealers_raw)[core_id] = stealer_to_ptr(my_stealer);
    }

    ALL_CORE_READY.wait();

    let start_tick = time();
    let mut ops: u64 = 0;

    let tps = TICKS_PER_SECOND.load(Ordering::Acquire);
    let duration_ticks = tps * 2;

    while time() - start_tick < duration_ticks {
        // 生产与消费逻辑
        for _ in 0..10 {
            let _ = worker.push(ops as usize);
            ops += 1;
        }

        // 尝试从相邻的核心窃取任务
        let target_id = (core_id + 1) % ctx.num_cores;
        let target_ptr = unsafe { (*ctx.stealers_raw)[target_id] };

        if target_ptr != 0 {
            let target_stealer = ptr_to_stealer_ref::<usize>(target_ptr);
            let _ = target_stealer.steal(&worker, |n| n / 2);
        }

        // 消耗掉
        while let Some(t) = worker.pop() {
            core::hint::black_box(t);
            ops += 1;
        }
    }

    unsafe {
        (*ctx.scores)[core_id] = ops;
    }

    let actual_end_tick = time();
    let actual_elapsed_ticks = actual_end_tick - start_tick;

    // 等待所有核心完成压测
    ALL_CORE_READY.wait();

    // --- 4. 汇总汇报 (仅 Core 0) ---
    if core_id == 0 {
        let mut report = String::new();
        macro_rules! log {
            ($($arg:tt)*) => {
                println!($($arg)*);
                let _ = writeln!(report, $($arg)*);
            };
        }

        log!("\n--- ST3 Benchmark Results (Calibrated) ---");
        let mut total_ops: u64 = 0;
        for i in 0..ctx.num_cores {
            let score = unsafe { (*ctx.scores)[i] };
            log!("Core {}: {} ops", i, score);
            total_ops += score;
        }

        // 使用实际消耗的 Tick 数除以每秒 Tick 数，得到精准的秒数
        let actual_seconds = actual_elapsed_ticks as f64 / tps as f64;

        log!("-----------------------------");
        log!("Elapsed Time: {:.4} s", actual_seconds);
        log!("Total Throughput: {} ops", total_ops);

        // 吞吐量计算：(总操作数 / 实际秒数) / 10^6 = Mops/s
        let mops_per_sec = (total_ops as f64 / actual_seconds) / 1_000_000.0;
        log!("Throughput: {:.2} Mops/s", mops_per_sec);

        // 平均每个操作消耗的周期数
        log!("Avg Latency: {:.1} cycles/op", actual_elapsed_ticks as f64 / total_ops as f64);
        log!("-----------------------------\n");

        let fs = get_image_file_system(image_handle())
            .expect("Failed to get image file system");
        let mut fs = FileSystem::new(fs);

        fs.write(cstr16!("st3.txt"), report.as_bytes())
            .expect("Failed to write multi.txt");
    }
}