//! # FIFO, bounded, work-stealing queue.
//!
//! ## Example
//!
//! ```
//! use std::thread;
//! use st3::fifo::Worker;
//!
//! // Push 4 items into a FIFO queue of capacity 256.
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

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::iter::FusedIterator;
use core::mem::{transmute, MaybeUninit};
use core::panic::{RefUnwindSafe, UnwindSafe};
use core::sync::atomic::{AtomicU32, AtomicU64};
use core::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};

use crate::st3::{pack, unpack, StealError};

#[derive(Debug)]
#[repr(align(128))]
struct Align<T>(pub T);

/// A double-ended FIFO work-stealing queue.
///
/// The general operation of the queue is based on tokio's worker queue, itself
/// based on the Go scheduler's worker queue.
///
/// The queue tracks its tail and head position within a ring buffer with
/// wrap-around integers, where the least significant bits specify the actual
/// buffer index. All positions have bit widths that are intentionally larger
/// than necessary for buffer indexing because:
/// - an extra bit is needed to disambiguate between empty and full buffers when
///   the start and end position of the buffer are equal,
/// - the worker head is also used as long-cycle counter to mitigate the risk of
///   ABA.
///
#[derive(Debug)]
#[repr(C, align(128))]
pub struct Queue<T, const N: usize> {
    /// Positions of the head as seen by the worker (most significant bits) and
    /// as seen by a stealer (least significant bits).
    /// 组 1: Worker/Stealer 共同竞争的头部区 (128B)
    heads: Align<AtomicU64>,

    /// 组 2: 数据区 (Buffer)
    /// FIFO 模式下，Buffer 同样充当隔离带
    /// Queue items.
    pub buffer: [UnsafeCell<MaybeUninit<T>>; N],

    /// 组 3: 尾部修改区 (128B)
    /// 只有 Worker 频繁修改 tail
    /// Position of the tail.
    tail: Align<AtomicU32>,
}

impl<T, const N: usize> Queue<T, N> {
    const _CHECK_N: () = assert!(N.is_power_of_two(), "N must be a power of two");
    const MASK: u32 = (N - 1) as u32;

    /// Read an item at the given position.
    ///
    /// The position is automatically mapped to a valid buffer index using a
    /// modulo operation.
    ///
    /// # Safety
    ///
    /// The item at the given position must have been initialized before and
    /// cannot have been moved out.
    ///
    /// The caller must guarantee that the item at this position cannot be
    /// written to or moved out concurrently.
    #[inline(always)]
    unsafe fn read_at(&self, position: u32) -> T {
        unsafe { self.buffer[(position & Self::MASK) as usize].get().read().assume_init() }
    }

    /// Write an item at the given position.
    ///
    /// The position is automatically mapped to a valid buffer index using a
    /// modulo operation.
    ///
    /// # Note
    ///
    /// If an item is already initialized but was not moved out yet, it will be
    /// leaked.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the item at this position cannot be read
    /// or written to concurrently.
    #[inline(always)]
    unsafe fn write_at(&self, position: u32, item: T) {
        unsafe { self.buffer[(position & Self::MASK) as usize].get().write(MaybeUninit::new(item)) }
    }

