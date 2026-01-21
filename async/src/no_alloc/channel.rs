//! # Lock-Free MPMC Channel
//!
//! A high-performance, synchronous, multiple-producer multiple-consumer (MPMC)
//! channel designed for `no_std` environments like UEFI.
//!
//! This channel is built on top of a sequence-based lock-free queue, ensuring
//! thread-safety and multi-core scalability without the need for a global allocator.
//!
//! ### Usage
//! ```rust
//! // Define a static queue in the global or local scope
//! static SHARED_QUEUE: Queue<MyEvent, 64> = Queue::new();
//!
//! async fn example() {
//!     let (tx, rx) = channel(&SHARED_QUEUE);
//!
//!     // Send an item asynchronously
//!     tx.send(MyEvent::Ping).await.ok();
//!
//!     // Receive an item asynchronously
//!     let event = rx.recv().await;
//! }
//! ```

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use nblfq::Queue;
use pin_project::pin_project;

/// The sending side of a channel.
///
/// Senders can be cloned or shared across multiple cores/threads as long as
/// the underlying [`Queue`] is accessible.
pub struct Sender<'a, T, const N: usize> {
    queue: &'a Queue<T, N>,
}

/// The receiving side of a channel.
///
/// Receivers allow multiple consumers to pull data from the underlying queue.
/// The delivery is fair and handled by the atomic sequence logic of the queue.
pub struct Receiver<'a, T, const N: usize> {
    queue: &'a Queue<T, N>,
}

impl<'a, T, const N: usize> Sender<'a, T, N> {
    /// Attempts to send an item into the channel without blocking.
    ///
    /// # Errors
    ///
    /// Returns `Err(item)` if the underlying queue is full.
    pub fn try_send(&self, item: T) -> Result<(), T> {
        self.queue.enqueue(item)
    }

    /// Sends an item into the channel asynchronously.
    ///
    /// The returned future will remain in the `Pending` state if the queue is full,
    /// and will resolve once the item is successfully enqueued.
    ///
    /// # Important
    ///
    /// In the current implementation, this future relies on the executor to re-poll.
    /// In a high-contention multi-core environment, ensure your executor handles
    /// spin-loops or yields efficiently.
    pub fn send(&self, item: T) -> SendFuture<'_, 'a, T, N> {
        SendFuture {
            sender: self,
            item: Some(item),
        }
    }
}

impl<'a, T, const N: usize> Receiver<'a, T, N> {
    /// Attempts to receive an item from the channel without blocking.
    ///
    /// Returns `None` if the channel is currently empty.
    pub fn try_recv(&self) -> Option<T> {
        self.queue.dequeue()
    }

    /// Receives an item from the channel asynchronously.
    ///
    /// The returned future resolves to the item once it becomes available in the queue.
    pub fn recv(&self) -> RecvFuture<'_, 'a, T, N> {
        RecvFuture { receiver: self }
    }
}

// --- Async Adapters ---

/// A future that resolves when an item has been successfully sent.
#[pin_project]
pub struct SendFuture<'f, 'a, T, const N: usize> {
    sender: &'f Sender<'a, T, N>,
    item: Option<T>,
}

impl<'f, 'a, T, const N: usize> Future for SendFuture<'f, 'a, T, N> {
    type Output = Result<(), T>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        // Safety: The item is taken only during the poll and put back if Pending.
        let item = this.item.take().expect("Future polled after completion");

        match this.sender.try_send(item) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(e) => {
                *this.item = Some(e);
                Poll::Pending
            }
        }
    }
}

/// A future that resolves when an item is available to be received.
pub struct RecvFuture<'f, 'a, T, const N: usize> {
    receiver: &'f Receiver<'a, T, N>,
}

impl<'f, 'a, T, const N: usize> Future for RecvFuture<'f, 'a, T, N> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.receiver.try_recv() {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }
    }
}

/// Creates a new channel backed by the provided [`Queue`].
///
/// This function returns a tuple containing the [`Sender`] and [`Receiver`].
/// Since it uses a reference to the queue, the queue must outlive the channel handles.
pub fn channel<T, const N: usize>(queue: &Queue<T, N>) -> (Sender<'_, T, N>, Receiver<'_, T, N>) {
    (Sender { queue }, Receiver { queue })
}