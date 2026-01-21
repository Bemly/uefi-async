

pub mod multiple {
    pub use futures_concurrency::*;
}

pub mod single {
    pub mod join {
        use core::future::Future;
        use core::pin::Pin;
        use core::task::{Context, Poll};
        use pin_project::pin_project;

        /// Joins multiple futures together, running them concurrently until all complete.
        ///
        /// This macro expands to a nested `Join` structure. It does not return any values
        /// and is intended for tasks that perform side effects (returning `()`).
        ///
        /// # Examples
        /// ```
        /// join!(task_one(), task_two(), async { do_something().await }).await;
        /// ```
        pub use uefi_async_macros::join;

        /// Joins multiple `Result`-returning futures, short-circuiting on the first error.
        ///
        /// If any future returns an `Err`, the `try_join!` completes immediately with that error.
        /// Otherwise, it waits until all tasks complete successfully.
        ///
        /// # Examples
        /// ```
        /// let result = try_join!(disk_load(), network_fetch()).await;
        /// if result.is_err() {
        ///     println!("One of the tasks failed!");
        /// }
        /// ```
        pub use uefi_async_macros::try_join;

        /// Joins multiple futures and collects their results into a flattened tuple.
        ///
        /// Unlike `join!`, `join_all!` preserves the output of each future. The macro
        /// automatically flattens the internal recursive structure so you receive a
        /// standard tuple of results.
        ///
        /// # Examples
        /// ```
        /// let (mesh, texture) = join_all!(load_mesh(), load_texture()).await;
        /// render_engine.draw(mesh, texture);
        /// ```
        pub use uefi_async_macros::join_all;

        /// A Future that polls two sub-futures to completion.
        ///
        /// This is the primitive building block for the `join!` macro. It stores the completion
        /// state of two futures and returns `Ready` only when both are done.
        #[pin_project]
        pub struct Join<H, T> {
            #[pin] pub head: H,
            #[pin] pub tail: T,
            pub head_done: bool,
            pub tail_done: bool,
        }

        impl<H: Future<Output = ()>, T: Future<Output = ()>> Future for Join<H, T> {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.project();

                if !*this.head_done { *this.head_done = this.head.poll(cx).is_ready() }
                if !*this.tail_done { *this.tail_done = this.tail.poll(cx).is_ready() }
                if *this.head_done && *this.tail_done { Poll::Ready(()) } else { Poll::Pending }
            }
        }

        /// A Future that polls two `Result`-returning sub-futures with short-circuiting logic.
        ///
        /// If either `head` or `tail` returns `Err`, this Future resolves to that `Err` immediately.
        /// It is the foundation for the `try_join!` macro.
        #[pin_project]
        pub struct TryJoin<H, T> {
            #[pin] pub head: H,
            #[pin] pub tail: T,
            pub head_done: bool,
            pub tail_done: bool,
        }

        impl<E, H: Future<Output = Result<(),E>>, T: Future<Output = Result<(),E>>> Future for TryJoin<H, T> {
            type Output = Result<(), E>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.project();

                // 1. 轮询第一个任务 (Head)
                if !*this.head_done {
                    match this.head.poll(cx) {
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)), // 遇到错误立刻中断
                        Poll::Ready(Ok(_)) => *this.head_done = true,     // 标记成功
                        Poll::Pending => {}
                    }
                }

                // 2. 轮询第二个任务 (Tail)
                if !*this.tail_done {
                    match this.tail.poll(cx) {
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)), // 遇到错误立刻中断
                        Poll::Ready(Ok(_)) => *this.tail_done = true,     // 标记成功
                        Poll::Pending => {}
                    }
                }

                // 3. 只有当两者都成功时，才返回 Ok
                if *this.head_done && *this.tail_done { Poll::Ready(Ok(())) } else { Poll::Pending }
            }
        }

        /// A Future that polls two sub-futures and stores their outputs.
        ///
        /// Once both futures resolve, the outputs are returned as a tuple.
        #[pin_project]
        pub struct JoinAll<H: Future, T: Future> {
            #[pin] pub head: H,
            #[pin] pub tail: T,
            pub head_res: Option<H::Output>,
            pub tail_res: Option<T::Output>,
        }

        impl<H: Future, T: Future> JoinAll<H, T> {
            pub fn new(head: H, tail: T) -> Self { Self { head, tail, head_res: None, tail_res: None } }
        }

        impl<H: Future, T: Future> Future for JoinAll<H, T> {
            type Output = (H::Output, T::Output);
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.project();

                if this.head_res.is_none() {
                    if let Poll::Ready(out) = this.head.poll(cx) {
                        *this.head_res = Some(out);
                    }
                }
                if this.tail_res.is_none() {
                    if let Poll::Ready(out) = this.tail.poll(cx) {
                        *this.tail_res = Some(out);
                    }
                }

                if this.head_res.is_some() && this.tail_res.is_some() {
                    let h = unsafe { this.head_res.take().unwrap_unchecked() };
                    let t = unsafe { this.tail_res.take().unwrap_unchecked() };
                    Poll::Ready((h, t))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

