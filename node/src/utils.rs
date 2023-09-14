use libp2p::gossipsub::IdentTopic;
use libp2p::StreamProtocol;
use tokio::sync::oneshot;

pub(crate) fn stream_protocol_id(network: &str, protocol: &str) -> StreamProtocol {
    let network = network.trim_matches('/');
    let protocol = protocol.trim_matches('/');
    let s = format!("/{network}/{protocol}");
    StreamProtocol::try_from_owned(s).expect("does not start from '/'")
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