    /// Attempt to book `N` items for stealing where `N` is specified by a
    /// closure which takes as argument the total count of available items.
    ///
    /// In case of success, the returned tuple contains the stealer head and an
    /// item count at least equal to 1, in this order.
    ///
    /// # Errors
    ///
    /// An error is returned in the following cases:
    /// 1) no item could be stolen, either because the queue is empty or because
    ///    `N` is 0,
    /// 2) a concurrent stealing operation is ongoing.
    ///
    /// # Safety
    ///
    /// This function is not strictly unsafe, but because it initiates the
    /// stealing operation by modifying the worker head without ever updating
    /// the stealer head, its misuse can result in permanently blocking
    /// subsequent stealing operations.
    fn book_items<C>(&self, mut count_fn: C, max_count: u32) -> Result<(u32, u32), StealError>
    where
        C: FnMut(usize) -> usize,
    {
        let mut heads = self.heads.0.load(Acquire);
        loop {
            let (worker_head, stealer_head) = unpack(heads);

            // Bail out if both heads differ because it means another stealing
            // operation is concurrently ongoing.
            if stealer_head != worker_head {
                return Err(StealError::Busy);
            }
            let tail = self.tail.0.load(Acquire);
            let item_count = tail.wrapping_sub(worker_head);

            // `item_count` is tested now because `count_fn` may expect `item_count` to be 0.
            if item_count == 0 {
                return Err(StealError::Empty);
            }

            // Unwind safety: it is OK if `count_fn` panics because no state has been modified.
            let count = (count_fn(item_count as usize).min(max_count as usize) as u32).min(item_count);

            // The special case `count_fn() == 0` must be tested specifically,
            // because if the compare-exchange succeeds with `count=0`, the new
            // worker head will be the same as the old one so other stealers
            // will not detect that stealing is currently ongoing and may try to
            // actually steal items and concurrently modify the position of the
            // heads.
            if count == 0 {
                return Err(StealError::Empty);
            }
            
            // Move the worker head only.
            let new_heads = pack(worker_head.wrapping_add(count), stealer_head);

            // Attempt to book the slots. Only one stealer can succeed since
            // once this atomic is changed, the other thread will necessarily
            // observe a mismatch between the two heads.
            match self.heads.0.compare_exchange_weak(heads, new_heads, Acquire, Acquire) {
                Ok(_) => return Ok((stealer_head, count)),
                // We lost the race to a concurrent pop or steal operation, or
                // the CAS failed spuriously; try again.
                Err(h) => heads = h,
            }
        }
    }
    
    /// Capacity of the queue.
    #[inline(always)]
    fn capacity(&self) -> u32 { N as u32 }
}

impl<T, const N: usize> Drop for Queue<T, N> {
    fn drop(&mut self) {
        let worker_head = unpack(self.heads.0.load(Relaxed)).0;
        let tail = self.tail.0.load(Relaxed);

        let count = tail.wrapping_sub(worker_head);
        for offset in 0..count {
            drop(unsafe { self.read_at(worker_head.wrapping_add(offset)) })
        }
    }
}

/// Handle for single-threaded FIFO push and pop operations.
#[derive(Debug)]
pub struct Worker<T: 'static, const N: usize> {
    queue: &'static Queue<T, N>,
}

impl<T, const N: usize> Worker<T, N> {
    /// Creates a new queue and returns a `Worker` handle.
    ///
    /// **The capacity of a queue is always a power of two**. It is set to the
    /// smallest power of two greater than or equal to the requested minimum
    /// capacity.
    ///
    /// # Panic
    ///
    /// This method will panic if the minimum requested capacity is greater than
    /// 2³¹ on targets that support 64-bit atomics, or greater than 2¹⁵ on
    /// targets that only support 32-bit atomics.
    pub const fn new(queue: &'static Queue<T, N>) -> Self {
        Worker { queue }
    }

    /// Creates a new `Stealer` handle associated to this `Worker`.
    ///
    /// An arbitrary number of `Stealer` handles can be created, either using
    /// this method or cloning an existing `Stealer` handle.
    pub fn stealer(&self) -> Stealer<T, N> {
        Stealer { queue: self.queue, }
    }

    /// Creates a reference to a `Stealer` handle associated to this `Worker`.
    ///
    /// This is a zero-cost reference-to-reference conversion: the reference
    /// count to the underlying queue is not modified. The returned reference
    /// can in particular be used to perform a cheap equality check with another
    /// `Stealer` and verify that it is associated to the same `Worker`.
    pub fn stealer_ref(&self) -> &Stealer<T, N> {
        // Sanity check to assess that `queue` has indeed the size and alignment
        // of a `Stealer` (this assert is optimized away in release mode).
        assert_eq!(Layout::for_value(self), Layout::new::<Stealer<T, N>>());

        // Safety: `self.queue` has the size and alignment of `Stealer` since
        // the latter is a `repr(transparent)` type over an `Arc<Queue<T, N>>`. The
        // lifetime of the returned reference is bounded by the lifetime of
        // `&self`. The soundness of providing a `Stealer` from a `Worker` is
        // already assumed by the `stealer()` method, so providing a short-lived
        // reference to a `Stealer` does not in itself modify safety guarantees.
        unsafe { transmute::<&Self, &Stealer<T, N>>(self) }
    }

    /// Returns the capacity of the queue.
    pub fn capacity(&self) -> usize {
        self.queue.capacity() as usize
    }

