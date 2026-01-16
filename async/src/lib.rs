#![warn(unreachable_pub)]
#![no_main]
#![no_std]

#[cfg(feature = "no-alloc")]
pub mod bss;

#[cfg(feature = "alloc")]
pub mod alloc;