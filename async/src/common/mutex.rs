//! Cooperative async mutex for `no_std` (no heap required).
//!
//! [`AsyncMutex`] multiplexes exclusive access across tasks and cores: the
//! first locker wins, the rest park their [`Waker`]s in an intrusive queue
//! (nodes live on the waiters' own stacks, so no allocator is involved) and
//! are woken in FIFO order on unlock.
//!
//! Compared to [`spin::Mutex`]: this never spins -- a contended `lock()`.
//! await` suspends the task so the executor can run something else, which is
//! what the multicore story in the README means by "async mutexes". Use
//! [`spin::Mutex`] for sub-microsecond critical sections (e.g. the global
//! task registry), [`AsyncMutex`] for anything that may block.

use core::cell::UnsafeCell;
use core::future::Future;
use core::marker::PhantomPinned;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};
use spin::Mutex;

/// A waiter node linked into [`AsyncMutex::waiters`].
///
/// Lives on the stack of the [`LockFuture`] that registered it; the mutex
/// only stores raw pointers while the future is parked (i.e. pinned and
/// alive), so they never dangle.
struct Waiter {
    waker: Option<Waker>,
    woken: bool,
    next: *mut Waiter,
    prev: *mut Waiter,
}

/// An async-exclusive lock usable from any core and any executor.
///
/// `T: Send` is required because a guard created on one core may be dropped
/// on another after stealing; the data itself is only touched under exclusion.
pub struct AsyncMutex<T> {
    locked: AtomicBool,
    waiters: Mutex<WaitList>,
    data: UnsafeCell<T>,
}

/// Raw intrusive doubly-linked waiter queue (head = oldest waiter).
struct WaitList {
    head: *mut Waiter,
    tail: *mut Waiter,
}

// SAFETY: `AsyncMutex` only hands out `&mut T` under exclusion (the `locked`
// flag), so sharing `&AsyncMutex<T>` across cores/threads is sound as long
// as `T: Send`.
unsafe impl<T: Send> Sync for AsyncMutex<T> {}
unsafe impl<T: Send> Send for AsyncMutex<T> {}

impl<T> AsyncMutex<T> {
    /// Creates an unlocked mutex. `const`-constructible for statics.
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            waiters: Mutex::new(WaitList {
                head: ptr::null_mut(),
                tail: ptr::null_mut(),
            }),
            data: UnsafeCell::new(data),
        }
    }

    /// Tries to lock without parking; `None` when already held.
    pub fn try_lock(&self) -> Option<Guard<'_, T>> {
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(Guard { mutex: self })
        } else {
            None
        }
    }

    /// Locks cooperatively, parking the task until the mutex is free.
    pub fn lock(&self) -> LockFuture<'_, T> {
        LockFuture {
            mutex: self,
            node: Waiter {
                waker: None,
                woken: false,
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
            },
            linked: false,
            _pin: PhantomPinned,
        }
    }

    /// Wakes the oldest waiter, if any. Called with the mutex held-or-free;
    /// the woken task re-contends normally (no lock handoff, no lost wakeup:
    /// a task that finds the mutex free between link and park re-checks).
    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
        let waker = {
            let mut list = self.waiters.lock();
            // Pop the head; skip nodes whose future was dropped (waker taken).
            loop {
                let head = list.head;
                if head.is_null() {
                    break None;
                }
                // SAFETY: `head` points at a live parked future (it unlinks
                // itself on drop before its stack frame goes away).
                let node = unsafe { &mut *head };
                list.head = node.next;
                if list.head.is_null() {
                    list.tail = ptr::null_mut();
                } else {
                    // SAFETY: new head is live (same argument).
                    unsafe { (*list.head).prev = ptr::null_mut() };
                }
                node.next = ptr::null_mut();
                node.prev = ptr::null_mut();
                if let Some(w) = node.waker.take() {
                    node.woken = true;
                    break Some(w);
                }
                // Dead waiter (future dropped without unlinking -- can only
                // happen if `LockFuture` is forgotten; still safe to skip).
            }
        };
        if let Some(w) = waker {
            w.wake();
        }
    }

    /// Links a waiter node at the tail. Caller guarantees the node outlives
    /// the link (it lives on the parked future's stack).
    unsafe fn link(&self, node: *mut Waiter) {
        let mut list = self.waiters.lock();
        // SAFETY: node is live (caller contract).
        unsafe {
            (*node).prev = list.tail;
            (*node).next = ptr::null_mut();
            if list.tail.is_null() {
                list.head = node;
            } else {
                (*list.tail).next = node;
            }
            list.tail = node;
        }
    }

    /// Unlinks a node if still linked. Idempotent and safe for dead nodes:
    /// never dereferences a node that isn't in the list.
    unsafe fn unlink(&self, node: *mut Waiter) {
        let mut list = self.waiters.lock();
        // SAFETY: only dereferenced after confirming membership by walk.
        unsafe {
            let mut cur = list.head;
            while !cur.is_null() {
                if core::ptr::eq(cur, node) {
                    if (*cur).prev.is_null() {
                        list.head = (*cur).next;
                    } else {
                        (*(*cur).prev).next = (*cur).next;
                    }
                    if (*cur).next.is_null() {
                        list.tail = (*cur).prev;
                    } else {
                        (*(*cur).next).prev = (*cur).prev;
                    }
                    (*cur).next = ptr::null_mut();
                    (*cur).prev = ptr::null_mut();
                    break;
                }
                cur = (*cur).next;
            }
        }
    }
}

