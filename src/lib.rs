use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::task::AtomicWaker;

#[derive(Default)]
pub struct WaitGroup {
    inner: Arc<Inner>,
}

impl WaitGroup {
    pub fn new() -> Self {
        WaitGroup::default()
    }

    pub fn worker(&mut self) -> Worker {
        self.inner.count.fetch_add(1, Ordering::SeqCst);
        self.inner.remaining.fetch_add(1, Ordering::SeqCst);

        Worker {
            inner: self.inner.clone(),
            generation: self.inner.generation.load(Ordering::SeqCst),
        }
    }

    pub async fn wait(&self) {
        WaitGroupFuture::new(self.inner.clone()).await;
    }
}

pub struct Worker {
    generation: usize,
    inner: Arc<Inner>,
}

impl Worker {
    /// Notify the [`WaitGroup`] that this worker has finished execution.
    ///
    /// Calling this twice is a noop until [`WaitGroup::wait`] is called.
    pub fn done(&mut self) {
        if self.generation == self.inner.generation.load(Ordering::SeqCst) {
            self.generation = self.generation.wrapping_add(1);
            self.inner.remaining.fetch_sub(1, Ordering::SeqCst);
            if let Some(waker) = self.inner.waker.take() {
                waker.wake();
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.inner.count.fetch_sub(1, Ordering::SeqCst);
        self.done()
    }
}

#[derive(Default)]
struct Inner {
    count: AtomicUsize,
    remaining: AtomicUsize,
    generation: AtomicUsize,
    waker: AtomicWaker,
}

struct WaitGroupFuture {
    inner: Arc<Inner>,
}

impl WaitGroupFuture {
    fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }
}

impl Future for WaitGroupFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.waker.register(cx.waker());
        match self.inner.remaining.load(Ordering::SeqCst) {
            0 => {
                self.inner
                    .remaining
                    .store(self.inner.count.load(Ordering::SeqCst), Ordering::SeqCst);
                self.inner.generation.fetch_add(1, Ordering::SeqCst);
                Poll::Ready(())
            }
            _ => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::time::sleep;
    use tokio_test::{assert_pending, assert_ready, task};

    use crate::WaitGroup;

    #[tokio::test]
    async fn test_work_group() {
        let mut wg = WaitGroup::new();
        for _ in 0..1 {
            let mut wg = wg.worker();
            tokio::spawn(async move {
                sleep(Duration::from_secs(1)).await;
                wg.done();
            });
        }

        wg.wait().await;
    }

    #[tokio::test]
    async fn test_drop() {
        let mut wg = WaitGroup::new();
        let worker = wg.worker();

        let mut fut = task::spawn(async move {
            wg.wait().await;
        });

        assert_pending!(fut.poll());
        drop(worker);
        assert_ready!(fut.poll());
    }

    #[tokio::test]
    async fn test_ready() {
        let mut wg = WaitGroup::new();
        let mut worker1 = wg.worker();
        let mut worker2 = wg.worker();

        worker1.done();
        worker1.done();

        let mut fut = task::spawn(async move {
            wg.wait().await;
        });

        assert_pending!(fut.poll());
        worker2.done();
        assert_ready!(fut.poll());
    }

    #[tokio::test]
    async fn test_ready_followed_by_drop() {
        let mut wg = WaitGroup::new();
        let mut worker1 = wg.worker();
        let mut worker2 = wg.worker();
        let mut worker3 = wg.worker();

        worker1.done();
        worker2.done();
        drop(worker2);

        let mut fut = task::spawn(async move {
            wg.wait().await;
        });

        assert_pending!(fut.poll());
        worker3.done();
        assert_ready!(fut.poll());
    }

    #[tokio::test]
    async fn test_work_group_reuse() {
        let mut wg = WaitGroup::new();
        let mut worker = wg.worker();

        let mut fut = task::spawn(async {
            wg.wait().await;
        });

        assert_pending!(fut.poll());
        worker.done();
        assert_ready!(fut.poll());

        let mut fut = task::spawn(async {
            wg.wait().await;
        });

        assert_pending!(fut.poll());
        worker.done();
        assert_ready!(fut.poll());
    }

    #[tokio::test]
    async fn test_generation_overflow() {
        let mut wg = WaitGroup::new();
        wg.inner.generation.store(usize::MAX, Ordering::SeqCst);
        let mut worker = wg.worker();

        let mut fut = task::spawn(async {
            wg.wait().await;
        });

        assert_pending!(fut.poll());
        worker.done();
        assert_ready!(fut.poll());

        let mut fut = task::spawn(async {
            wg.wait().await;
        });

        assert_pending!(fut.poll());
        worker.done();
        assert_ready!(fut.poll());
    }
}
