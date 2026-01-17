use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use crate::no_alloc::task::{SafeFuture, TaskHeader, TaskPool, TaskSlot};
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
pub struct StaticFuture<F>(pub UnsafeCell<usize>, pub F);
impl<F: SafeFuture> StaticFuture<StaticCell<F>> {
    #[inline(always)] pub const fn new() -> Self { Self(UnsafeCell::new(0), StaticCell::new()) }
    /// Lazy Initialization
    #[inline(always)] pub fn init(&'static self, future: impl FnOnce() -> F) -> &'static mut F {
        let addr = unsafe { &mut *self.0.get() };
        match *addr {
            0 => {
                let cell = self.1.init_with(future);
                *addr = cell as *const _ as usize;
                cell
            }
            _ => {
                let cell = *addr as *mut F;
                if cell.is_null() { panic!("Error Struct: StaticFuture!") }
                unsafe {
                    drop_in_place(cell);
                    write(cell, future());
                    &mut *cell
                }
            }
        }
    }
}
impl<F: SafeFuture> Deref for StaticFuture<StaticCell<F>> {
    type Target = F;
    #[inline(always)] fn deref(&self) -> &Self::Target {
        unsafe { &*(*self.0.get() as *const F) }
    }
}

impl<F: SafeFuture, const N: usize> TaskPool<F, N> {
    #[inline(always)] pub fn spawn(&'static self, future: impl FnOnce() -> F) {
        for slot in self.0.iter() {
            if slot.header.state.compare_exchange(
                State::Free.into(), State::Initialized.into(), Ordering::Acquire, Ordering::Relaxed
            ).is_err() { continue }

            let future = slot.future.init(future);
            let task_ptr = &slot.header as *const TaskHeader as *mut ();
            todo!();
            return
        }

        panic!("TaskPool capacity exceeded! No empty slots available.");
    }
}