/// Exclusive guard; releases the mutex (and wakes the next waiter) on drop.
pub struct Guard<'a, T> {
    mutex: &'a AsyncMutex<T>,
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: exclusion guaranteed by construction (only `try_lock` /
        // successful `lock` create guards, unlock happens at most once).
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: same as `deref`; `&mut Guard` is unique.
        unsafe { &mut *self.mutex.data.get() }
    }
}

/// Future returned by [`AsyncMutex::lock`].
///
/// `!Unpin` (pinned by construction): the registered node address must stay
/// stable while parked, and parking only happens behind `Pin`.
pub struct LockFuture<'a, T> {
    mutex: &'a AsyncMutex<T>,
    node: Waiter,
    linked: bool,
    _pin: PhantomPinned,
}

impl<T> Drop for LockFuture<'_, T> {
    fn drop(&mut self) {
        if self.linked {
            // SAFETY: linked nodes are live (self is alive); unlink is
            // idempotent, so double unlink via `unlock`'s pop path is safe.
            unsafe { self.mutex.unlink(&mut self.node as *mut Waiter) };
        }
    }
}

impl<'a, T> Future for LockFuture<'a, T> {
    type Output = Guard<'a, T>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: `LockFuture` is `!Unpin`; the node address stays stable.
        let this = unsafe { self.get_unchecked_mut() };
        // Fast path: free mutex, take it without linking.
        if let Some(guard) = this.mutex.try_lock() {
            if this.linked {
                // We were parked but the mutex freed before our waker ran:
                // unlink (idempotent) and take it.
                // SAFETY: node is live (we are alive); unlink only touches
                // list state found by walk.
                unsafe { this.mutex.unlink(&mut this.node as *mut Waiter) };
                this.linked = false;
            }
            return Poll::Ready(guard);
        }
        // Slow path: link once, then park on our waker.
        if !this.linked {
            // SAFETY: node lives on our (pinned) stack frame.
            unsafe { this.mutex.link(&mut this.node as *mut Waiter) };
            this.linked = true;
        }
        // Re-check after linking: unlock may have raced ahead of our link
        // (unlock pops whoever is linked *now* and wakes them; we just
        // linked, so either we were popped+woken -- handled by `woken` --
        // or a later unlock will wake us; either way no lost wakeup as long
        // as we park the *current* waker every poll).
        if this.node.woken {
            // We were popped and flagged while linking: unlink and retry the
            // fast path on the next poll (waker already consumed).
            // SAFETY: same as above.
            unsafe { this.mutex.unlink(&mut this.node as *mut Waiter) };
            this.linked = false;
            this.node.woken = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        this.node.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
