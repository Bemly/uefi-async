use crate::no_alloc::task::{SafeFuture, TaskHeader, TaskPool};
use core::ptr::{drop_in_place, write};
use core::sync::atomic::{AtomicUsize, Ordering};
use num_enum::{FromPrimitive, IntoPrimitive};
use static_cell::StaticCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, FromPrimitive)] #[repr(u8)]
pub enum State {
    Free,           // 空槽
    Initialized,    // 槽使用
    Ready,          // 进入任务队列
    Running,        // 运行
    Yielded,        // 让出
    Waiting,        // 等待

    #[default]
    Unreachable,    // 不可达
}

#[derive(Debug)] #[repr(C)]
pub struct StaticFuture<F>(pub AtomicUsize, pub F);
impl<F: SafeFuture> StaticFuture<StaticCell<F>> {
    /// Lazy Initialization
    #[inline(always)] pub fn spawn(&'static self, future: fn() -> F) -> &'static mut F {
        match self.0.load(Ordering::Acquire) {
            0 => {
                let cell = self.1.uninit();
                self.0.store(cell as *const _ as usize, Ordering::Release);
                cell.write(future())
            }
            addr => {
                let cell = addr as *mut F;
                if cell.is_null() { panic!("Error StaticFuture!") }
                unsafe {
                    drop_in_place(cell);
                    write(cell, future());
                    &mut *cell
                }
            }
        }
    }
}

impl<F: SafeFuture, const N: usize> TaskPool<F, N> {
    #[inline(always)] pub fn spawn(&'static self, future: fn() -> F) {
        for slot in self.0.iter() {
            if slot.header.state.compare_exchange(
                State::Free.into(), State::Initialized.into(), Ordering::Acquire, Ordering::Relaxed
            ).is_err() { continue }
            
            let future = slot.future.spawn(future);
            let task_ptr = &slot.header as *const TaskHeader as *mut ();
            if let Err(e) = worker.push(task_ptr) {
                
            }
            return
        }

        panic!("TaskPool capacity exceeded! No empty slots available.");
    }
}
// impl<F: SafeFuture> Deref for StaticFuture<StaticCell<F>> {
//     type Target = F;
//     #[inline(always)] fn deref(&self) -> &Self::Target {
//         let addr = self.0.load(Ordering::Acquire) as *const F;
//         if addr.is_null() { panic!("Attempted to deref an uninitialized StaticFuture!") }
//         unsafe { &*addr }
//     }
// }
// impl<F: SafeFuture> DerefMut for StaticFuture<StaticCell<F>> {
//     #[inline(always)] fn deref_mut(&mut self) -> &mut Self::Target {
//         let addr = self.0.load(Ordering::Acquire) as *mut F;
//         if addr.is_null() { panic!("Attempted to deref mut an uninitialized StaticFuture!") }
//         unsafe { &mut *addr }
//     }
// }