#![no_main]
#![no_std]
extern crate alloc;

use alloc::vec;
use core::time::Duration;
use uefi::boot::stall;
use uefi::{entry, println, Status};
// mod task;
mod st3_benchmark;
mod talc;
mod st3_patch_benchmark;

#[entry]
fn main() -> Status {
    uefi::helpers::init().expect("Failed to init UEFI");

    talc::alloc_init_wrapper();

    // unsafe { st3_benchmark::benchmark(); }         // 67 cycles :)
    // unsafe { st3_patch_benchmark::benchmark(); }   // 84 acycles :(


    stall(Duration::from_hours(1));
    Status::SUCCESS
}
