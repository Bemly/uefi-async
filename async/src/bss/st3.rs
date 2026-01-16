//! # St³ — Stealing Static Stack (Patch!)
//!
//! Very fast lock-free, bounded, work-stealing queue with FIFO stealing and
//! LIFO or FIFO semantic for the worker thread.
//!
//! The `Worker` handle enables push and pop operations from a single thread,
//! while `Stealer` handles can be shared between threads to perform FIFO
//! batch-stealing operations.
//!
//! `St³` is effectively a faster, fixed-size alternative to the Chase-Lev
//! double-ended queue. It uses no atomic fences, much fewer atomic loads and
//! stores, and fewer Read-Modify-Write operations: none for `push`, one for
//! `pop` and one (LIFO) or two (FIFO) for `steal`.
//!
//! ## Example
//!
//! ```
//! use std::thread;
//! use st3::lifo::Worker;
//!
//! // Push 4 items into a queue of capacity 256.
//! let worker = Worker::new(256);
//! worker.push("a").unwrap();
//! worker.push("b").unwrap();
//! worker.push("c").unwrap();
//! worker.push("d").unwrap();
//!
//! // Steal items concurrently.
//! let stealer = worker.stealer();
//! let th = thread::spawn(move || {
//!     let other_worker = Worker::new(256);
//!
//!     // Try to steal half the items and return the actual count of stolen items.
//!     match stealer.steal(&other_worker, |n| n/2) {
//!         Ok(actual) => actual,
//!         Err(_) => 0,
//!     }
//! });
//!
//! // Pop items concurrently.
//! let mut pop_count = 0;
//! while worker.pop().is_some() {
//!     pop_count += 1;
//! }
//!
//! // Does it add up?
//! let steal_count = th.join().unwrap();
//! assert_eq!(pop_count + steal_count, 4);
//! ```
// #![warn(missing_docs, missing_debug_implementations, unreachable_pub)]
use core::fmt;
use core::iter::FusedIterator;
use core::mem::transmute;
use core::panic::{RefUnwindSafe, UnwindSafe};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64};
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};

/// Error returned when stealing is unsuccessful.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StealError {
    /// No item was stolen.
    Empty,
    /// Another concurrent stealing operation is ongoing.
    Busy,
}

impl fmt::Display for StealError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StealError::Empty => write!(f, "cannot steal from empty queue"),
            StealError::Busy => write!(f, "a concurrent steal operation is ongoing"),
        }
    }
}

#[inline]
/// Pack two short integers into a long one.
fn pack(value1: u32, value2: u32) -> u64 {
    ((value1 as u64) << u32::BITS) | value2 as u64
}
#[inline]
/// Unpack a long integer into 2 short ones.
fn unpack(value: u64) -> (u32, u32) {
    (
        (value >> u32::BITS) as u32,
        value as u32,
    )
}

#[derive(Debug)]
#[repr(C, align(128))]
pub struct Queue<const N: usize> {
    pub push_count: AtomicU32,
    pub stealer_data: StealerData,
    pub buffer: Ptr<N>,
}
#[derive(Debug)]
#[repr(C, align(128))]
pub struct Ptr<const N: usize> ([AtomicPtr<()>; N]);
impl<const N: usize> Ptr<N> {
    pub const fn new() -> Self {
        const EMPTY_PTR: AtomicPtr<()> = AtomicPtr::new(null_mut());
        Self([EMPTY_PTR; N])
    }
}
#[derive(Debug)]
#[repr(C, align(128))]
pub struct StealerData {
    pub pop_count_and_head: AtomicU64,
    pub head: AtomicU32,
}

impl<const N: usize> Queue<N> {
    const _CHECK_N: () = assert!(N.is_power_of_two(), "N must be a power of two");
    const MASK: u32 = (N - 1) as u32;
    pub const fn new() -> Self {
        Self {
            push_count: AtomicU32::new(0),
            stealer_data: StealerData {
                pop_count_and_head: AtomicU64::new(0),
                head: AtomicU32::new(0),
            },
            buffer: Ptr::new(),
        }
    }
    #[inline(always)]
    unsafe fn read_at(&self, position: u32) -> *mut () {
        let index = (position & Self::MASK) as usize;
        self.buffer.0[index].load(Acquire)
    }
    #[inline(always)]
    unsafe fn write_at(&self, position: u32, item: *mut ()) {
        let index = (position & Self::MASK) as usize;
        self.buffer.0[index].store(item, Release);
    }
    #[inline]
    fn book_items<C>(
        &self,
        mut count_fn: C,
        max_count: u32,
    ) -> Result<(u32, u32, u32), StealError>
    where
        C: FnMut(usize) -> usize,
    {
        let mut pop_count_and_head = self.stealer_data.pop_count_and_head.load(Acquire);
        let old_head = self.stealer_data.head.load(Acquire);
        loop {
            let (pop_count, head) = unpack(pop_count_and_head);
            if old_head != head {
                return Err(StealError::Busy);
            }
            let push_count = self.push_count.load(Acquire);
            let tail = push_count.wrapping_sub(pop_count);
            let item_count = tail.wrapping_sub(head);
            if item_count == 0 {
                return Err(StealError::Empty);
            }
            let count = (count_fn(item_count as usize).min(max_count as usize) as u32)
                .min(item_count);
            if count == 0 {
                return Err(StealError::Empty);
            }
            let new_head = head.wrapping_add(count);
            let new_pop_count_and_head = pack(pop_count, new_head);
            match self.stealer_data.pop_count_and_head.compare_exchange_weak(
                pop_count_and_head,
                new_pop_count_and_head,
                Acquire,
                Acquire,
            ) {
                Ok(_) => return Ok((head, new_head, count)),
                Err(current) => pop_count_and_head = current,
            }
        }
    }
}
#[derive(Debug)]
pub struct Worker<const N: usize> {
    pub queue: &'static Queue<N>,
}

