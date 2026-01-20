use core::cell::Cell;
use core::task::{Poll, Context};
use core::future::Future;
use core::pin::Pin;

pub struct Signal (Cell<bool>); // triggered

impl Signal {
    pub const fn new() -> Self { Self(Cell::new(false)) }
    pub fn fire(&self) { self.0.set(true) }
    pub fn wait(&self) -> SignalFuture<'_> { SignalFuture(self) }
}

pub struct SignalFuture<'bemly_> (&'bemly_ Signal); // signal

impl Future for SignalFuture<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.0.0.get() { Poll::Ready(()) } else { Poll::Pending }
    }
}