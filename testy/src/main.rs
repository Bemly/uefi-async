#![no_main]
#![no_std]
extern crate alloc;

use core::time::Duration;
use uefi::boot::stall;
use uefi::{entry, Status};
use uefi_async::global_allocator::alloc_init_wrapper;

// mod task;
mod st3_benchmark;
mod st3_patch_benchmark;

#[entry]
fn main() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");

    alloc_init_wrapper();

    // unsafe { st3_benchmark::benchmark(); }         // 67 cycles :)
    // unsafe { st3_patch_benchmark::benchmark(); }   // 84 acycles :(


    stall(Duration::from_hours(1));
    Status::SUCCESS
}