    /// Returns the number of items that can be successfully pushed onto the
    /// queue.
    ///
    /// Note that that the spare capacity may be underestimated due to
    /// concurrent stealing operations.
    pub fn spare_capacity(&self) -> usize {
        let stealer_head = unpack(self.queue.heads.0.load(Relaxed)).1;
        let tail = self.queue.tail.0.load(Relaxed);

        // Aggregate count of available items (those which can be popped) and of
        // items currently being stolen.
        let len = tail.wrapping_sub(stealer_head);

        (self.queue.capacity() - len) as usize
    }

    /// Returns true if the queue is empty.
    ///
    /// Note that the queue size is somewhat ill-defined in a multi-threaded
    /// context, but it is warranted that if `is_empty()` returns true, a
    /// subsequent call to `pop()` will fail.
    pub fn is_empty(&self) -> bool {
        let worker_head = unpack(self.queue.heads.0.load(Relaxed)).0;
        let tail = self.queue.tail.0.load(Relaxed);

        tail == worker_head
    }

    /// Attempts to push one item at the tail of the queue.
    ///
    /// # Errors
    ///
    /// This will fail if the queue is full, in which case the item is returned
    /// as the error field.
    pub fn push(&self, item: T) -> Result<(), T> {
        let stealer_head = unpack(self.queue.heads.0.load(Acquire)).1;
        let tail = self.queue.tail.0.load(Relaxed);
        if tail.wrapping_sub(stealer_head) >= N as u32 {
            return Err(item);
        }

        // Store the item.
        unsafe { self.queue.write_at(tail, item) };

        // Make the item visible by moving the tail.
        //
        // Ordering: the Release ordering ensures that the subsequent
        // acquisition of this atomic by a stealer will make the previous write
        // visible.
        self.queue.tail.0.store(tail.wrapping_add(1), Release);
        Ok(())
    }

    /// Attempts to push the content of an iterator at the tail of the queue.
    ///
    /// It is the responsibility of the caller to ensure that there is enough
    /// spare capacity to accommodate all iterator items, for instance by
    /// calling [`spare_capacity`](Worker::spare_capacity) beforehand.
    /// Otherwise, the iterator is dropped while still holding the items in
    /// excess.
    pub fn extend<I: IntoIterator<Item = T>>(&self, iter: I) {
        let stealer_head = unpack(self.queue.heads.0.load(Acquire)).1;
        let mut tail = self.queue.tail.0.load(Relaxed);

        let max_tail = stealer_head.wrapping_add(N as u32);
        for item in iter {
            // Check whether the buffer is full.
            if tail == max_tail {
                break;
            }
            // Store the item.
            unsafe { self.queue.write_at(tail, item) };
            tail = tail.wrapping_add(1);
        }

        // Make the items visible by incrementing the push count.
        //
        // Ordering: the Release ordering ensures that the subsequent
        // acquisition of this atomic by a stealer will make the previous write
        // visible.
        self.queue.tail.0.store(tail, Release);
    }

    /// Attempts to pop one item from the head of the queue.
    ///
    /// This returns None if the queue is empty.
    pub fn pop(&self) -> Option<T> {
        let mut heads = self.queue.heads.0.load(Acquire);

        let prev_worker_head = loop {
            let (worker_head, stealer_head) = unpack(heads);
            let tail = self.queue.tail.0.load(Relaxed);

            // Check if the queue is empty.
            if tail == worker_head {
                return None;
            }

            // Move the worker head. The weird cast from `bool` to
            // `u32` is to steer the compiler towards branchless code.
            let next_heads = pack(
                worker_head.wrapping_add(1),
                stealer_head.wrapping_add((stealer_head == worker_head) as u32),
            );

            // Attempt to book the items.
            match self.queue.heads.0.compare_exchange_weak(heads, next_heads, AcqRel, Acquire) {
                Ok(_) => break worker_head,
                // We lost the race to a stealer or the CAS failed spuriously; try again.
                Err(h) => heads = h,
            }
        };

        unsafe { Some(self.queue.read_at(prev_worker_head)) }
    }