impl<const N: usize> Worker<N> {
    pub const fn new(queue: &'static Queue<N>) -> Self {
        Worker { queue }
    }
    pub fn stealer(&self) -> Stealer<N> {
        Stealer {
            queue: self.queue,
        }
    }
    pub fn stealer_ref(&self) -> &Stealer<N> {
        unsafe { transmute::<&Self, &Stealer<N>>(self) }
    }
    pub fn spare_capacity(&self) -> usize {
        let push_count = self.queue.push_count.load(Relaxed);
        let pop_count = unpack(self.queue.stealer_data.pop_count_and_head.load(Relaxed)).0;
        let tail = push_count.wrapping_sub(pop_count);
        let head = self.queue.stealer_data.head.load(Relaxed);
        let len = tail.wrapping_sub(head);

        (N as u32 - len) as usize
    }
    pub fn is_empty(&self) -> bool {
        let push_count = self.queue.push_count.load(Relaxed);
        let (pop_count, head) = unpack(self.queue.stealer_data.pop_count_and_head.load(Relaxed));
        let tail = push_count.wrapping_sub(pop_count);

        tail == head
    }
    pub fn push(&self, item: *mut ()) -> Result<(), *mut ()> {
        let push_count = self.queue.push_count.load(Relaxed);
        let pop_count_and_head = self.queue.stealer_data.pop_count_and_head.load(Acquire);
        let (pop_count, head) = unpack(pop_count_and_head);
        let tail = push_count.wrapping_sub(pop_count);
        if tail.wrapping_sub(head) >= N as u32 {
            return Err(item);
        }
        unsafe { self.queue.write_at(tail, item) };
        self.queue.push_count.store(push_count.wrapping_add(1), Release);
        Ok(())
    }
    pub fn extend<I: IntoIterator<Item = *mut ()>>(&self, iter: I) {
        let push_count = self.queue.push_count.load(Relaxed);
        let pop_count = unpack(self.queue.stealer_data.pop_count_and_head.load(Relaxed)).0;
        let mut tail = push_count.wrapping_sub(pop_count);
        let head = self.queue.stealer_data.head.load(Acquire);

        let max_tail = head.wrapping_add(N as u32);
        for item in iter {
            if tail == max_tail {
                break;
            }
            unsafe { self.queue.write_at(tail, item) };
            tail = tail.wrapping_add(1);
        }
        self.queue
            .push_count
            .store(tail.wrapping_add(pop_count), Release);
    }
    pub fn pop(&self) -> Option<*mut ()> {
        let mut pop_count_and_head = self.queue.stealer_data.pop_count_and_head.load(Relaxed);
        let push_count = self.queue.push_count.load(Relaxed);
        let (pop_count, mut head) = unpack(pop_count_and_head);
        let tail = push_count.wrapping_sub(pop_count);
        let new_pop_count = pop_count.wrapping_add(1);

        loop {
            if tail == head {
                return None;
            }
            let new_pop_count_and_head = pack(new_pop_count, head);
            match self.queue.stealer_data.pop_count_and_head.compare_exchange_weak(
                pop_count_and_head,
                new_pop_count_and_head,
                Release,
                Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => {
                    pop_count_and_head = current;
                    head = unpack(current).1;
                }
            }
        }
        unsafe { Some(self.queue.read_at(tail.wrapping_sub(1))) }
    }
    pub fn drain<C>(&self, count_fn: C) -> Result<Drain<'_, N>, StealError>
    where C: FnMut(usize) -> usize {
        let (old_head, new_head, _) = self.queue.book_items(count_fn, u32::MAX)?;

        Ok(Drain {
            queue: &self.queue,
            current: old_head,
            end: new_head,
        })
    }
    pub fn clear<F>(&self, mut dropper: F)
    where F: FnMut(*mut ())
    {
        if let Ok(drain) = self.drain(|count| count) {
            for ptr in drain {
                dropper(ptr);
            }
        }
    }
}

