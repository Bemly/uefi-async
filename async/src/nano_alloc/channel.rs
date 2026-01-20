use core::cell::RefCell;
use alloc::rc::Rc;
use core::pin::Pin;
use core::task::{Context, Poll};

pub struct Oneshot<Data> (Rc<RefCell<Option<Data>>>);
pub struct Receiver<Data> (Rc<RefCell<Option<Data>>>);

impl<Data> Oneshot<Data> {
    pub fn new() -> (Self, Receiver<Data>) {
        let data = Rc::new(RefCell::new(None));
        (Self(data.clone()), Receiver(data))
    }

    pub fn send(self, value: Data) { *self.0.borrow_mut() = Some(value) }
}

impl<Data> Future for Receiver<Data> {
    type Output = Data;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Data> {
        match self.0.borrow_mut().take() {
            Some(val) => Poll::Ready(val),
            None => Poll::Pending,
        }
    }
}