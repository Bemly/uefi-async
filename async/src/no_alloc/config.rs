use crate::no_alloc::lifo::Worker;

pub trait ExecutorConfig {
    const CORE_SIZE: usize;
    const QUEUE_SIZE: usize;
}

// 4核特化配置
pub struct SmallConfig;
impl ExecutorConfig for SmallConfig {
    const CORE_SIZE: usize = 4;
    const QUEUE_SIZE: usize = 256;
}

// 16核特化配置
pub struct LargeConfig;
impl ExecutorConfig for LargeConfig {
    const CORE_SIZE: usize = 16;
    const QUEUE_SIZE: usize = 128; // 核心多时可以适当减小单核队列
}

macro_rules! register_config {
    ($config_type:ty) => {};
}

// TODO: 注册特化
register_config!(SmallConfig);
register_config!(LargeConfig);