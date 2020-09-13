//! A small and fast executor - with only one thread!
//! Copied from https://github.com/stjepang/smol/blob/b3005d942040f68f30ad84b6f8f1621ebaf9d753/src/lib.rs#L149

#![forbid(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

use std::{
    future::Future,
    panic::catch_unwind,
    pin::Pin,
    task::{Context, Poll},
    thread,
};

use multitask::Executor;
use once_cell::sync::Lazy;

#[must_use = "tasks get canceled when dropped, use `.detach()` to run them in the background"]
#[derive(Debug)]
pub struct Task<T>(multitask::Task<T>);

impl<T> Task<T> {
    pub fn spawn<F>(future: F) -> Task<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        static EXECUTOR: Lazy<Executor> = Lazy::new(|| {
            for _ in 0..2 {
                thread::spawn(|| {
                    enter(|| {
                        let (p, u) = async_io::parking::pair();
                        let ticker = EXECUTOR.ticker(move || u.unpark());

                        loop {
                            if let Ok(false) = catch_unwind(|| ticker.tick()) {
                                p.park();
                            }
                        }
                    })
                });
            }

            Executor::new()
        });

        Task(EXECUTOR.spawn(future))
    }

    pub fn detach(self) {
        self.0.detach();
    }
    pub async fn cancel(self) -> Option<T> {
        self.0.cancel().await
    }
}

impl<T> Future for Task<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

/// Enters the tokio context if the `tokio` feature is enabled.
fn enter<T>(f: impl FnOnce() -> T) -> T {
    use std::cell::Cell;
    use tokio::runtime::Runtime;

    thread_local! {
        /// The level of nested `enter` calls we are in, to ensure that the outermost always
        /// has a runtime spawned.
        static NESTING: Cell<usize> = Cell::new(0);
    }

    /// The global tokio runtime.
    static RT: Lazy<Runtime> = Lazy::new(|| Runtime::new().expect("cannot initialize tokio"));

    NESTING.with(|nesting| {
        let res = if nesting.get() == 0 {
            nesting.replace(1);
            RT.enter(f)
        } else {
            nesting.replace(nesting.get() + 1);
            f()
        };
        nesting.replace(nesting.get() - 1);
        res
    })
}
