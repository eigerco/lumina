use libp2p::gossipsub::IdentTopic;
use libp2p::StreamProtocol;
use tokio::sync::oneshot;

pub(crate) fn stream_protocol_id(network: &str, protocol: &str) -> StreamProtocol {
    let protocol = protocol.trim_start_matches('/');
    let s = format!("/{network}/{protocol}");
    StreamProtocol::try_from_owned(s).expect("does not start from '/'")
}

pub(crate) fn gossipsub_ident_topic(network: &str, topic: &str) -> IdentTopic {
    let topic = topic.trim_start_matches('/');
    let s = format!("/{network}/{topic}");
    IdentTopic::new(s)
}

pub(crate) trait OneshotSenderExt<T: Send + 'static> {
    fn maybe_send(self, val: T);
}

impl<T: Send + 'static> OneshotSenderExt<T> for oneshot::Sender<T> {
    fn maybe_send(self, val: T) {
        let _ = self.send(val);
    }
}