    /// Returns an iterator that steals items from the head of the queue.
    ///
    /// The returned iterator steals up to `N` items, where `N` is specified by
    /// a closure which takes as argument the total count of items available for
    /// stealing. Upon success, the number of items ultimately stolen can be
    /// from 1 to `N`, depending on the number of available items.
    ///
    /// # Beware
    ///
    /// All items stolen by the iterator should be moved out as soon as
    /// possible, because until then or until the iterator is dropped, all
    /// concurrent stealing operations will fail with [`StealError::Busy`].
    ///
    /// # Leaking
    ///
    /// If the iterator is leaked before all stolen items have been moved out,
    /// subsequent stealing operations will permanently fail with
    /// [`StealError::Busy`].
    ///
    /// # Errors
    ///
    /// An error is returned in the following cases:
    /// 1) no item was stolen, either because the queue is empty or `N` is 0,
    /// 2) a concurrent stealing operation is ongoing.
    pub fn drain<C>(&self, count_fn: C) -> Result<Drain<'_, T, N>, StealError>
    where
        C: FnMut(usize) -> usize,
    {
        let (head, count) = self.queue.book_items(count_fn, u32::MAX)?;
        Ok(Drain {
            queue: self.queue,
            head,
            to_head: head.wrapping_add(count),
        })
    }
}

impl<T, const N: usize> UnwindSafe for Worker<T, N> {}
impl<T, const N: usize> RefUnwindSafe for Worker<T, N> {}
unsafe impl<T: Send, const N: usize> Send for Worker<T, N> {}

/// A draining iterator for [`Worker<T, N>`].
///
/// This iterator is created by [`Worker::drain`]. See its documentation for
/// more.
#[derive(Debug)]
pub struct Drain<'a, T, const N: usize> {
    queue: &'a Queue<T, N>,
    head: u32,
    to_head: u32,
}

impl<'a, T, const N: usize> Iterator for Drain<'a, T, N> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.head == self.to_head { return None; }
        let item = unsafe { self.queue.read_at(self.head) };
        self.head = self.head.wrapping_add(1);

        // We cannot rely on the caller to call `next` again after the last item
        // is yielded so the heads must be updated immediately when yielding the
        // last item.
        if self.head == self.to_head {
            // Signal that the stealing operation has completed.
            let mut heads = self.queue.heads.0.load(Relaxed);
            loop {
                let (worker_head, _) = unpack(heads);
                match self.queue.heads.0.compare_exchange_weak(heads, pack(worker_head, worker_head), AcqRel, Acquire) {
                    Ok(_) => break,
                    Err(h) => heads = h,
                }
            }
        }
        Some(item)
    }
}

impl<'a, T, const N: usize> ExactSizeIterator for Drain<'a, T, N> {}

impl<'a, T, const N: usize> FusedIterator for Drain<'a, T, N> {}

impl<'a, T, const N: usize> Drop for Drain<'a, T, N> {
    fn drop(&mut self) {
        // Drop all items and make sure the head is updated so that subsequent
        // stealing operations can succeed.
        for _item in self {}
    }
}

impl<'a, T, const N: usize> UnwindSafe for Drain<'a, T, N> {}
impl<'a, T, const N: usize> RefUnwindSafe for Drain<'a, T, N> {}
unsafe impl<'a, T: Send, const N: usize> Send for Drain<'a, T, N> {}
unsafe impl<'a, T: Send, const N: usize> Sync for Drain<'a, T, N> {}

/// Handle for multi-threaded stealing operations.
#[derive(Debug)]
#[repr(transparent)]
pub struct Stealer<T: 'static, const N: usize> {
    queue: &'static Queue<T, N>,
}

impl<T, const N: usize> Stealer<T, N> {
    /// Attempts to steal items from the head of the queue and move them to the
    /// tail of another queue.
    ///
    /// Up to `N` items are moved to the destination queue, where `N` is
    /// specified by a closure which takes as argument the total count of items
    /// available for stealing. Upon success, the number of items ultimately
    /// transferred to the destination queue can be from 1 to `N`, depending on
    /// the number of available items and the capacity of the destination queue;
    /// the count of transferred items is returned as the success payload.
    ///
    /// # Errors
    ///
    /// An error is returned in the following cases:
    /// 1) no item was stolen, either because the queue is empty, the
    ///    destination is full or `N` is 0,
    /// 2) a concurrent stealing operation is ongoing.
    pub fn steal<C>(&self, dest: &Worker<T, N>, count_fn: C) -> Result<usize, StealError>
    where
        C: FnMut(usize) -> usize,
    {
        // Compute the free capacity of the destination queue.
        //
        // Ordering: see `Worker::push()` method.
        let dest_tail = dest.queue.tail.0.load(Relaxed);
        let dest_stealer_head = unpack(dest.queue.heads.0.load(Acquire)).1;
        let dest_free_capacity = N as u32 - dest_tail.wrapping_sub(dest_stealer_head);

        let (stealer_head, transfer_count) = self.queue.book_items(count_fn, dest_free_capacity)?;

        // Move all items but the last to the destination queue.
        for offset in 0..transfer_count {
            unsafe {
                let item = self.queue.read_at(stealer_head.wrapping_add(offset));
                dest.queue.write_at(dest_tail.wrapping_add(offset), item);
            }
        }
        // Make the moved items visible by updating the destination tail position.
        //
        // Ordering: see comments in the `push()` method.
        dest.queue.tail.0.store(dest_tail.wrapping_add(transfer_count), Release);

        // Signal that the stealing operation has completed.
        let mut heads = self.queue.heads.0.load(Relaxed);
        loop {
            let (worker_head, _) = unpack(heads);
            match self.queue.heads.0.compare_exchange_weak(heads, pack(worker_head, worker_head), AcqRel, Acquire) {
                Ok(_) => return Ok(transfer_count as usize),
                Err(h) => heads = h,
            }
        }
    }


