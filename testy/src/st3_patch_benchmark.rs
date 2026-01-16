use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::fmt::Write;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;

use uefi::boot::{create_event, get_handle_for_protocol, get_image_file_system, image_handle, open_protocol_exclusive, stall, EventType, Tpl};
use uefi::fs::FileSystem;
use uefi::proto::pi::mp::MpServices;
use uefi::{cstr16, println};
use uefi_async::bss::lifo::{Queue, Stealer, Worker};

const QUEUE_SIZE: usize = 1024;
static TICKS_PER_SECOND: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct Context<'a> {
    pub mp: &'a MpServices,
    pub num_cores: usize,
    pub stealers_raw: *mut [Option<Stealer<QUEUE_SIZE>>; 64],
    pub scores: *mut [u64; 64],
    pub queues: &'static [&'static Queue<QUEUE_SIZE>],
}

#[inline(always)]
fn time() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}
fn calibrate_tsc() {
    let start = time();
    stall(Duration::from_micros(100_000)); // 100ms
    let end = time();
    let ticks_per_100ms = end - start;
    TICKS_PER_SECOND.store(ticks_per_100ms * 10, Ordering::Release);
}
pub unsafe fn benchmark() {
    calibrate_tsc();

    let mp_handle = get_handle_for_protocol::<MpServices>().expect("Failed to find MpServices");
    let mp = open_protocol_exclusive::<MpServices>(mp_handle).expect("Failed to open MpServices");
    let num_cores = mp.get_number_of_processors().expect("Failed to get processors").enabled;
    let stealers_raw = Box::into_raw(Box::new([None; 64]));
    let scores = Box::into_raw(Box::new([0u64; 64]));
    let mut queues_vec = Vec::with_capacity(num_cores);
    for _ in 0..num_cores {
        queues_vec.push(&*Box::leak(Box::new(Queue::<QUEUE_SIZE>::new())));
    }
    let queues_ref: &'static [&'static Queue<QUEUE_SIZE>] = Box::leak(queues_vec.into_boxed_slice());

    let mut ctx = Context {
        mp: &mp,
        num_cores,
        stealers_raw,
        scores,
        queues: queues_ref,
    };

    let arg_ptr = addr_of_mut!(ctx).cast::<c_void>();
    let event = unsafe {
        create_event(EventType::empty(), Tpl::CALLBACK, None, None).expect("Failed to create event")
    };

    println!("Starting Benchmark with {} cores...", num_cores);

    if num_cores > 1 {
        let _ = mp.startup_all_aps(false, process, arg_ptr, Some(event), None);
    }
    process(arg_ptr);
}

extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &*arg.cast::<Context>() };
    let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");
    let my_queue = ctx.queues[core_id];
    let worker = Worker::new(my_queue);
    unsafe {
        (*ctx.stealers_raw)[core_id] = Some(worker.stealer());
    }
    unsafe { core::arch::asm!("mfence") };
    stall(Duration::from_millis(50));

    let tps = TICKS_PER_SECOND.load(Ordering::Acquire);
    let start_tick = time();
    let mut ops: u64 = 0;
    let duration_ticks = tps * 2;
    while time() - start_tick < duration_ticks {
        for _ in 0..10 {
            let _ = worker.push(ops as *mut ());
            ops += 1;
        }
        let target_id = (core_id + 1) % ctx.num_cores;
        if let Some(target_stealer) = unsafe { (*ctx.stealers_raw)[target_id] } {
            let _ = target_stealer.steal(&worker, |n| n / 2);
        }
        while let Some(job) = worker.pop() {
            core::hint::black_box(job);
            ops += 1;
        }
    }
    unsafe {
        (*ctx.scores)[core_id] = ops;
    }

    let actual_elapsed = time() - start_tick;
    stall(Duration::from_millis(100));
    if core_id == 0 {
        print_and_save_report(ctx, actual_elapsed, tps);
    }
}

fn print_and_save_report(ctx: &Context, elapsed_ticks: u64, tps: u64) {
    let mut report = String::new();
    let mut total_ops: u64 = 0;

    println!("\n--- ST3 Static Queue Benchmark ---");
    let _ = writeln!(report, "ST3 Benchmark Results");

    for i in 0..ctx.num_cores {
        let s = unsafe { (*ctx.scores)[i] };
        println!("Core {}: {} ops", i, s);
        let _ = writeln!(report, "Core {}: {} ops", i, s);
        total_ops += s;
    }

    let actual_sec = elapsed_ticks as f64 / tps as f64;
    let mops = (total_ops as f64 / actual_sec) / 1_000_000.0;
    let latency = elapsed_ticks as f64 / total_ops as f64;

    println!("-----------------------------");
    println!("Total Ops: {}", total_ops);
    println!("Throughput: {:.2} Mops/s", mops);
    println!("Avg Latency: {:.1} cycles/op", latency);
    println!("-----------------------------\n");

    let _ = writeln!(report, "Total Ops: {}", total_ops);
    let _ = writeln!(report, "Throughput: {:.2} Mops/s", mops);
    let _ = writeln!(report, "Avg Latency: {:.1} cycles/op", latency);
    if let Ok(fs_handle) = get_image_file_system(image_handle()) {
        let mut fs = FileSystem::new(fs_handle);
        let _ = fs.write(cstr16!("st3_bench.txt"), report.as_bytes());
        println!("Report saved to st3_bench.txt");
    }
}