#![no_main]
#![no_std]

extern crate alloc;

mod st3;

use core::time::Duration;
use uefi::boot::stall;
use uefi::prelude::*;
use uefi::proto::pi::mp::MpServices;
// use uefi_async_macros::ヽ;
//
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

mod example_app {
    use core::cell::UnsafeCell;
    use core::ffi::c_void;
    use core::ptr::addr_of_mut;
    use core::sync::atomic::AtomicUsize;
    use core::time::Duration;
    use uefi::{entry, Status};
    use uefi::boot::{create_event, get_handle_for_protocol, open_protocol_exclusive, stall, EventType, Tpl};
    use uefi::proto::pi::mp::MpServices;

    async fn master_setup() {}
    async fn agent_setup() {}
    async fn agent_main() {}
    fn agent_idle() {}
    fn on_panic() {}
    fn on_error() {}
    fn on_exit() {}

    const STATE_EMPTY: usize = 0;
    const STATE_PENDING: usize = 1;
    const STATE_RUNNING: usize = 2;
    const STATE_DONE: usize = 3;

    pub struct StaticTask<F: Future> {
        state: AtomicUsize,
        future: UnsafeCell<Option<F>>, // 实际存储 Future 状态机
    }

    // 安全性：UEFI 多核环境下需要保证 Send/Sync
    unsafe impl<F: Future + Send> Sync for StaticTask<F> {}

    #[repr(C)]
    struct Context<'bemly_> {
        pub mp: &'bemly_ MpServices,
        pub num_cores: usize,
    }


    extern "efiapi" fn process(arg: *mut c_void) {
        if arg.is_null() { return; }
        let ctx = unsafe { &mut *arg.cast::<Context>() };

        let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");
        let core_info = ctx.mp.get_processor_info(core_id).expect("Failed to get processor info");


        // 获取当前核心的本地任务视图（这里需要一个全局静态数组）
        loop {
            // // 1. 尝试寻找属于自己的任务
            // if let Some(task) = find_local_task(core_id) {
            //     poll_task(task);
            // }
            // // 2. 尝试从相邻核心窃取
            // else if let Some(stolen_task) = try_steal(core_id, ctx.num_cores) {
            //     poll_task(stolen_task);
            // }
            // // 3. 协作式闲置
            // else {
            //     agent_idle(); // 调用你的 idle 函数，可以在里面执行 pause 指令省电
            // }
        }
    }

    #[entry]
    fn main() -> Status {
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
}



