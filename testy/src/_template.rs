use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::addr_of_mut;
use core::time::Duration;
use uefi::boot::{create_event, get_handle_for_protocol, open_protocol_exclusive, stall, EventType, Tpl};
use uefi::proto::pi::mp::MpServices;
use uefi::Status;
use uefi_async::bss::task::{SafeFuture, TaskCapture, TaskFn, TaskPool, TaskPoolLayout};

// use uefi_async_macros::ヽ;
// use uefi_async_macros::ヽ as Caillo;
// Ciallo～(∠・ω< )⌒☆

// #[ヽ('ε')]
// mod example_app {
//     fn master_setup() {}
//     fn agent_setup() {}
//     fn agent_main() {}
//     fn agent_idle() {}
//     fn on_panic() {}
//     fn on_error() {}
//     fn on_exit() {}
// }

#[repr(C)]
struct Context<'bemly_> {
    pub mp: &'bemly_ MpServices,
    pub num_cores: usize,
}

extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &mut *arg.cast::<Context>() };

    let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");
}

#[doc(hidden)]
fn __async_fun() -> impl Future<Output = ()> { ( move || async move {})() }
fn async_fun() {
    const POOL_SIZE: usize = 4;
    static POOL: TaskPoolLayout<{ TaskCapture::<_, _>::size::<POOL_SIZE>(__async_fun) }> = unsafe {
        transmute(TaskCapture::<_,_>::new::<POOL_SIZE>(__async_fun))
    };
    const fn get<F, Args, Fut>(_: F) -> &'static TaskPool<Fut, POOL_SIZE>
    where F: TaskFn<Args, Fut = Fut>, Fut: SafeFuture {
        const {
            assert_eq!(size_of::<TaskPool<Fut, POOL_SIZE>>(), size_of_val(&POOL));
            assert!(align_of::<TaskPool<Fut, POOL_SIZE>>() <= 128);
        }
        unsafe { &*POOL.get().cast() }
    }
    get(__async_fun);
}


pub fn template() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");


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

