#![no_main]
#![no_std]

extern crate alloc;

mod st3;
mod executor;
mod task;



pub use crate::task::{
    TaskSlot, TaskPool, TaskPoolLayout, TaskFn, task_pool_size, task_pool_align, task_pool_new
};




pub struct SpawnToken {
    pub task_ptr: *const (),
    pub task_id: usize,
}

impl SpawnToken {
    pub fn new(task_ptr: *const (), task_id: usize) -> Self {
        Self { task_ptr, task_id }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SpawnError {
    PoolFull,
    ExecutorClosed,
    Unknown,
}