impl<const N: usize> UnwindSafe for Worker<N> {}
impl<const N: usize> RefUnwindSafe for Worker<N> {}
unsafe impl<const N: usize> Send for Worker<N> {}
#[derive(Debug)]
pub struct Drain<'a, const N: usize> {
    queue: &'a Queue<N>,
    current: u32,
    end: u32,
}

impl<'a, const N: usize> Iterator for Drain<'a, N> {
    type Item = *mut ();

    fn next(&mut self) -> Option<*mut ()> {
        if self.current == self.end {
            return None;
        }
        let item = unsafe { self.queue.read_at(self.current) };
        self.current = self.current.wrapping_add(1);
        if self.current == self.end {
            self.queue.stealer_data.head.store(self.end, Release);
        }
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let sz = self.end.wrapping_sub(self.current) as usize;

        (sz, Some(sz))
    }
}

impl<'a, const N: usize> ExactSizeIterator for Drain<'a, N> {}

impl<'a, const N: usize> FusedIterator for Drain<'a, N> {}

impl<'a, const N: usize> Drop for Drain<'a, N> {
    fn drop(&mut self) {
        for _item in self {}
    }
}

impl<'a, const N: usize> UnwindSafe for Drain<'a, N> {}
impl<'a, const N: usize> RefUnwindSafe for Drain<'a, N> {}
unsafe impl<'a, const N: usize> Send for Drain<'a, N> {}
unsafe impl<'a, const N: usize> Sync for Drain<'a, N> {}
#[derive(Debug)]
#[repr(transparent)]
pub struct Stealer<const N: usize> {
    pub queue: &'static Queue<N>,
}

impl<const N: usize> Stealer<N> {
    pub fn steal<C>(&self, dest: &Worker<N>, count_fn: C) -> Result<usize, StealError>
    where
        C: FnMut(usize) -> usize,
    {
        let dest_push_count = dest.queue.push_count.load(Relaxed);
        let dest_pop_count = unpack(dest.queue.stealer_data.pop_count_and_head.load(Relaxed)).0;
        let dest_tail = dest_push_count.wrapping_sub(dest_pop_count);
        let dest_head = dest.queue.stealer_data.head.load(Acquire);
        let dest_free_capacity = N as u32 - dest_tail.wrapping_sub(dest_head);

        let (old_head, new_head, transfer_count) =
            self.queue.book_items(count_fn, dest_free_capacity)?;
        for offset in 0..transfer_count {
            unsafe {
                let item = self.queue.read_at(old_head.wrapping_add(offset));
                dest.queue.write_at(dest_tail.wrapping_add(offset), item);
            }
        }
        dest.queue
            .push_count
            .store(dest_push_count.wrapping_add(transfer_count), Release);
        self.queue.stealer_data.head.store(new_head, Release);

        Ok(transfer_count as usize)
    }
    pub fn steal_and_pop<C>(&self, dest: &Worker<N>, count_fn: C) -> Result<(*mut (), usize), StealError>
    where
        C: FnMut(usize) -> usize,
    {
        let dest_push_count = dest.queue.push_count.load(Relaxed);
        let dest_pop_count = unpack(dest.queue.stealer_data.pop_count_and_head.load(Relaxed)).0;
        let dest_tail = dest_push_count.wrapping_sub(dest_pop_count);
        let dest_head = dest.queue.stealer_data.head.load(Acquire);
        let dest_free_capacity = N as u32 - dest_tail.wrapping_sub(dest_head);

        let (old_head, new_head, count) =
            self.queue.book_items(count_fn, dest_free_capacity + 1)?;
        let transfer_count = count - 1;
        for offset in 0..transfer_count {
            unsafe {
                let item = self.queue.read_at(old_head.wrapping_add(offset));
                dest.queue.write_at(dest_tail.wrapping_add(offset), item);
            }
        }
        let last_item = unsafe { self.queue.read_at(old_head.wrapping_add(transfer_count)) };
        dest.queue
            .push_count
            .store(dest_push_count.wrapping_add(transfer_count), Release);
        self.queue.stealer_data.head.store(new_head, Release);

        Ok((last_item, transfer_count as usize))
    }
}

impl<const N: usize> PartialEq for Stealer<N> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.queue, other.queue)
    }
}
impl<const N: usize> Clone for Stealer<N> {
    fn clone(&self) -> Self { *self }
}
impl<const N: usize> Copy for Stealer<N> {}
impl<const N: usize> Eq for Stealer<N> {}
impl<const N: usize> UnwindSafe for Stealer<N> {}
impl<const N: usize> RefUnwindSafe for Stealer<N> {}
unsafe impl<const N: usize> Send for Stealer<N> {}
unsafe impl<const N: usize> Sync for Stealer<N> {}
