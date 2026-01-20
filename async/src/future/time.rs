// use core::pin::Pin;
// use core::task::{Context, Poll};
// use crate::util::tick;

pub struct Timeout<F> {
    future: F,
    deadline: u64,
}

// impl<F: Future> Future for Timeout<F> {
//     type Output = Result<F::Output, ()>;
//
//     fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
//         // 1. 检查是否超时
//         if tick() >= self.deadline {
//             return Poll::Ready(Err(()));
//         }
//         // 2. 推进内部 Future
//         match self.future.poll(cx) {
//             Poll::Ready(val) => Poll::Ready(Ok(val)),
//             Poll::Pending => Poll::Pending,
//         }
//     }
// }