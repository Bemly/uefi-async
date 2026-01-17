#![no_main]
#![no_std]
extern crate alloc;

use core::time::Duration;
use uefi::boot::stall;
use uefi::{entry, Status};
use uefi_async::global_allocator::alloc_init_wrapper;

// mod task_v1;
mod st3_benchmark;
mod st3_patch_benchmark;
mod task_v2;
// mod _template;

#[entry]
fn main() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");

    alloc_init_wrapper();

    // template
    // _template::template();

    // benchmark
    // unsafe { st3_benchmark::benchmark(); }         // 67 cycles :)
    // unsafe { st3_patch_benchmark::benchmark(); }   // 84 acycles :(

    // test
    // task_v1::task_fn();
    task_v2::task_fn();

    stall(Duration::from_hours(1));
    Status::SUCCESS
}
