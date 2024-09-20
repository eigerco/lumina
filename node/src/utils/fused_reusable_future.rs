use std::fmt::{self, Debug};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio_util::sync::ReusableBoxFuture;

pub(crate) struct FusedReusableFuture<T> {
    fut: ReusableBoxFuture<'static, T>,
    terminated: bool,
}

impl<T: 'static> FusedReusableFuture<T> {
    pub(crate) fn terminated() -> Self {
        FusedReusableFuture {
            fut: ReusableBoxFuture::new(std::future::pending()),
            terminated: true,
        }
    }

    pub(crate) fn terminate(&mut self) {
        if !self.terminated {
            self.fut.set(std::future::pending());
            self.terminated = true;
        }
    }

    pub(crate) fn is_terminated(&self) -> bool {
        self.terminated
    }

    pub(crate) fn set<F>(&mut self, future: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        self.fut.set(future);
        self.terminated = false;
    }

    pub(crate) fn poll(&mut self, cx: &mut Context) -> Poll<T> {
        if self.terminated {
            return Poll::Pending;
        }

        match self.fut.poll(cx) {
            Poll::Ready(val) => {
                self.terminated = true;
                Poll::Ready(val)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: 'static> Debug for FusedReusableFuture<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FusedReusableFuture { .. }")
    }
}

impl<T: 'static> Future for FusedReusableFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<T> {
        self.get_mut().poll(cx)
    }
}
