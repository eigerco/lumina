use std::fmt::{self, Debug};
use std::task::{Context, Poll};

use tokio::sync::oneshot;

use crate::p2p::P2pError;

/// Oneshot sender that responds with error if not used.
pub(super) struct OneshotSender<T> {
    tx: Option<oneshot::Sender<Result<T, P2pError>>>,
    drop_error: Option<P2pError>,
}

impl<T> OneshotSender<T> {
    pub(super) fn new<E>(tx: oneshot::Sender<Result<T, P2pError>>, drop_error: E) -> Self
    where
        E: Into<P2pError>,
    {
        OneshotSender {
            tx: Some(tx),
            drop_error: Some(drop_error.into()),
        }
    }

    pub(super) fn is_closed(&self) -> bool {
        self.tx.as_ref().is_none_or(|tx| tx.is_closed())
    }

    pub(super) fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self.tx {
            Some(ref mut tx) => tx.poll_closed(cx),
            None => Poll::Ready(()),
        }
    }

    pub(super) fn maybe_send(&mut self, result: Result<T, P2pError>) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(result);
        }
    }

    pub(super) fn maybe_send_ok(&mut self, val: T) {
        self.maybe_send(Ok(val));
    }

    pub(super) fn maybe_send_err<E>(&mut self, err: E)
    where
        E: Into<P2pError>,
    {
        self.maybe_send(Err(err.into()));
    }
}

impl<T> Drop for OneshotSender<T> {
    fn drop(&mut self) {
        let drop_error = self.drop_error.take().expect("drop_error not initialized");
        // If sender is dropped without being used, then `drop_error` is send.
        self.maybe_send_err(drop_error);
    }
}

impl<T> Debug for OneshotSender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OneshotSender { .. }")
    }
}
