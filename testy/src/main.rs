#![no_main]
#![no_std]

use core::ffi::c_void;
use core::mem::transmute;
use core::ptr;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use core::task::{RawWaker, Waker};
use core::time::Duration;
use uefi::boot::{create_event, get_handle_for_protocol, open_protocol_exclusive, stall, EventType, Tpl};
use uefi::proto::pi::mp::MpServices;
use uefi::{entry, println, Status};
use uefi_async::executor::init_executor;
use uefi_async::sleep::sleep_ms;
use uefi_async::st3::lifo::Worker;
use uefi_async::task::{SafeFuture, TaskCapture, TaskFn, TaskPool, TaskPoolLayout};
use uefi_async::waker::VTABLE;
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

pub struct SpinLock(AtomicBool);

impl SpinLock {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn lock(&self) {
        // 循环尝试将 false 改为 true
        while self.0.compare_exchange_weak(
            false, true, Ordering::Acquire, Ordering::Relaxed
        ).is_err() {
            core::hint::spin_loop();
        }
    }

    pub fn unlock(&self) {
        self.0.store(false, Ordering::Release);
    }
}

// 定义一个全局控制台锁
static PRINT_LOCK: SpinLock = SpinLock::new();

#[derive(Copy, Clone)]
pub struct SendPtr<T>(pub *const T);

// 手动标记为 Send 和 Sync
// 安全性声明：在 UEFI 环境下，协议指针是全局唯一的，跨核访问是物理安全的
unsafe impl<T> Send for SendPtr<T> {}
unsafe impl<T> Sync for SendPtr<T> {}

impl<T> SendPtr<T> {
    pub unsafe fn as_ref(&self) -> &T {
        &*self.0
    }
}


#[repr(C)]
struct Context<'bemly_> {
    pub mp: &'bemly_ MpServices,
    pub num_cores: usize,
}

extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() { return; }
    let ctx = unsafe { &mut *arg.cast::<Context>() };

    let core_id = ctx.mp.who_am_i().expect("Failed to get core ID");
    // 1. 获取专属执行器
    let mut executor = init_executor(core_id);

    let mut worker = &mut executor.worker;

    www(&mut worker, core_id, ctx.mp);

    let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &VTABLE)) };
    // 3. 进入执行主循环
    loop {

        let has_work = executor.run_step(&waker);
        if !has_work {
            core::hint::spin_loop();
        }
    }
}


static RANG: AtomicUsize = AtomicUsize::new(0);

fn __www_task(a: usize, core_id: usize, mp_ptr: SendPtr<MpServices>) -> impl Future<Output = ()> {
    async fn __www_task_inner_function(a: usize, core_id: usize, mp_ptr: SendPtr<MpServices>) {
        let mut time = 1;
        let mp = unsafe { mp_ptr.as_ref() };

        loop {
            if RANG.load(Ordering::SeqCst) == core_id {
                continue;
            }
            PRINT_LOCK.lock();
            let current_id = mp.who_am_i().unwrap_or(core_id);
            println!("[Core {}] Real ID: {} | val: {}", core_id, current_id, a);
            RANG.store(current_id, Ordering::SeqCst);
            PRINT_LOCK.unlock();

            sleep_ms(1000).await;
            // stall(Duration::from_secs(1))
        }
    }
    __www_task_inner_function(a, core_id, mp_ptr)
}
fn www(worker: &Worker<256>, core_id: usize, mp: &MpServices) {
    const POOL_SIZE: usize = 4;
    static POOL: TaskPoolLayout<{ TaskCapture::<_, _>::size::<POOL_SIZE>(__www_task) }> = TaskPoolLayout::new();
    static INIT: AtomicBool = AtomicBool::new(false);
    let pool_ptr = POOL.0.get() as *mut TaskPool<_, POOL_SIZE>;
    if INIT.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        unsafe { core::ptr::write(pool_ptr, TaskPool::new()); }
    }

    unsafe {
        let pool = &*pool_ptr;
        let task_gen = __www_task;
        let _ = worker.spawn_task(pool, task_gen(20, core_id, SendPtr(mp as *const _)));
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