    /// Attempts to steal items from the head of the queue, returning one of
    /// them directly and moving the others to the tail of another queue.
    ///
    /// Up to `N` items are stolen (including the one returned directly), where
    /// `N` is specified by a closure which takes as argument the total count of
    /// items available for stealing. Upon success, one item is returned and
    /// from 0 to `N-1` items are moved to the destination queue, depending on
    /// the number of available items and the capacity of the destination queue;
    /// the number of transferred items is returned as the second field of the
    /// success value.
    ///
    /// The returned item is the most recent one among the stolen items.
    ///
    /// # Errors
    ///
    /// An error is returned in the following cases:
    /// 1) no item was stolen, either because the queue is empty or `N` is 0,
    /// 2) a concurrent stealing operation is ongoing.
    ///
    /// Failure to transfer any item to the destination queue is not considered
    /// an error as long as one element could be returned directly. This can
    /// occur if the destination queue is full, if the source queue has only one
    /// item or if `N` is 1.
    pub fn steal_and_pop<C>(&self, dest: &Worker<T, N>, count_fn: C) -> Result<(T, usize), StealError>
    where
        C: FnMut(usize) -> usize,
    {
        // Compute the free capacity of the destination queue.
        //
        // Ordering: see `Worker::push()` method.
        let dest_tail = dest.queue.tail.0.load(Relaxed);
        let dest_stealer_head = unpack(dest.queue.heads.0.load(Acquire)).1;
        let dest_free_capacity = dest.queue.capacity() - dest_tail.wrapping_sub(dest_stealer_head);

        let (stealer_head, count) = self.queue.book_items(count_fn, dest_free_capacity + 1)?;
        let transfer_count = count - 1;

        // Move all items but the last to the destination queue.
        for offset in 0..transfer_count {
            unsafe {
                let item = self.queue.read_at(stealer_head.wrapping_add(offset));
                dest.queue.write_at(dest_tail.wrapping_add(offset), item);
            }
        }

        // Read the last item.
        let last_item = unsafe {
            self.queue
                .read_at(stealer_head.wrapping_add(transfer_count))
        };

        // Make the moved items visible by updating the destination tail position.
        //
        // Ordering: see comments in the `push()` method.
        dest.queue
            .tail
            .0.store(dest_tail.wrapping_add(transfer_count), Release);

        // Signal that the stealing operation has completed.
        let mut heads = self.queue.heads.0.load(Relaxed);
        loop {
            let (worker_head, _sh) = unpack(heads);

            let res = self.queue.heads.0.compare_exchange_weak(
                heads,
                pack(worker_head, worker_head),
                AcqRel,
                Acquire,
            );

            match res {
                Ok(_) => return Ok((last_item, transfer_count as usize)),
                Err(h) => {
                    heads = h;
                }
            }
        }
    }
}

impl<T, const N: usize> PartialEq for Stealer<T, N> {
    fn eq(&self, other: &Self) -> bool { core::ptr::eq(self.queue, other.queue) }
}
impl<T, const N: usize> Eq for Stealer<T, N> {}
impl<T, const N: usize> UnwindSafe for Stealer<T, N> {}
impl<T, const N: usize> RefUnwindSafe for Stealer<T, N> {}
unsafe impl<T: Send, const N: usize> Send for Stealer<T, N> {}
unsafe impl<T: Send, const N: usize> Sync for Stealer<T, N> {}
