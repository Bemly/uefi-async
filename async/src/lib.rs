#![warn(unreachable_pub)]
#![no_main]
#![no_std]

/// pretty unsafe, but it works.
#[cfg(feature = "static")]
pub mod bss;

/// WIP
#[cfg(feature = "alloc")]
pub mod alloc;

/// WIP
#[cfg(feature = "global-allocator")]
pub mod global_allocator;