use crate::no_alloc::lifo::Worker;

pub trait ExecutorConfig {
    const MAX_CORES: usize;
    const QUEUE_SIZE: usize;
}

// 4核特化配置
pub struct SmallConfig;
impl ExecutorConfig for SmallConfig {
    const MAX_CORES: usize = 4;
    const QUEUE_SIZE: usize = 256;
}

// 16核特化配置
pub struct LargeConfig;
impl ExecutorConfig for LargeConfig {
    const MAX_CORES: usize = 16;
    const QUEUE_SIZE: usize = 128; // 核心多时可以适当减小单核队列
}

pub struct Executor<T: ExecutorConfig>
where
    [(); T::QUEUE_SIZE]: , // 这是一个复杂的约束，用于告诉编译器这个大小是合法的
{
    pub worker: Worker<{ T::QUEUE_SIZE }>,
    pub core_id: usize,
}