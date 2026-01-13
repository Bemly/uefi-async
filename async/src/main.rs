#![no_main]
#![no_std]

use uefi_async_macros::ヽ;

#[ヽ(=v=)]
mod example_app {
    async fn master_setup() {}
    async fn agent_setup() {}
    async fn agent_main() {}
    fn agent_idle() {}
    fn on_panic() {}
    fn on_error() {}
    fn on_exit() {}
}
