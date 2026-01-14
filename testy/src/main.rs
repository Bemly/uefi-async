#![no_main]
#![no_std]

use core::time::Duration;
use uefi::{entry, Status};
use uefi::boot::stall;
use uefi_async_macros::task;

// use uefi_async_macros::ヽ;
// use uefi_async_macros::ヽ as Caillo;
// Ciallo～(∠・ω< )⌒☆

// #[ヽ('ε')]
// mod example_app {
//     async fn master_setup() {}
//     async fn agent_setup() {}
//     async fn agent_main() {}
//     fn agent_idle() {}
//     fn on_panic() {}
//     fn on_error() {}
//     fn on_exit() {}
// }


#[task]
async fn task() {

}

#[entry]
fn main() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");

    stall(Duration::from_hours(1));
    Status::SUCCESS
}

// #[repr(C)]
// struct Context<'bemly_> {
//     pub mp: &'bemly_ MpServices,
//     pub num_cores: usize,
// }
//
// extern "efiapi" fn process(arg: *mut c_void) {
//     if arg.is_null() { return; }
//     let ctx = unsafe { &mut *arg.cast::<Context>() };
//
//     let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");
//     let core_info = ctx.mp.get_processor_info(core_id).expect("Failed to get processor info");
//     let executor = get_executor_for_core(core_id);
//     executor.run_loop();
// }
//
// #[entry]
// fn main() -> Status {
//     uefi::helpers::init().expect("Failed to init UEFI");
//
//
//
//     let mp = get_handle_for_protocol::<MpServices>()
//         .expect("Failed to get MP services");
//     let mp = open_protocol_exclusive::<MpServices>(mp)
//         .expect("Failed to open MP services");
//     let num_cores = mp.get_number_of_processors()
//         .expect("Failed to get number of processors")
//         .enabled;
//
//     let mut ctx = Context {
//         mp: &mp,
//         num_cores,
//     };
//     let arg_ptr = addr_of_mut!(ctx).cast::<c_void>();
//
//     let event = unsafe {
//         create_event(EventType::empty(), Tpl::CALLBACK, None, None)
//             .expect("Failed to create event")
//     };
//
//     if num_cores > 1 {
//         let _ = mp.startup_all_aps(false, process, arg_ptr, Some(event), None);
//     }
//     process(arg_ptr);
//
//     stall(Duration::from_hours(1));
//     Status::SUCCESS
// }