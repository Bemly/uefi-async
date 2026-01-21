use uefi_async::nano_alloc::control::single::join;
use alloc::boxed::Box;
use alloc::vec;
use core::ffi::c_void;
use core::ptr::addr_of_mut;
use core::time::Duration;
use uefi::boot::{create_event, get_handle_for_protocol, open_protocol_exclusive, stall, EventType, Tpl};
use uefi::proto::pi::mp::MpServices;
use uefi::{println, Status};
use uefi_async::nano_alloc::time::{Timeout, WaitTimer, _Timeout, _WaitTimer};
use uefi_async::nano_alloc::{add, Executor, TaskNode};
use uefi_async::{tick, yield_now, Pacer, Skip, Yield, YIELD};
use uefi_async::nano_alloc::control::multiple::prelude::*;

#[repr(C)]
struct Context<'bemly_> {
    pub mp: &'bemly_ MpServices,
    pub num_cores: usize,
}

async fn calc_1() {}
async fn init_fs() -> Result<(), ()> {Ok(())}
async fn check_memory() -> Result<(), ()> {Ok(())}

async fn calc_2() {
    WaitTimer::from_ms(500).await;
    2.year().await;
    5.day().await;
    1.mins().await;
    80.ps().await;
    1.us().await;
    500.ms().await;
    20.fps().await;
    YIELD.await;
    Yield.await;
    yield_now().await;
    Skip(2).await;

    match Box::pin(calc_2()).timeout(Duration::from_secs(2).as_secs()).await { _ => () }
    match Timeout::new_pin(Box::pin(calc_2()), 500).await { _ => () }
    match Timeout::new(calc_1(), 300).await { _ => () }
    match calc_2().timeout(500).await { Ok(_) => {}, Err(_) => {} }

    join!(calc_1(), calc_1(), calc_1(), calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1()
        ,calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1()
        ,calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1()).await;
    match try_join!(init_fs(), check_memory(), init_fs(), check_memory(), init_fs()).await {
        Ok(()) => {},
        Err(_) => {}
    }
    let (a, _, c, .. ) = join_all!(init_fs(), check_memory(), init_fs(), check_memory(), init_fs()).await;
    match a { Ok(_) => {}, Err(_) => {} }; if c.is_ok() { _ = c.unwrap(); }

    [calc_1(), calc_1(), calc_1(), calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),
        calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),
        calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),
        calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1(),calc_1()].join().await;
    vec![calc_1(), calc_1(), calc_1(), calc_1()].join().await;
    match (init_fs(), check_memory(), init_fs(), check_memory(), init_fs(), check_memory(), init_fs(),
     check_memory(), init_fs(), check_memory(), init_fs(), check_memory(), init_fs(), check_memory(),
     init_fs()).try_join().await {
        Ok(((), (), (), (), (), (), (), (), (), (), (), (), mem, fs, _)) => {},
        Err(_) => {},
    }



    let mut pacer = Pacer::new(20);
    loop {
        pacer.burst(20).await;
        pacer.throttle().await;
        pacer.count_update(90).repeat().await;
        pacer.step(10, true).await;
    }
}

fn calc_sync(_core: usize) {}

extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() { return }
    let ctx = unsafe { &mut *arg.cast::<Context>() };
    let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");

    // usage_1();
    // usage_2();
    // usage_3(core_id);
    // usage_4(core_id);
    usage_5(core_id);
}

fn usage_1() {
    let mut executor = Executor::new();

    // believe rust llvm compile NRVO
    let fut1 = Box::pin(calc_1());
    let fut2 = Box::pin(calc_2());

    let mut fut1 = TaskNode::new(fut1, 0);
    let mut fut2 = TaskNode::new(fut2, 60);

    executor.add(&mut fut1);
    executor.add(&mut fut2);
    executor.run_forever();
}

fn usage_2() {
    Executor::new()
        .add(&mut TaskNode::new(Box::pin(calc_1()), 0))
        .add(&mut TaskNode::new(Box::pin(calc_2()), 60))
        .run_forever();
}

fn usage_3(core: usize) {
    let mut executor = Executor::new();

    let mut fut1 = TaskNode::new(Box::pin(calc_1()), 0);
    let mut fut2 = TaskNode::new(Box::pin(calc_2()), 0);

    executor.add(&mut fut1).add(&mut fut2);

    let mut cx = Executor::init_step();

    loop {
        calc_sync(core);
        executor.run_step(tick(), &mut cx)
    }
}

async fn af1() {}
async fn af2(_: usize) {}
async fn af3(_: usize, _:usize) {}

fn usage_4(core: usize) {
    let mut executor = Executor::new();

    add! ( executor => {
        0  -> af1(),
        60 -> af2(core),
    });

    executor.run_forever();
}

fn usage_5(core: usize) {
    let mut executor1 = Executor::new();
    let mut executor2 = Executor::new();
    let mut cx = Executor::init_step();
    let offset = 20;

    add! (
        executor1 => {
            0  -> af1(),
            60 -> af2(core),
        },
        executor2 => {
            // 防止下溢
            10u64.saturating_sub(offset) -> af3(core, core),
            30 + 10                      -> af1(),
        },
    );

    loop {
        calc_sync(core);
        executor1.run_step(tick(), &mut cx);
        executor2.run_step(tick(), &mut cx);
    }
}

pub fn task() -> Status {

    let mp = get_handle_for_protocol::<MpServices>()
        .expect("Failed to get MP services");
    let mp = open_protocol_exclusive::<MpServices>(mp)
        .expect("Failed to open MP services");
    let num_cores = mp.get_number_of_processors()
        .expect("Failed to get number of processors")
        .enabled;

    let mut ctx = Context {
        mp: &mp,
        num_cores,
    };
    let arg_ptr = addr_of_mut!(ctx).cast::<c_void>();

    let event = unsafe {
        create_event(EventType::empty(), Tpl::CALLBACK, None, None)
            .expect("Failed to create event")
    };

    if num_cores > 1 {
        let _ = mp.startup_all_aps(false, process, arg_ptr, Some(event), None);
    }
    process(arg_ptr);

    stall(Duration::from_hours(1));
    Status::SUCCESS
}

