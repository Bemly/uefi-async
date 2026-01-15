#![warn(unreachable_pub)]
#![no_main]
#![no_std]

extern crate alloc;

pub mod st3;
pub mod executor;
pub mod task;
pub mod waker;
pub mod sleep;

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

