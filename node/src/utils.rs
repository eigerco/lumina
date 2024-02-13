//! Utilities and helpers for running the node.

use celestia_types::ExtendedHeader;
use libp2p::gossipsub::IdentTopic;
use libp2p::multiaddr::{Multiaddr, Protocol};
use libp2p::{PeerId, StreamProtocol};
use tokio::sync::oneshot;

use crate::executor::yield_now;

#[cfg(not(target_arch = "wasm32"))]
pub fn data_dir() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("co", "eiger", "lumina").map(|dirs| dirs.cache_dir().to_owned())
}

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
