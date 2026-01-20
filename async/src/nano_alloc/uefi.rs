use core::pin::Pin;
use core::task::{Context, Poll};
use uefi::proto::console::text::{Input, Key};

pub struct KeyInputFuture<'a> {
    text_input: &'a mut Input,
}

impl<'a> Future for KeyInputFuture<'a> {
    type Output = Key;

    fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        match self.text_input.read_key() {
            Ok(Some(key)) => Poll::Ready(key),
            _ => Poll::Pending,
        }
    }
}