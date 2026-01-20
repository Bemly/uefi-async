use alloc::boxed::Box;
use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::addr_of_mut;
use core::time::Duration;
use uefi::boot::{create_event, get_handle_for_protocol, open_protocol_exclusive, stall, EventType, Tpl};
use uefi::proto::pi::mp::MpServices;
use uefi::Status;
use uefi_async::nano_alloc::{Executor, TaskNode};
use uefi_async::no_alloc::task::{SafeFuture, TaskCapture, TaskFn, TaskHeader, TaskPool, TaskPoolLayout};

#[repr(C)]
struct Context<'bemly_> {
    pub mp: &'bemly_ MpServices,
    pub num_cores: usize,
}

async fn calc_1() {
    let mut i = 0;
    i += 1;
}

async fn calc_2() {
    let mut i = 0;
    i -= 1;
}

extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &mut *arg.cast::<Context>() };

    let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");

    let mut exec = Executor::new();

    let fut1 = Box::pin(calc_1());
    let mut fut1 = TaskNode::new(fut1, 0);
    let fut2 = Box::pin(calc_2());
    let mut fut2 = TaskNode::new(fut2, 60);



    exec.add(&mut fut1);
    exec.add(&mut fut2);


    // exec.add(&mut fut1).add(&mut fut2).run();
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

