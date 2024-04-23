use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use celestia_types::ExtendedHeader;
use libp2p::gossipsub::IdentTopic;
use libp2p::multiaddr::{Multiaddr, Protocol};
use libp2p::{PeerId, StreamProtocol};
use tokio::sync::oneshot;
use tokio_util::sync::ReusableBoxFuture;

use crate::executor::yield_now;

pub(crate) const VALIDATIONS_PER_YIELD: usize = 4;

pub(crate) fn protocol_id(network: &str, protocol: &str) -> StreamProtocol {
    let network = network.trim_matches('/');
    let protocol = protocol.trim_matches('/');
    let s = format!("/{network}/{protocol}");
    StreamProtocol::try_from_owned(s).expect("does not start from '/'")
}

pub(crate) fn celestia_protocol_id(network: &str, protocol: &str) -> StreamProtocol {
    let network = network.trim_matches('/');
    let network = format!("/celestia/{network}");
    protocol_id(&network, protocol)
}

pub(crate) fn gossipsub_ident_topic(network: &str, topic: &str) -> IdentTopic {
    let network = network.trim_matches('/');
    let topic = topic.trim_matches('/');
    let s = format!("/{network}/{topic}");
    IdentTopic::new(s)
}

pub(crate) fn fraudsub_ident_topic(proof_type: &str, network: &str) -> IdentTopic {
    let proof_type = proof_type.trim_matches('/');
    let network = network.trim_matches('/');
    let s = format!("/{proof_type}/fraud-sub/{network}/v0.0.1");
    IdentTopic::new(s)
}

pub(crate) type OneshotResultSender<T, E> = oneshot::Sender<Result<T, E>>;

pub(crate) trait OneshotSenderExt<T>
where
    T: Send + 'static,
{
    fn maybe_send(self, val: T);
}

impl<T> OneshotSenderExt<T> for oneshot::Sender<T>
where
    T: Send + 'static,
{
    fn maybe_send(self, val: T) {
        let _ = self.send(val);
    }
}

pub(crate) trait OneshotResultSenderExt<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    fn maybe_send_ok(self, val: T);
    fn maybe_send_err(self, err: impl Into<E>);
}

impl<T, E> OneshotResultSenderExt<T, E> for oneshot::Sender<Result<T, E>>
where
    T: Send + 'static,
    E: Send + 'static,
{
    fn maybe_send_ok(self, val: T) {
        let _ = self.send(Ok(val));
    }

    fn maybe_send_err(self, err: impl Into<E>) {
        let _ = self.send(Err(err.into()));
    }
}

pub(crate) trait MultiaddrExt {
    fn peer_id(&self) -> Option<PeerId>;
}

impl MultiaddrExt for Multiaddr {
    fn peer_id(&self) -> Option<PeerId> {
        self.iter().find_map(|proto| match proto {
            Protocol::P2p(peer_id) => Some(peer_id),
            _ => None,
        })
    }
}

pub(crate) async fn validate_headers(headers: &[ExtendedHeader]) -> celestia_types::Result<()> {
    for headers in headers.chunks(VALIDATIONS_PER_YIELD) {
        for header in headers {
            header.validate()?;
        }

        // Validation is computation heavy so we yield on every chunk
        yield_now().await;
    }

    Ok(())
}

pub(crate) struct FusedReusableBoxFuture<T> {
    fut: ReusableBoxFuture<'static, T>,
    terminated: bool,
}

impl<T: 'static> FusedReusableBoxFuture<T> {
    pub(crate) fn terminated() -> Self {
        FusedReusableBoxFuture {
            fut: ReusableBoxFuture::new(std::future::pending()),
            terminated: true,
        }
    }

    pub(crate) fn is_terminated(&self) -> bool {
        self.terminated
    }

    pub(crate) fn terminate(&mut self) {
        if !self.terminated {
            self.fut.set(std::future::pending());
            self.terminated = true;
        }
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

impl<T: 'static> Future for FusedReusableBoxFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<T> {
        self.get_mut().poll(cx)
    }
}
