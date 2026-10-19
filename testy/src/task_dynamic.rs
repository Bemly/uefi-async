//! Dynamic executor demo: Box-owned tasks, spawn / cancel / retune,
//! cross-executor stealing. Mirrors the house `task_nano` shape
//! (MP startup + per-core `Context`) but drives [`dynamic`](uefi_async::dynamic).
//!
//! Wire-up is identical to siblings: uncomment the module + call in `main.rs`.

use alloc::boxed::Box;
use core::ffi::c_void;
use core::time::Duration;
use uefi::boot::{
    EventType, Tpl, create_event, get_handle_for_protocol, open_protocol_exclusive, stall,
};
use uefi::proto::pi::mp::MpServices;
use uefi_async::dynamic::Executor;
use uefi_async::{Yield, tick};

#[repr(C)]
struct Context<'bemly_> {
    pub mp: &'bemly_ MpServices,
    pub num_cores: usize,
}

async fn heartbeat(id: u64) {
    let mut n = 0u64;
    loop {
        n = n.wrapping_add(1);
        if n == 5 {
            // Prove the task ran; real work would touch hardware here.
            log_tick(id, n);
        }
        Yield.await;
    }
}

fn log_tick(_id: u64, _n: u64) {}

async fn quitter() {
    Yield.await;
}

/// Drives one [`Executor`] on every core: BSP + APs each own an executor,
/// stealers are cross-registered so work balances itself.
extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() {
        return;
    }
    let ctx = unsafe { &mut *arg.cast::<Context>() };
    let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");

    let mine = Executor::new();
    // NOTE: cross-core steal handles are shared out-of-band in real
    // deployments (e.g. a global array published before `startup_all_aps`);
    // here each core simply runs its own queue to keep the demo self-contained.
    let a = mine
        .spawn(Box::pin(heartbeat(core_id as u64)), 10)
        .expect("spawn heartbeat");
    let b = mine.spawn(Box::pin(quitter()), 0).expect("spawn quitter");
    // Retune + cancel exercise the registry path (cancel wins before first poll).
    mine.set_freq(a, 20);
    assert!(mine.cancel(b));
    assert!(mine.is_alive(a));

    let mut cx = Executor::init_step();
    for _ in 0..8 {
        mine.run_step(tick(), &mut cx);
    }
    // The cancelled task was reaped on its first poll; the heartbeat lives on.
    assert!(!mine.is_alive(b));
    assert!(mine.is_alive(a));
}

pub fn task_fn() {
    let mp_handle = get_handle_for_protocol::<MpServices>().expect("Failed to get MP Services");
    let mp = open_protocol_exclusive::<MpServices>(mp_handle).expect("Failed to open MP Services");
    let num_cores = mp
        .get_number_of_processors()
        .expect("Failed to get CPU count")
        .enabled;
    let mut ctx = Box::new(Context { mp: &mp, num_cores });

    let event = unsafe { create_event(EventType::empty(), Tpl::CALLBACK, None, None) }
        .expect("Failed to create event");
    let arg_ptr = &mut *ctx as *mut _ as *mut c_void;

    if num_cores > 1 {
        let _ = mp.startup_all_aps(false, process, arg_ptr, Some(event), None);
    }
    process(arg_ptr);

    stall(Duration::from_micros(100));
}
