#![no_main]
#![no_std]

extern crate alloc;

mod st3;
mod executor;

use core::time::Duration;
use uefi::boot::stall;
use uefi::prelude::*;
use uefi::proto::pi::mp::MpServices;
// use uefi_async_macros::ヽ;
// use uefi_async_macros::ヽ as Caillo;
// Ciallo～(∠・ω< )⌒☆

// #[ヽ('ε')]
// mod example_app {
//     async fn master_setup() {}
//     async fn agent_setup() {}
//     async fn agent_main() {}
//     fn agent_idle() {}
//     fn on_panic() {}
//     fn on_error() {}
//     fn on_exit() {}
// }
