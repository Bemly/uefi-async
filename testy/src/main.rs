#![no_main]
#![no_std]

use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::addr_of_mut;
use core::time::Duration;
use uefi::{entry, println, Status};
use uefi::boot::{create_event, get_handle_for_protocol, open_protocol_exclusive, stall, EventType, Tpl};
use uefi::proto::pi::mp::MpServices;
use uefi_async::{task_pool_align, task_pool_new, task_pool_size, TaskPoolLayout};
use uefi_async_macros::task;
// use uefi_async::executor::init_executor;

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
    let core_info = ctx.mp.get_processor_info(core_id).expect("Failed to get processor info");
    // 1. 获取专属执行器
    // let executor = init_executor(core_id);
    //
    // // 2. 构造一个基础 Waker
    // // 在 UEFI 简单实现中，我们可以先用一个“不执行任何操作”的 Dummy Waker
    // // 因为我们的任务是通过外部事件或轮询重新入队的
    // let waker = unsafe { todo!() };
    //
    // // 3. 进入执行主循环
    // loop {
    //     // 尝试运行一个任务
    //     let has_work = executor.run_step(&waker);
    //
    //     if !has_work {
    //         // 4. 如果没有任务，执行 CPU 放松指令，防止过度竞争总线
    //         core::hint::spin_loop();
    //
    //         // 如果你需要响应 UEFI 的停止信号，可以在这里检查标志位
    //         // if ctx.finished.load(core::sync::atomic::Ordering::Relaxed) {
    //         //     break;
    //         // }
    //     }
    // }
}


async fn wadawd() -> ! {
    loop {
        println!("Hello, world!");
    }
}

#[task]
async fn aaa() {
    loop {
        println!("Hello, world!");
    }
}
fn __www_task() -> impl Future<Output = ()> {
    async fn __www_task_inner_function() {
        loop {
            ::uefi::helpers::_print(
                format_args!("{0}{1}", format_args!("Hello, world!"), "\n"),
            );
        }
    }
    __www_task_inner_function()
}
fn www() {
    const fn __task
    const POOL_SIZE: usize = 1usize;
    static POOL: TaskPoolLayout<
        { task_pool_size::<_, _, _, POOL_SIZE>(__www_task) },
        { task_pool_align::<_, _, _, POOL_SIZE>(__www_task) },
    > = unsafe { transmute(task_pool_new::<_, _, _, POOL_SIZE>(__www_task))};
}

// #[doc(hidden)]
// fn __awd_task() -> impl ::core::future::Future<Output = ()> {
//     async fn __awd_task_inner_function() {
//         let a = 2;
//     }
//     { __awd_task_inner_function() }
// }
// fn awd() {
//     // const fn __task_pool_get<F, Args, Fut>(
//     //     _: F,
//     // ) -> &'static ::embassy_executor::raw::TaskPool<Fut, POOL_SIZE>
//     // where
//     //     F: ::embassy_executor::_export::TaskFn<Args, Fut = Fut>,
//     //     Fut: ::core::future::Future + 'static,
//     // {
//     //     unsafe { &*POOL.get().cast() }
//     // }
//     const POOL_SIZE: usize = 1;
//     static POOL: ::embassy_executor::_export::TaskPoolHolder<
//         {
//             ::embassy_executor::_export::task_pool_size::<_, _, _, POOL_SIZE>(__awd_task)
//         },
//         {
//             ::embassy_executor::_export::task_pool_align::<
//                 _,
//                 _,
//                 _,
//                 POOL_SIZE,
//             >(__awd_task)
//         },
//     > = unsafe {
//         ::core::mem::transmute(
//             ::embassy_executor::_export::task_pool_new::<_, _, _, POOL_SIZE>(__awd_task),
//         )
//     };
//     // unsafe { __task_pool_get(__awd_task)._spawn_async_fn(move || __awd_task()) }
// }


#[entry]
fn main() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");


    // let mp = get_handle_for_protocol::<MpServices>()
    //     .expect("Failed to get MP services");
    // let mp = open_protocol_exclusive::<MpServices>(mp)
    //     .expect("Failed to open MP services");
    // let num_cores = mp.get_number_of_processors()
    //     .expect("Failed to get number of processors")
    //     .enabled;
    //
    // let mut ctx = Context {
    //     mp: &mp,
    //     num_cores,
    // };
    // let arg_ptr = addr_of_mut!(ctx).cast::<c_void>();
    //
    // let event = unsafe {
    //     create_event(EventType::empty(), Tpl::CALLBACK, None, None)
    //         .expect("Failed to create event")
    // };
    //
    // if num_cores > 1 {
    //     let _ = mp.startup_all_aps(false, process, arg_ptr, Some(event), None);
    // }
    // process(arg_ptr);

    stall(Duration::from_hours(1));
    Status::SUCCESS
}

