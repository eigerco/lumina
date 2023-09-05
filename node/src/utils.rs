use libp2p::gossipsub::IdentTopic;
use libp2p::StreamProtocol;

pub fn stream_protocol_id(network: &str, protocol: &str) -> StreamProtocol {
    let protocol = protocol.trim_start_matches('/');
    let s = format!("/{network}/{protocol}");
    StreamProtocol::try_from_owned(s).expect("does not start from '/'")
}

pub fn gossipsub_ident_topic(network: &str, topic: &str) -> IdentTopic {
    let topic = topic.trim_start_matches('/');
    let s = format!("/{network}/{topic}");
    IdentTopic::new(s)
}
